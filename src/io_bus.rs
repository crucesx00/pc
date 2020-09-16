use crate::identity::Identifier;
use bytes::Bytes;
use slog::*;
use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::stream::Stream;
use tokio::sync::{mpsc, Mutex};

#[derive(Debug)]
pub struct IOBusReceiver {
    pub id: Identifier,
    rx: mpsc::UnboundedReceiver<(Identifier, Bytes)>,
    logger: Logger,
}

impl IOBusReceiver {
    fn new(
        id: Identifier,
        rx: mpsc::UnboundedReceiver<(Identifier, Bytes)>,
        logger: &Logger,
    ) -> Self {
        Self {
            id: id.clone(),
            rx,
            logger: logger.new(o!("IOBusReceiver" => id.to_string())),
        }
    }
}

impl Stream for IOBusReceiver {
    type Item = Bytes;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Bytes>> {
        match self.rx.poll_recv(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some((id, message))) => {
                debug!(
                    self.logger,
                    "IOBusReceiver poll_next got {:?} from {}", message, id
                );
                if id != self.id {
                    Poll::Ready(Some(message))
                } else {
                    debug!(
                        self.logger,
                        "IOBusReceiver dropping message from self: {:?}", message
                    );
                    Poll::Pending
                }
            }
            Poll::Ready(None) => Poll::Ready(None),
        }
    }
}

impl fmt::Display for IOBusReceiver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "IOBusReceiver({})", self.id.to_string())
    }
}

#[derive(Debug)]
pub struct IOBusClient {
    pub id: Identifier,
    bus: IOBus,
    logger: Logger,
}

impl IOBusClient {
    pub fn new(id: Identifier, bus: &IOBus, logger: &Logger) -> Self {
        Self {
            id: id.clone(),
            bus: bus.clone(),
            logger: logger.new(o!("IOBusClient" => id.to_string())),
        }
    }

    pub async fn broadcast(&self, msg: Bytes) {
        self.bus.broadcast(&self.id, msg).await;
    }

    pub async fn shutdown(&mut self) {
        self.bus.shutdown().await;
    }
}

impl fmt::Display for IOBusClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "IOBusClient({})", self.id.to_string())
    }
}

#[derive(Clone, Debug)]
pub struct IOBus {
    senders: Arc<Mutex<Vec<mpsc::UnboundedSender<(Identifier, Bytes)>>>>,
    logger: Logger,
}

impl IOBus {
    pub fn new(logger: &Logger) -> Self {
        Self {
            senders: Arc::new(Mutex::new(vec![])),
            logger: logger.clone(),
        }
    }

    pub async fn get_channel(&mut self) -> (IOBusReceiver, IOBusClient) {
        let identifier = Identifier::new();
        let mut senders = self.senders.lock().await;
        let (tx, rx) = mpsc::unbounded_channel();
        let receiver = IOBusReceiver::new(identifier.clone(), rx, &self.logger);
        let client = IOBusClient::new(identifier, self, &self.logger);
        senders.push(tx);

        (receiver, client)
    }

    async fn broadcast(&self, id: &Identifier, msg: Bytes) {
        debug!(self.logger, "IO Bus broadcast {:?}", msg);
        for sender in self.senders.lock().await.iter() {
            debug!(self.logger, "IO Bus broadcast sending {:?} to sender", msg);
            if let Err(error) = sender.send((id.clone(), msg.clone())) {
                error!(self.logger, "Error sending to bus: {}", error);
            }
        }
    }

    pub async fn shutdown(&mut self) {
        for sender in self.senders.lock().await.drain(..) {
            drop(sender);
        }
    }
}
