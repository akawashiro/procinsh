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
use std::{io::Write, net::SocketAddr};

#[derive(Parser)]
#[command(version, about = "Read-only Linux x86-64 process inspector")]
struct Cli {
    /// Listen address (repeat for multiple listeners); non-loopback addresses require --allow-non-loopback
    #[arg(long, default_value = "127.0.0.1:8080")]
    listen: Vec<SocketAddr>,
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
    for address in &cli.listen {
        ensure!(
            address.ip().is_loopback() || cli.allow_non_loopback,
            "refusing to listen on non-loopback address {}; use --allow-non-loopback to explicitly expose process memory and environment variables without authentication or TLS",
            address
        );
    }
    http_server::run(&cli.listen).await
}
