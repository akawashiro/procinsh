//! Process inspector for Linux.
//!
//! Start with the [HTTP server interface](http_server). Each subsystem's
//! **Interface** section lists its re-exported types and function signatures,
//! with links to the defining items and their source.

mod http_server;

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
compile_error!("procinsh supports Linux x86-64 only");

use anyhow::{Result, ensure};
use clap::Parser;
use std::{io::Write, net::SocketAddr, time::Duration};

#[derive(Parser)]
#[command(version, about = "Read-only Linux x86-64 process inspector")]
struct Cli {
    #[arg(long, default_value = "1s", value_parser = humantime::parse_duration)]
    interval: Duration,
    /// Listen address; non-loopback addresses require --allow-non-loopback
    #[arg(long, default_value = "127.0.0.1:8080")]
    listen: SocketAddr,
    /// Allow non-loopback listening: exposes process memory and environment variables without authentication or TLS
    #[arg(long)]
    allow_non_loopback: bool,
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::Stderr)
        .format(|buffer, record| {
            writeln!(
                buffer,
                "[{} {:5} {}:{}] {}",
                buffer.timestamp_millis(),
                record.level(),
                record.file().unwrap_or("unknown"),
                record.line().unwrap_or(0),
                record.args()
            )
        })
        .init();
    match run().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            log::error!("{error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    ensure!(
        cli.listen.ip().is_loopback() || cli.allow_non_loopback,
        "refusing to listen on non-loopback address {}; use --allow-non-loopback to explicitly expose process memory and environment variables without authentication or TLS",
        cli.listen
    );
    ensure!(
        cli.interval >= Duration::from_millis(100) && cli.interval <= Duration::from_secs(60),
        "--interval must be between 100ms and 60s"
    );
    http_server::run(cli.listen, cli.interval).await
}
