use crate::error::Result;
use crate::io_bus::{IOBusClient, IOBusReceiver};
use bytes::Bytes;
use futures::SinkExt;
use slog::*;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::net::TcpStream;
use tokio::select;
use tokio::stream::{Stream, StreamExt};
use tokio_util::codec::{BytesCodec, Framed};

#[derive(Debug)]
pub struct NetworkStream {
    bytes: Framed<TcpStream, BytesCodec>,
}

impl NetworkStream {
    fn new(network_connection: TcpStream) -> Self {
        Self {
            bytes: Framed::new(network_connection, BytesCodec::new()),
        }
    }

    async fn send(&mut self, message: Bytes) -> Result<()> {
        Ok(self.bytes.send(message).await?)
    }

    fn shutdown(&mut self) -> Result<()> {
        Ok(self.bytes.get_ref().shutdown(std::net::Shutdown::Both)?)
    }
}

/// Stream interface for NetworkStream
impl Stream for NetworkStream {
    type Item = std::result::Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Receive a message from the network connection
        let result: Option<_> = futures::ready!(Pin::new(&mut self.bytes).poll_next(cx));

        Poll::Ready(match result {
            // Mark the message as outgoing
            Some(Ok(message)) => Some(Ok(message.into())),

            // An error occured.
            Some(Err(e)) => Some(Err(e)),

            // The stream has been exhausted.
            None => None,
        })
    }
}

/// Handle the network side of the chat
pub async fn handle_network_connection(
    connection: TcpStream,
    mut receiver: IOBusReceiver,
    mut client: IOBusClient,
    logger: &Logger,
) {
    let logger = logger.clone();
    debug!(
        logger,
        "handle_network_connection({}, {}, {})",
        connection.peer_addr().unwrap(),
        receiver.id,
        client.id
    );
    let addr = connection.peer_addr().unwrap();
    let mut network_stream = NetworkStream::new(connection);
    loop {
        // network_result is Option<Result<Bytes>>
        // rx_result is Option<Bytes>
        select! {
            network_result = network_stream.next() => {
                debug!(logger, "network_result = {:?}", network_result);
                match network_result {
                    Some(Ok(message)) => {
                        debug!(logger, "Got message from network: {:?}", message);
                        client.broadcast(message).await;
                    }
                    Some(Err(error)) => error!(logger, "Error reading from network: {}", error),
                    None => {
                        info!(logger, "Network connection {:?} dropped", addr);
                        client.shutdown().await;
                        break;
                    }
                }
            },

            rx_result = receiver.next() => {
                debug!(logger, "rx_result = {:?}", rx_result);
                match rx_result {
                    Some(message) => {
                        debug!(logger, "Sending '{:?}' over the wire", message);
                        if let Err(error) = network_stream.send(message).await {
                            error!(logger, "Error sending message on network: {}", error);
                        }
                    }
                    None => {
                        if let Err(error) = network_stream.shutdown() {
                            error!(logger, "Error shutting down network connection: {}", error);
                        };
                        break;
                    }
                }
            }
        }
    }
}
