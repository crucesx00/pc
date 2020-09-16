use crate::async_read_write::AsyncReadWrite;
use crate::error::{Error, Result};
use crate::identity::Identity;
use crate::io_bus::{IOBusClient, IOBusReceiver};
use crate::protocol::Protocol;
use bytes::Bytes;
use futures::SinkExt;
use slog::*;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::select;
use tokio::stream::{Stream, StreamExt};
use tokio_util::codec::{Framed, LinesCodec};

/// Module to handle Terminal I/O
pub struct TerminalStream<
    In: AsyncRead + Unpin + Send + Sync + 'static,
    Out: AsyncWrite + Unpin + Send + Sync + 'static,
> {
    lines: Framed<AsyncReadWrite<In, Out>, LinesCodec>,
    logger: Logger,
}

impl<
        In: AsyncRead + Unpin + Send + Sync + 'static,
        Out: AsyncWrite + Unpin + Send + Sync + 'static,
    > TerminalStream<In, Out>
{
    /// Wrap terminal input and output in this module
    pub async fn new(input: In, output: Out, logger: &Logger) -> Self {
        Self {
            lines: Framed::new(AsyncReadWrite::new(input, output), LinesCodec::new()),
            logger: logger.new(o!()),
        }
    }

    /// Send data to the terminal
    async fn send(&mut self, message: String) -> Result<()> {
        debug!(
            self.logger,
            "TerminalStream, sending {:?} to lines", message
        );
        Ok(self.lines.send(message).await?)
    }

    /// Decrypt an encrypted message
    fn decrypt(&self, msg: Bytes) -> Result<Bytes> {
        Ok(msg)
    }

    /// Encrypt a message to be sent out
    fn encrypt(&self, msg: &[u8]) -> Bytes {
        Bytes::copy_from_slice(msg)
    }
}

/// Stream interface for TerminalStream
impl<
        In: AsyncRead + Unpin + Send + Sync + 'static,
        Out: AsyncWrite + Unpin + Send + Sync + 'static,
    > Stream for TerminalStream<In, Out>
{
    type Item = std::result::Result<String, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Receive a message from the terminal
        let result: Option<_> = futures::ready!(Pin::new(&mut self.lines).poll_next(cx));
        debug!(
            self.logger,
            "TerminalStream, lines poll_next got {:?}", result
        );

        Poll::Ready(match result {
            // We've received a message we should broadcast to others.
            Some(Ok(message)) => Some(Ok(message)),

            // An error occured.
            Some(Err(e)) => Some(Err(e.into())),

            // The stream has been exhausted.
            None => None,
        })
    }
}

/// Handle the "terminal" side of the chat
pub async fn handle_terminal_io<
    In: AsyncRead + Unpin + Send + Sync + 'static,
    Out: AsyncWrite + Unpin + Send + Sync + 'static,
>(
    input: In,
    output: Out,
    identity: Identity,
    mut receiver: IOBusReceiver,
    mut client: IOBusClient,
    logger: &Logger,
) -> Result<()> {
    debug!(logger, "handle_terminal_io");
    let mut terminal_stream = TerminalStream::new(input, output, logger).await;
    loop {
        debug!(logger, "handle_terminal_io, entering main loop");
        select! {
            terminal_result = terminal_stream.next() => {
                debug!(logger, "terminal_result = {:?}", terminal_result);
                match terminal_result {
                    Some(Ok(message)) => {
                        match Protocol::ChatMessage(identity.identifier.clone(), terminal_stream.encrypt(message.as_ref()).to_vec()).encode() {
                            Ok(data) => client.broadcast(data.into()).await,
                            Err(error) => error!(logger, "Error encoding message: {}", error),
                        }
                    }
                    Some(Err(error)) => error!(logger, "Error reading from terminal: {}", error),
                    None => {
                        client.shutdown().await;
                        break;
                    }
                }
            },
            receiver_result = receiver.next() => {
                debug!(logger, "receiver_result = {:?}", receiver_result);
                if let Some(message) = receiver_result {
                    match Protocol::decode(message.as_ref()) {
                        Err(error) => error!(logger, "Error decoding message: {}", error),
                        Ok(Protocol::ChatMessage(client_id, encrypted)) => {
                            match terminal_stream.decrypt(encrypted.into()) {
                                Ok(decrypted) => match std::str::from_utf8(decrypted.as_ref()) {
                                    Ok(string) => {
                                        if let Err(error) = terminal_stream.send(format!("{}: {}", client_id, string)).await {
                                            error!(logger, "Error sending to terminal: {}", error);
                                        }
                                    }
                                    Err(error) => error!(logger, "Error converting bytes to string: {}", error),
                                },
                                Err(error) => error!(logger, "Error decrypting message: {}", error),
                            }
                        },
                        Ok(proto_message) => error!(logger, "Unexpected protocol message: {:?}", proto_message),
                    }
                }
            }
        }
    }

    Ok(())
}
