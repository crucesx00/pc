use crate::error::{Error, Result};
use crate::identity::{Identifier, Identity, PublicIdentity};
use crate::keys::Nonce;
use futures::SinkExt;
use rmp_serde::{decode, encode};
use serde_derive::{Deserialize, Serialize};
use slog::*;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::stream::StreamExt;
use tokio_serde_msgpack::{DecodeError, EncodeError, MsgPackDecoder, MsgPackEncoder};
use tokio_util::codec::{FramedRead, FramedWrite};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Protocol {
    ServerIdentity(PublicIdentity),
    ClientIdentity(PublicIdentity, Nonce),
    ChatMessage(Identifier, Vec<u8>),
}

impl Protocol {
    pub fn encode(&self) -> Result<Vec<u8>> {
        Ok(encode::to_vec(self)?)
    }

    pub fn decode(message: &[u8]) -> Result<Protocol> {
        Ok(decode::from_read_ref(message)?)
    }
}

pub async fn server_handshake<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    identity: &Identity,
    nonce: &Nonce,
    logger: &Logger,
) -> Result<PublicIdentity> {
    debug!(logger, "Server PRiVY handshake");
    let server_id = get_server_id(stream, logger).await?;
    debug!(logger, "server_handshake got server ID {:?}", server_id);

    send_client_identity(stream, identity, &server_id, nonce.clone(), logger).await?;

    Ok(server_id)
}

async fn get_server_id<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    logger: &Logger,
) -> Result<PublicIdentity> {
    // let (rx, _) = split(stream);
    let mut reader = FramedRead::new(stream, MsgPackDecoder::<Protocol>::new());
    debug!(logger, "server_handshake, reading server ID");
    match reader.next().await {
        None => Err(Error::Error("Unexpected EOF".into())),
        Some(Ok(Protocol::ServerIdentity(public_identity))) => Ok(public_identity),
        Some(Ok(protocol)) => Err(Error::ProtocolError(format!(
            "Got unexpected protocol message {:?}",
            protocol
        ))),
        Some(Err(DecodeError::IO(error))) => Err(Error::IOError(error)),
        Some(Err(DecodeError::Decode(error))) => Err(Error::MPDecodeError(error)),
    }
}

async fn send_client_identity<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    identity: &Identity,
    server_id: &PublicIdentity,
    nonce: Nonce,
    logger: &Logger,
) -> Result<()> {
    let encoded = Protocol::ClientIdentity(identity.public_identity(), nonce).encode()?;
    let encrypted = server_id.encrypt_anonymous(&encoded);
    debug!(logger, "server_handshake writing {:?}", encrypted);
    stream.write(&encrypted).await?;

    Ok(())
}

pub async fn client_handshake<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    identity: &Identity,
    logger: &Logger,
) -> Result<(PublicIdentity, Nonce)> {
    debug!(logger, "Client handshake");
    debug!(logger, "Sending server identity");
    send_server_identity(stream, identity).await?;

    debug!(logger, "Getting client ID");
    get_client_id(stream, identity, logger).await
}

async fn send_server_identity<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    identity: &Identity,
) -> Result<()> {
    // let (_, tx) = split(stream);
    let mut writer = FramedWrite::new(stream, MsgPackEncoder::<Protocol>::new());
    match writer
        .send(Protocol::ServerIdentity(identity.public_identity()))
        .await
    {
        Ok(()) => Ok(()),
        Err(EncodeError::IO(error)) => Err(Error::IOError(error)),
        Err(EncodeError::Encode(error)) => Err(Error::MPEncodeError(error)),
    }
}

async fn get_client_id<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    identity: &Identity,
    logger: &Logger,
) -> Result<(PublicIdentity, Nonce)> {
    let mut buffer = [0; 256];
    let bytes_read = stream.read(&mut buffer).await?;
    debug!(logger, "client_handshake read {:?}", &buffer[..bytes_read]);
    Ok(
        match Protocol::decode(&identity.decrypt_anonymous(&buffer[..bytes_read])?)? {
            Protocol::ClientIdentity(client_id, nonce) => Ok((client_id, nonce)),
            _ => Err(Error::ProtocolError("Unexpected protocol message".into())),
        }?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::async_read_write::tests::MockAsyncReadWrite;
    use crate::error::Result;
    use crate::identity::Identity;
    use crate::keys::Nonce;
    use slog::*;
    use sodiumoxide::crypto::box_::gen_nonce;
    use std::sync::Once;
    use tokio;

    fn setup_logging() -> Logger {
        let decorator = slog_term::TermDecorator::new().build();
        let drain = slog_term::FullFormat::new(decorator).build().fuse();
        let drain = slog_async::Async::new(drain).build().fuse();
        let drain = LevelFilter(drain, Level::Warning).fuse();

        slog::Logger::root(drain, o!())
    }

    #[tokio::test(threaded_scheduler)]
    async fn test_handshake() -> Result<()> {
        let logger = setup_logging();
        debug!(logger, "Starting");
        let (mut server_stream, mut client_stream) = MockAsyncReadWrite::pair(&logger);
        let server_identity = Identity::new("server");
        let id = server_identity.clone();
        let mut logger_copy = logger.clone();
        let server_handle =
            tokio::spawn(
                async move { client_handshake(&mut server_stream, &id, &logger_copy).await },
            );
        let client_identity = Identity::new("client");
        let id = client_identity.clone();
        let nonce: Nonce = gen_nonce().into();
        logger_copy = logger.clone();
        let client_handle = tokio::spawn(async move {
            server_handshake(&mut client_stream, &id, &nonce, &logger_copy).await
        });

        let (client_id, _nonce) = server_handle.await.unwrap()?;
        assert_eq!(client_identity.public_identity(), client_id);
        let server_id = client_handle.await.unwrap()?;
        assert_eq!(server_identity.public_identity(), server_id);

        Ok(())
    }
}
