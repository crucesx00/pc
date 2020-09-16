use super::parse_socket_addr;
use crate::error::Result;
use crate::identity::Identity;
use crate::io_bus::IOBus;
use crate::keys::gen_nonce;
use crate::network_io::handle_network_connection;
use crate::protocol::server_handshake;
use crate::term_io::handle_terminal_io;
use slog::*;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

pub struct ChatClient<
    In: AsyncRead + Unpin + Send + Sync + 'static,
    Out: AsyncWrite + Unpin + Send + Sync + 'static,
> {
    input: In,
    output: Out,
    stream: TcpStream,
    remote_name: String,
    identity: Identity,
    logger: Logger,
}

impl<
        In: AsyncRead + Unpin + Send + Sync + 'static,
        Out: AsyncWrite + Unpin + Send + Sync + 'static,
    > ChatClient<In, Out>
{
    pub async fn new(
        host: &str,
        port: &str,
        identity: Identity,
        input: In,
        output: Out,
        logger: &Logger,
    ) -> Result<Self> {
        let addr = parse_socket_addr(host, port)?;
        let mut stream = TcpStream::connect(addr).await?;
        let server_nonce = gen_nonce();
        let _server_id = server_handshake(&mut stream, &identity, &server_nonce, logger).await?;
        let remote_name = stream.peer_addr()?.to_string();
        let child_logger = logger.new(o!("client" => identity.identifier.to_string()));
        info!(child_logger, "Client connected to chat server at {}", addr);

        Ok(Self {
            input,
            output,
            stream: stream,
            remote_name,
            identity,
            logger: child_logger,
        })
    }

    pub async fn start(self) -> Result<()> {
        let logger = self.logger.clone();
        debug!(logger, "PRiVY Chat client starting");
        let input = self.input;
        let output = self.output;
        let remote_name = self.remote_name;
        let stream = self.stream;
        let mut bus = IOBus::new(&logger);
        let (receiver, client) = bus.get_channel().await;
        debug!(
            self.logger,
            "Chat network side got receiver {}, client {}", receiver, client
        );
        let identifier = self.identity.identifier.clone();

        // Handle connection to the server
        debug!(logger, "Chat client spawning handle_network_connection",);
        tokio::spawn(async move {
            handle_network_connection(stream, receiver, client, &logger).await;
            debug!(
                logger,
                "Chat client {} connection to {} exiting", identifier, remote_name
            );
        });

        // Handle terminal side
        debug!(self.logger, "Chat client calling handle_terminal_io",);
        let (receiver, client) = bus.get_channel().await;
        debug!(
            self.logger,
            "Chat terminal side got receiver {}, client {}", receiver, client
        );
        if let Err(e) =
            handle_terminal_io(input, output, self.identity, receiver, client, &self.logger).await
        {
            error!(self.logger, "Client error handling local user input: {}", e);
        }
        debug!(self.logger, "Chat client exiting");

        Ok(())
    }
}
