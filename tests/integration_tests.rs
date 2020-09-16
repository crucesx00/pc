#we need to thinking about aa good integration tester and fast audit for every #release
use rand::Rng;
use slog::*;
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::Once;
use std::time::Duration;
use tokio::spawn;
use tokio::sync::Mutex;
use trithemius::chat::client::ChatClient;
use trithemius::chat::server::ChatServer;
use trithemius::error::Result;
use trithemius::identity::Identity;
use trithemius::io::Builder;

fn setup_logging() -> Logger {
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let drain = LevelFilter(drain, Level::Warning).fuse();

    slog::Logger::root(drain, o!())
}

fn get_available_port() -> Option<u16> {
    let mut rng = rand::thread_rng();
    loop {
        let port = rng.gen_range(8000, 9000);
        if port_is_available(port) {
            return Some(port);
        }
    }
}

fn port_is_available(port: u16) -> bool {
    match TcpListener::bind(("127.0.0.1", port)) {
        Ok(_) => true,
        Err(_) => false,
    }
}

#[tokio::test(threaded_scheduler)]
async fn test_start_kill_server() -> Result<()> {
    let logger = setup_logging();
    let port = get_available_port()
        .expect("get_available_port")
        .to_string();
    let server = ChatServer::new("server", "localhost", &port, &logger)?;
    let stop_handle = server.get_stop_handle().await;
    let handle = spawn(server.start());
    std::thread::sleep(std::time::Duration::from_millis(500));
    stop_handle.stop()?;
    handle.await.unwrap()?;

    Ok(())
}

#[tokio::test(threaded_scheduler)]
async fn test_client_fails_when_cannot_connect() {
    let logger = setup_logging();
    let port = get_available_port()
        .expect("get_available_port")
        .to_string();
    assert!(ChatClient::new(
        "localhost",
        &port,
        Identity::new("foo"),
        tokio::io::stdin(),
        tokio::io::stdout(),
        &logger,
    )
    .await
    .is_err());
}

#[tokio::test(threaded_scheduler)]
async fn test_client_connect_to_server() -> Result<()> {
    let logger = setup_logging();
    let port = get_available_port()
        .expect("get_available_port")
        .to_string();
    let server = ChatServer::new("server", "localhost", &port, &logger)?;
    let stop_handle = server.get_stop_handle().await;
    let server_handle = spawn(server.start());
    std::thread::sleep(std::time::Duration::from_millis(500));
    let client_in = Builder::new(&logger).wait(Duration::new(1, 0)).build();
    let client = ChatClient::new(
        "localhost",
        &port,
        Identity::new("foo"),
        client_in,
        tokio::io::stdout(),
        &logger,
    )
    .await?;
    let client_handle = spawn(client.start());
    std::thread::sleep(std::time::Duration::from_millis(500));
    stop_handle.stop()?;
    server_handle.await.unwrap()?;
    client_handle.await.unwrap()?;
    Ok(())
}

#[tokio::test(threaded_scheduler)]
async fn test_multiple_clients_connect_to_server() -> Result<()> {
    let logger = setup_logging();
    let port = get_available_port()
        .expect("get_available_port")
        .to_string();
    let server = ChatServer::new("server", "localhost", &port, &logger)?;
    let stop_handle = server.get_stop_handle().await;
    let handle = spawn(server.start());
    std::thread::sleep(std::time::Duration::from_millis(500));
    let client1_in = Builder::new(&logger).wait(Duration::new(1, 0)).build();
    let client2_in = Builder::new(&logger).wait(Duration::new(1, 0)).build();
    let client1 = ChatClient::new(
        "localhost",
        &port,
        Identity::new("foo"),
        client1_in,
        tokio::io::stdout(),
        &logger,
    )
    .await?;
    let client2 = ChatClient::new(
        "localhost",
        &port,
        Identity::new("foo"),
        client2_in,
        tokio::io::stdout(),
        &logger,
    )
    .await?;
    let client1_handle = spawn(client1.start());
    let client2_handle = spawn(client2.start());
    std::thread::sleep(std::time::Duration::from_millis(500));
    stop_handle.stop()?;
    handle.await.unwrap()?;
    client1_handle.await.unwrap()?;
    client2_handle.await.unwrap()?;
    Ok(())
}

#[tokio::test(threaded_scheduler)]
async fn test_clients_communicate() -> Result<()> {
    let logger = setup_logging();
    let port = get_available_port()
        .expect("get_available_port")
        .to_string();
    debug!(logger, "Creating chat server");
    let server = ChatServer::new("server", "localhost", &port, &logger)?;
    let stop_handle = server.get_stop_handle().await;
    debug!(logger, "Starting chat server");
    let handle = spawn(server.start());
    std::thread::sleep(std::time::Duration::from_millis(500));

    let client1_id = Identity::new("foo");
    let client1_identifier = client1_id.identifier.clone();
    let client2_id = Identity::new("foo");
    let client2_identifier = client2_id.identifier.clone();

    let client1_in = Builder::new(&logger)
        .wait(Duration::new(1, 0))
        .read("foo\n".as_bytes())
        .wait(Duration::new(1, 0))
        .build();
    let client1_out = Builder::new(&logger)
        .write(format!("{}: bar\n", client2_identifier).as_bytes())
        .build();
    let client2_in = Builder::new(&logger)
        .wait(Duration::new(1, 0))
        .read("bar\n".as_bytes())
        .wait(Duration::new(1, 0))
        .build();
    let client2_out = Builder::new(&logger)
        .write(format!("{}: foo\n", client1_identifier).as_bytes())
        .build();
    debug!(logger, "Creating client 1, {}", client1_identifier);
    let client1_handle = spawn(
        ChatClient::new(
            "localhost",
            &port,
            client1_id,
            client1_in,
            client1_out,
            &logger,
        )
        .await?
        .start(),
    );
    debug!(logger, "Creating client 2, {}", client2_identifier);
    let client2_handle = spawn(
        ChatClient::new(
            "localhost",
            &port,
            client2_id,
            client2_in,
            client2_out,
            &logger,
        )
        .await?
        .start(),
    );
    std::thread::sleep(std::time::Duration::from_secs(2));
    client1_handle.await.unwrap()?;
    client2_handle.await.unwrap()?;

    stop_handle.stop()?;
    handle.await.expect("awaiting server exit")?;
    Ok(())
}
