pub mod cli;
pub mod error;
pub mod server;
pub mod services;
pub mod tools;
pub mod transport;

use std::path::PathBuf;

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
    let _ = tracing_subscriber::fmt()
        .with_target(false)
        .without_time()
        .try_init();
}

pub fn normalize_path(path: PathBuf) -> Result<PathBuf> {
    Ok(path.canonicalize().unwrap_or(path))
}
