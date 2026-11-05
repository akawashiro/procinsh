//! Validates the inherited private endpoint and owns helper lifetime.
//!
//! # Interface
//! [`run`] (`pub(crate)`): `fn run() -> anyhow::Result<()>`.
use super::helper_service::{CollectorManager, HelperIpcService};
use crate::ipc::{schema::*, transport};
use anyhow::{Result, ensure};
use clap::Parser;
use std::{
    net::Shutdown,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::net::UnixStream,
    },
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

#[derive(Parser)]
struct Cli {
    /// Inherited private IPC endpoint (must be file descriptor 3)
    #[arg(long, value_parser = clap::value_parser!(i32).range(3..=3))]
    ipc_fd: i32,
    /// PID of the parent process that owns the IPC endpoint
    #[arg(long)]
    parent_pid: i32,
    /// Refuse to read process environments and memory
    #[arg(long)]
    privacy: bool,
}

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn stop(_: i32) {
    STOP.store(true, Ordering::Relaxed);
}

pub(crate) fn run() -> Result<()> {
    let cli = Cli::parse();
    let parent = cli.parent_pid;
    if cli.privacy {
        super::process::enable_privacy();
    }
    let mut peer: libc::ucred = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of_val(&peer) as libc::socklen_t;
    ensure!(
        unsafe {
            libc::getsockopt(
                cli.ipc_fd,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut peer as *mut _ as *mut _,
                &mut size,
            )
        } == 0
            && peer.pid == parent
            && parent == unsafe { libc::getppid() }
            && peer.uid == unsafe { libc::getuid() },
        "invalid IPC parent identity"
    );
    let mut ipc_socket = unsafe { UnixStream::from_raw_fd(cli.ipc_fd) };
    ensure!(
        unsafe { libc::fcntl(cli.ipc_fd, libc::F_SETFD, libc::FD_CLOEXEC) } == 0,
        "seal inherited endpoint"
    );
    super::privileges::restrict_helper_privileges()?;
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = stop as *const () as libc::sighandler_t;
    unsafe {
        libc::sigemptyset(&mut action.sa_mask);
    }
    for signal in [libc::SIGTERM, libc::SIGINT] {
        ensure!(
            unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) } == 0,
            "install shutdown signal handler"
        );
    }
    ensure!(
        unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM, 0, 0, 0) } == 0
            && unsafe { libc::getppid() } == parent,
        "parent exited during helper startup"
    );
    transport::send(
        &mut ipc_socket,
        &HelperIdentity {
            pid: std::process::id(),
            capabilities: super::privileges::effective_capabilities()?,
        },
        MAX_REQUEST,
    )?;
    let collectors = Arc::new(CollectorManager::new(Duration::from_secs(1)));
    let ipc_service = HelperIpcService::new(collectors.clone());
    let shutdown = ipc_socket.try_clone()?;
    let (finished, completion) = mpsc::channel();
    std::thread::Builder::new()
        .name("privileged-ipc".into())
        .spawn(move || {
            let _ = finished.send(ipc_service.serve(ipc_socket));
        })?;
    loop {
        match completion.try_recv() {
            Ok(result) => {
                if let Err(error) = result {
                    log::debug!("helper IPC ended: {error:#}");
                }
                break;
            }
            Err(mpsc::TryRecvError::Disconnected) => anyhow::bail!("helper IPC worker panicked"),
            Err(mpsc::TryRecvError::Empty) => {}
        }
        let mut fd = libc::pollfd {
            fd: shutdown.as_raw_fd(),
            events: libc::POLLRDHUP,
            revents: 0,
        };
        unsafe {
            libc::poll(&mut fd, 1, 100);
        }
        if STOP.load(Ordering::Relaxed)
            || fd.revents & (libc::POLLRDHUP | libc::POLLHUP | libc::POLLERR) != 0
        {
            break;
        }
    }
    collectors.stop();
    let _ = shutdown.shutdown(Shutdown::Both);
    let (done, joined) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(collectors.join_collectors());
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    joined.recv_timeout(deadline.saturating_duration_since(Instant::now()))??;
    Ok(())
}
