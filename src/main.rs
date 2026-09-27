#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
compile_error!("procinsh supports Linux x86-64 only");

use anyhow::{Context, Result, ensure};
use clap::Parser;
use std::{io::Write, net::SocketAddr, sync::Arc, time::Duration};

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
    let state = Arc::new(procinsh::state::AppState::new(cli.interval));
    let listener = tokio::net::TcpListener::bind(cli.listen)
        .await
        .context("could not bind HTTP listener")?;
    let address = listener.local_addr()?;
    if !address.ip().is_loopback() {
        log::warn!(
            "Warning: remote access exposes process memory and environment variables without authentication or TLS. Use only on a trusted network."
        );
    }
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .context("could not install SIGTERM handler")?;
    let shutdown_state = state.clone();
    log::info!(
        "procinsh {} listening on http://{address} interval={:?}",
        env!("CARGO_PKG_VERSION"),
        cli.interval
    );
    let result = axum::serve(listener, procinsh::server::router(state.clone(), address))
        .with_graceful_shutdown(async move {
            tokio::select! {
                result = tokio::signal::ctrl_c() => {
                    match result {
                        Ok(()) => log::info!("received SIGINT; shutting down"),
                        Err(error) => log::error!("SIGINT handler failed: {error}"),
                    }
                },
                _ = terminate.recv() => log::info!("received SIGTERM; shutting down"),
            }
            shutdown_state.stop();
        })
        .await;
    state.stop();
    state.join_collectors()?;
    log::info!("procinsh stopped");
    result.context("HTTP server failed")
}
