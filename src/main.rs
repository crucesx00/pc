use clap::{clap_app, crate_version};
use futures::executor::block_on;
use shrust::{ExecResult, Shell, ShellIO};
use slog::*;
use slog_async;
use std::fs;
use std::path::PathBuf;
use tokio::spawn;
use privy::chat::client::ChatClient;
use privy::chat::server::{ChatServer, StopHandle};
use privy::error::{Error, Result};
use privy::identity::IdentityFile;

#[tokio::main]
async fn main() -> Result<()> {
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let drain = LevelFilter(drain, Level::Warning).fuse();

    let logger = slog::Logger::root(drain, o!());

    clap_app!(privy =>
        (version: crate_version!())
        (author: "PRIVY - SOMTA  <cwo@tuta.io>")
        (about: "ircd.chat/6697 -- #PRiVY")
    )
    .get_matches();

    if sodiumoxide::init().is_err() {
        eprintln!("Error initializing the sodiumoxide library");
        std::process::exit(1);
    }

    let home_dir_opt = dirs::home_dir();
    if home_dir_opt.is_none() {
        eprintln!("Error getting user's home directory");
        std::process::exit(1);
    }
    let mut privy_dir = home_dir_opt.unwrap();
    privy_dir.push(".privy");
    fs::create_dir_all(&privy_dir)?;

    let identity_file = open_identity_file(&privy_dir).await?;
    let context = Context {
        identity_file,
        server_handle: None,
        server_stop_handle: None,
        logger,
    };

    let mut shell = Shell::new(context);
    shell.new_command(
        "create-identity",
        "Create a new identity",
        1,
        create_identity,
    );
    shell.new_command("export-identity", "Export an identity", 1, export_identity);
    shell.new_command_noargs("list-identities", "List your identities", list_identities);
    shell.new_command("add-trusted", "Add a trusted identity", 1, add_trusted);
    shell.new_command_noargs("list-trusted", "List your trusted friends", list_trusted);
    shell.new_command(
        "start-server",
        "start server on host and port",
        3,
        start_server,
    );
    shell.new_command_noargs("kill-server", "stop server", kill_server);
    shell.new_command("connect", "connect to host and port", 3, connect);

    shell.run_loop(&mut ShellIO::default());

    Ok(())
}

struct Context {
    identity_file: IdentityFile<fs::File>,
    server_handle: Option<tokio::task::JoinHandle<Result<()>>>,
    server_stop_handle: Option<StopHandle>,
    logger: Logger,
}

async fn open_identity_file(privy_dir: &PathBuf) -> Result<IdentityFile<fs::File>> {
    let password = rpassword::prompt_password_stdout("Identity file password:")?;
    let mut identity_file_path = privy_dir.clone();
    identity_file_path.push("identity");
    match IdentityFile::open(&identity_file_path, password).await {
        Ok(identity_file) => Ok(identity_file),
        Err(error) => Err(Error::Error(format!(
            "Error opening identity file: {}",
            error
        ))),
    }
}

fn create_identity(_io: &mut ShellIO, context: &mut Context, args: &[&str]) -> ExecResult {
    let name = args[0];
    block_on(context.identity_file.add_identity(name))?;
    Ok(())
}

fn export_identity(_io: &mut ShellIO, context: &mut Context, args: &[&str]) -> ExecResult {
    let name = args[0];
    let exported = context.identity_file.export_public_identity(name)?;
    match exported {
        Some(exported) => println!("{}", exported),
        None => eprintln!("No identity named {} found", name),
    };
    Ok(())
}

fn list_identities(_io: &mut ShellIO, context: &mut Context) -> ExecResult {
    println!("{:?}", context.identity_file.list_identities());
    Ok(())
}

fn add_trusted(_io: &mut ShellIO, context: &mut Context, args: &[&str]) -> ExecResult {
    let encoded = args[0];
    block_on(context.identity_file.add_trusted(encoded))?;

    Ok(())
}

fn list_trusted(_io: &mut ShellIO, context: &mut Context) -> ExecResult {
    println!("{:?}", block_on(context.identity_file.list_trusted()));
    Ok(())
}

fn start_server(_io: &mut ShellIO, context: &mut Context, args: &[&str]) -> ExecResult {
    let name = args[0];
    let host = args[1];
    let port = args[2];

    let chat_server = ChatServer::new(name, host, port, &context.logger)?;
    context.server_stop_handle = Some(block_on(chat_server.get_stop_handle()));
    context.server_handle = Some(spawn(chat_server.start()));

    Ok(())
}

fn kill_server(_io: &mut ShellIO, context: &mut Context) -> ExecResult {
    if context.server_stop_handle.is_none() {
        eprintln!("Server not running");
    }

    let stop_handle = context.server_stop_handle.take().unwrap();
    if let Err(e) = stop_handle.stop() {
        eprintln!("Error signaling server to stop: {:?}", e);
    }

    let join_handle = context.server_handle.take().unwrap();
    block_on(async {
        if let Err(e) = join_handle.await {
            eprintln!("Error joining server: {:?}", e);
        }
    });

    context.server_stop_handle = None;
    context.server_handle = None;

    Ok(())
}

fn connect(_io: &mut ShellIO, context: &mut Context, args: &[&str]) -> ExecResult {
    let host = args[0];
    let port = args[1];
    let identity = args[2];

    let identity_opt = context
        .identity_file
        .identities
        .iter()
        .find(|id| id.name == identity);
    if identity_opt.is_none() {
        return Err(Error::Error(format!("Identity {} not found", identity).into()).into());
    }

    let chat_client = block_on(ChatClient::new(
        host,
        port,
        identity_opt.unwrap().clone(),
        tokio::io::stdin(),
        tokio::io::stdout(),
        &context.logger,
    ))?;
    block_on(chat_client.start())?;

    Ok(())
}
