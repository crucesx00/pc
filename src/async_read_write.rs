use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite};

pub struct AsyncReadWrite<In: AsyncRead, Out: AsyncWrite> {
    input: In,
    output: Out,
}

impl<In: AsyncRead, Out: AsyncWrite> AsyncReadWrite<In, Out> {
    pub fn new(input: In, output: Out) -> Self {
        Self { input, output }
    }

    pub fn get_input(self: Pin<&mut Self>) -> Pin<&mut In> {
        unsafe { self.map_unchecked_mut(|s| &mut s.input) }
    }

    pub fn get_output(self: Pin<&mut Self>) -> Pin<&mut Out> {
        unsafe { self.map_unchecked_mut(|s| &mut s.output) }
    }
}

impl<In: AsyncRead, Out: AsyncWrite> AsyncRead for AsyncReadWrite<In, Out> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        self.get_input().poll_read(cx, buf)
    }
}

impl<In: AsyncRead, Out: AsyncWrite> AsyncWrite for AsyncReadWrite<In, Out> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.get_output().poll_write(cx, buf)
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut Context,
    ) -> Poll<std::result::Result<(), std::io::Error>> {
        self.get_output().poll_flush(cx)
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut Context,
    ) -> Poll<std::result::Result<(), std::io::Error>> {
        self.get_output().poll_shutdown(cx)
    }
}

#[cfg(test)]
pub mod tests {
    use crate::error::Result;
    use rand::{
        self,
        distributions::{Distribution, Standard},
        thread_rng, Rng,
    };
    use slog::*;
    use std::pin::Pin;
    use std::sync::Once;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, BufWriter};
    use tokio::sync::mpsc::{
        unbounded_channel, UnboundedReceiver, UnboundedSender,
    };

    pub struct MockAsyncReadWrite {
        input: UnboundedReceiver<Vec<u8>>,
        output: UnboundedSender<Vec<u8>>,
        input_buffer: Vec<u8>,
        logger: Logger,
    }

    impl MockAsyncReadWrite {
        pub fn pair(logger: &Logger) -> (Self, Self) {
            let (output_a, input_b) = unbounded_channel();
            let (output_b, input_a) = unbounded_channel();
            (
                Self {
                    input: input_a,
                    output: output_a,
                    input_buffer: vec![],
                    logger: logger.clone(),
                },
                Self {
                    input: input_b,
                    output: output_b,
                    input_buffer: vec![],
                    logger: logger.clone(),
                },
            )
        }

        fn read_data(&mut self, buf: &mut [u8]) -> Poll<std::io::Result<usize>> {
            if self.input_buffer.is_empty() {
                Poll::Pending
            } else {
                let len = std::cmp::min(self.input_buffer.len(), buf.len());
                debug!(self.logger, "read_data, len = {}", len);
                debug!(self.logger, "read_data, Before, buf = {:?}", buf);
                self.input_buffer
                    .drain(..len)
                    .enumerate()
                    .for_each(|(i, val)| buf[i] = val);
                debug!(self.logger, "read_data, After, buf = {:?}", buf);
                Poll::Ready(Ok(len))
            }
        }
    }

    impl AsyncRead for MockAsyncReadWrite {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
            buf: &mut [u8],
        ) -> Poll<std::io::Result<usize>> {
            debug!(self.logger, "poll_read, attempting read");
            let ret = match self.input.poll_recv(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(None) => Poll::Ready(Ok(0)),
                Poll::Ready(Some(mut data)) => {
                    debug!(self.logger, "poll_read, Reading {} bytes", buf.len());
                    debug!(
                        self.logger,
                        "poll_read, Input buffer = {:?}", self.input_buffer
                    );
                    debug!(self.logger, "poll_read, Data read = {:?}", data);
                    self.input_buffer.append(&mut data);
                    self.read_data(buf)
                }
            };
            debug!(self.logger, "poll_read, Returning {:?}", ret);
            ret
        }
    }

    impl AsyncWrite for MockAsyncReadWrite {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context,
            buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            debug!(
                self.logger,
                "poll_write, Writing {} bytes: {:?}",
                buf.len(),
                buf
            );
            match self.output.send(buf.into()) {
                Ok(_) => Poll::Ready(Ok(buf.len())),
                Err(error) => {
                    Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, error)))
                }
            }
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _cx: &mut Context,
        ) -> Poll<std::result::Result<(), std::io::Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(
            self: Pin<&mut Self>,
            _cx: &mut Context,
        ) -> Poll<std::result::Result<(), std::io::Error>> {
            Poll::Ready(Ok(()))
        }
    }

    static INIT: Once = Once::new();

    fn setup_logging() -> Logger {
        let decorator = slog_term::TermDecorator::new().build();
        let drain = slog_term::FullFormat::new(decorator).build().fuse();
        let drain = slog_async::Async::new(drain).build().fuse();
        let drain = LevelFilter(drain, Level::Warning).fuse();

        slog::Logger::root(drain, o!())
    }

    fn generate_random_data() -> Vec<u8> {
        let rng = thread_rng();
        rng.sample_iter::<u8, Standard>(Standard)
            .take(2048)
            .collect()
    }

    #[tokio::test]
    async fn test_mock_async_read_write() -> Result<()> {
        let logger = setup_logging();
        {
            let (client, server) = MockAsyncReadWrite::pair(&logger);
            let data = generate_random_data();
            let mut client_writer = BufWriter::new(client);
            let mut server_reader = BufReader::new(server);
            client_writer.write(&data).await?;
            client_writer.flush().await?;
            let mut output_buffer = [0; 2048];
            let size = server_reader.read(&mut output_buffer).await?;
            assert_eq!(2048, size);
            assert_eq!(data, &output_buffer[..]);
        }
        {
            let (client, server) = MockAsyncReadWrite::pair(&logger);
            let data = generate_random_data();
            let mut server_writer = BufWriter::new(server);
            let mut client_reader = BufReader::new(client);
            server_writer.write(&data).await?;
            server_writer.flush().await?;
            let mut output_buffer = [0; 2048];
            let size = client_reader.read(&mut output_buffer).await?;
            assert_eq!(2048, size);
            assert_eq!(data, &output_buffer[..]);
        }

        Ok(())
    }
}
