use super::parse_socket_addr;
use crate::error::{Error, Result};
use crate::identity::Identity;
use crate::io_bus::IOBus;
use crate::network_io::handle_network_connection;
use crate::protocol::client_handshake;
use futures::future::{abortable, AbortHandle};
use slog::{debug, error, o, Logger};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

/// Chat server
#[derive(Debug)]
pub struct ChatServer {
    identity: Identity,
    addr: SocketAddr,
    abort_handle: Arc<Mutex<Option<AbortHandle>>>,
    logger: Logger,
}

pub struct StopHandle {
    tx: tokio::sync::oneshot::Sender<()>,
}

impl StopHandle {
    fn new(tx: tokio::sync::oneshot::Sender<()>) -> Self {
        Self { tx }
    }

    pub fn stop(self) -> Result<()> {
        match self.tx.send(()) {
            Ok(_) => Ok(()),
            Err(_) => Err(Error::Error("Error stopping server".into())),
        }
    }
}

impl ChatServer {
    pub fn new(name: &str, host: &str, port: &str, logger: &Logger) -> Result<Self> {
        let identity = Identity::new(name);
        let id_string = identity.identifier.to_string();
        Ok(Self {
            identity,
            addr: parse_socket_addr(host, port)?,
            abort_handle: Arc::new(Mutex::new(None)),
            logger: logger.new(o!("server" => id_string)),
        })
    }

    pub async fn get_stop_handle(&self) -> StopHandle {
        // Spawn listener for the kill signal
        let server_addr = self.addr;
        let (tx, rx) = tokio::sync::oneshot::channel();
        let abort_handle = Arc::clone(&self.abort_handle);
        let logger = self.logger.clone();
        tokio::spawn(async move {
            // Wait for the signal
            debug!(
                logger,
                "Chat server {} listening for kill signal", server_addr
            );
            if let Err(e) = rx.await {
                error!(logger, "Error on kill channel: {:?}", e);
            }
            debug!(logger, "Chat server {} got kill signal", server_addr);

            // Abort the acceptor
            let mut abort_handle = abort_handle.lock().await;
            if abort_handle.is_some() {
                debug!(logger, "Chat server {} aborting listener", server_addr);
                abort_handle.take().unwrap().abort();
            }
        });

        StopHandle::new(tx)
    }

    pub async fn start(self) -> Result<()> {
        debug!(self.logger, "Starting chat server on {}", self.addr);
        let mut listener = TcpListener::bind(&self.addr).await?;
        let mut bus = IOBus::new(&self.logger);
        let server_addr = self.addr;

        loop {
            // Set up TCP listener and abort handle
            let (acceptor, abort_handle) = abortable(listener.accept());
            {
                let mut handle = self.abort_handle.lock().await;
                *handle = Some(abort_handle);
            }

            // Listen for connections
            if let Ok(result) = acceptor.await {
                let (mut stream, addr) = result?;
                debug!(
                    self.logger,
                    "Chat server {} got connection from {}", self.addr, addr
                );
                let (receiver, sender) = bus.get_channel().await;
                let identity = self.identity.clone();
                let logger = self.logger.clone();

                // Handle the connection to the peer
                tokio::spawn(async move {
                    match client_handshake(&mut stream, &identity, &logger).await {
                        Err(error) => {
                            error!(logger, "Error in handshake with client {}: {}", addr, error)
                        }
                        Ok((_client_id, _nonce)) => {
                            handle_network_connection(stream, receiver, sender, &logger).await
                        }
                    };
                    debug!(
                        logger,
                        "Chat server {} connection to {} exiting", server_addr, addr
                    );
                });
            } else {
                // If we got here, the listener future was aborted
                break;
            }
        }

        bus.shutdown().await;

        Ok(())
    }
}
