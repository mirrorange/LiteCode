use std::{net::SocketAddr, path::PathBuf};

use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Transport {
    Stdio,
    Http,
}

#[derive(Debug, Parser)]
#[command(
    name = "litecode",
    about = "LiteCode MCP server",
    version,
    disable_help_subcommand = true
)]
pub struct Cli {
    #[arg(long, value_enum, default_value_t = Transport::Stdio)]
    pub transport: Transport,

    #[arg(long, default_value = "127.0.0.1:3000")]
    pub bind: SocketAddr,

    #[arg(long)]
    pub cwd: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Transport};

    #[test]
    fn parses_http_mode_and_bind() {
        let cli = Cli::parse_from([
            "litecode",
            "--transport",
            "http",
            "--bind",
            "127.0.0.1:7777",
        ]);

        assert_eq!(cli.transport, Transport::Http);
        assert_eq!(cli.bind.to_string(), "127.0.0.1:7777");
    }
}
