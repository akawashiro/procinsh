#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
compile_error!("procinsh supports Linux x86-64 only");

use anyhow::{Context, Result, ensure};
use clap::Parser;
use std::{net::SocketAddr, sync::Arc, time::Duration};

#[derive(Parser)]
#[command(version, about = "Read-only Linux x86-64 process inspector")]
struct Cli {
    #[arg(long, value_parser = clap::value_parser!(i32).range(1..))]
    pid: Option<i32>,
    #[arg(long, default_value = "1s", value_parser = humantime::parse_duration)]
    interval: Duration,
    #[arg(long, default_value = "127.0.0.1:8080")]
    listen: SocketAddr,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    ensure!(
        cli.interval >= Duration::from_millis(100) && cli.interval <= Duration::from_secs(60),
        "--interval must be between 100ms and 60s"
    );
    let state = Arc::new(procinsh::state::AppState::new(cli.interval));
    if let Some(pid) = cli.pid {
        state.select(procinsh::process::identity(pid)?)?;
    }
    let listener = tokio::net::TcpListener::bind(cli.listen)
        .await
        .context("could not bind HTTP listener")?;
    let address = listener.local_addr()?;
    if !address.ip().is_loopback() {
        eprintln!(
            "Warning: remote access exposes process memory. Use only on a trusted network; no authentication is provided."
        );
    }
    let collector = state.start_collector();
    let shutdown_state = state.clone();
    println!("procinsh is running:\nhttp://{address}");
    let result = axum::serve(
        listener,
        procinsh::server::router(state.clone(), address),
    )
    .with_graceful_shutdown(async move {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("SIGTERM handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
        shutdown_state.stop();
    })
    .await;
    state.stop();
    collector.join().ok();
    result.context("HTTP server failed")
}
