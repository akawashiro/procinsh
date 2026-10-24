//! Validates the inherited private endpoint and owns helper lifetime.
//!
//! # Interface
//! [`run`] (`pub(crate)`): `fn run() -> anyhow::Result<()>`.
use super::helper_service::{MonitorService, MonitorState};
use anyhow::{Result, ensure};
use procinsh_protocol::ipc::{schema::*, transport};
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

static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stop(_: i32) {
    STOP.store(true, Ordering::Relaxed);
}

pub(crate) fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 4 && args[0] == "--ipc-fd" && args[1] == "3" && args[2] == "--parent-pid",
        "helper requires an inherited private IPC endpoint"
    );
    let parent: i32 = args[3].parse()?;
    let mut peer: libc::ucred = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of_val(&peer) as libc::socklen_t;
    ensure!(
        unsafe {
            libc::getsockopt(
                3,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut peer as *mut _ as *mut _,
                &mut size,
            )
        } == 0
            && peer.uid != 0
            && unsafe { libc::geteuid() } != 0
            && peer.pid == parent
            && parent == unsafe { libc::getppid() }
            && peer.uid == unsafe { libc::getuid() },
        "invalid IPC parent identity"
    );
    let mut socket = unsafe { UnixStream::from_raw_fd(3) };
    ensure!(
        unsafe { libc::fcntl(3, libc::F_SETFD, libc::FD_CLOEXEC) } == 0,
        "seal inherited endpoint"
    );
    super::privileges::restrict_monitor_privileges(peer.uid)?;
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
    transport::write(
        &mut socket,
        &HelperIdentity {
            pid: std::process::id(),
            version: env!("CARGO_PKG_VERSION").into(),
            capabilities: super::privileges::effective_capabilities()?,
        },
        MAX_REQUEST,
    )?;
    let state = Arc::new(MonitorState::new(Duration::from_secs(1)));
    let service = MonitorService::new(state.clone());
    let shutdown = socket.try_clone()?;
    let (finished, completion) = mpsc::channel();
    std::thread::Builder::new()
        .name("privileged-ipc".into())
        .spawn(move || {
            let _ = finished.send(service.serve(socket));
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
    state.stop();
    let _ = shutdown.shutdown(Shutdown::Both);
    let (done, joined) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(state.join_collectors());
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    joined.recv_timeout(deadline.saturating_duration_since(Instant::now()))??;
    Ok(())
}
