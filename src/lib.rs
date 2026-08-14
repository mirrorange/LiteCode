pub mod cli;
pub mod error;
pub mod schema;
pub mod server;
pub mod services;
pub mod tools;
pub mod transport;

use crate::{cli::Transport, error::Result, server::LiteCodeServer};

pub async fn run(cli: cli::Cli) -> Result<()> {
    init_tracing();

    let working_dir = match cli.cwd {
        Some(path) => path,
        None => std::env::current_dir()?,
    };

    let server = LiteCodeServer::new(working_dir);

    match cli.transport {
        Transport::Stdio => transport::stdio::serve(server).await,
        Transport::Http => transport::http::serve(server, cli.bind).await,
    }
}

fn init_tracing() {
    // The stdio transport carries JSON-RPC over stdout, so logs must never be
    // written there: a single log line corrupts the message stream and the
    // connected client fails to parse the responses that follow it.
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_target(false)
        .without_time()
        .try_init();
}
