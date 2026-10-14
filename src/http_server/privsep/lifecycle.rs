use super::{
    client::MonitorClient,
    privileges,
    service::{MonitorService, MonitorState},
};
use anyhow::{Context, Result, ensure};
use std::{
    net::{Shutdown, SocketAddr},
    os::{fd::AsRawFd, unix::net::UnixStream},
    sync::{
        Arc,
        atomic::{AtomicI32, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

static SIGNAL: AtomicI32 = AtomicI32::new(0);
extern "C" fn handle_signal(signal: i32) {
    SIGNAL.store(signal, Ordering::Relaxed);
}

fn signal_handlers(handler: libc::sighandler_t) -> Result<()> {
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = handler;
    unsafe { libc::sigemptyset(&mut action.sa_mask) };
    for signal in [libc::SIGTERM, libc::SIGINT] {
        ensure!(
            unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) } == 0,
            "could not install lifecycle signal handler: {}",
            std::io::Error::last_os_error()
        );
    }
    Ok(())
}

/// Fork before any runtime, workers, monitors, or target descriptors exist.
/// Each process retains exactly one socket endpoint, with close-on-exec set by
/// UnixStream::pair. The parent never binds an HTTP listener.
pub(crate) fn run(listen: &[SocketAddr]) -> Result<()> {
    let (uid, gid) = privileges::application_identity()?;
    privileges::close_inherited()?;
    signal_handlers(handle_signal as *const () as libc::sighandler_t)?;
    let (parent, child) = UnixStream::pair()?;
    let parent_pid = unsafe { libc::getpid() };
    let pid = unsafe { libc::fork() };
    ensure!(pid >= 0, "fork failed: {}", std::io::Error::last_os_error());
    if pid == 0 {
        drop(parent);
        signal_handlers(libc::SIG_DFL)?;
        // Credentials can clear PDEATHSIG, so install it after dropping them.
        privileges::drop_application_privileges(uid, gid)?;
        ensure!(
            unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM, 0, 0, 0) } == 0,
            "set worker parent-death signal failed"
        );
        ensure!(
            unsafe { libc::getppid() } == parent_pid,
            "monitor exited during worker startup"
        );
        let client = MonitorClient::new(child)?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let result = runtime.block_on(crate::http_server::server::run(listen, client));
        runtime.shutdown_timeout(Duration::from_secs(1));
        return result;
    }
    drop(child);
    let mut reaped = false;
    let result = monitor(parent, pid, uid, &mut reaped);
    if result.is_err() && !reaped {
        unsafe {
            libc::kill(pid, libc::SIGKILL);
            while libc::waitpid(pid, std::ptr::null_mut(), 0) < 0 {
                if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                    break;
                }
            }
        }
    }
    result
}

fn monitor(socket: UnixStream, pid: i32, uid: u32, reaped: &mut bool) -> Result<()> {
    privileges::restrict_monitor_privileges(uid)?;
    log::info!(
        "privileged monitor pid={} HTTP worker pid={pid}",
        std::process::id()
    );
    let state = Arc::new(MonitorState::new(Duration::from_secs(1)));
    let service = MonitorService::new(state.clone());
    let shutdown = socket.try_clone()?;
    let (finished, completion) = mpsc::channel();
    let worker = std::thread::Builder::new()
        .name("privileged-monitor".into())
        .spawn(move || {
            let result = service.serve(socket);
            let _ = finished.send(result);
        })?;
    let mut stopping = None;
    let mut service_result = None;
    let mut exit: libc::siginfo_t = unsafe { std::mem::zeroed() };
    loop {
        // WEXITED never consumes ptrace stops when the worker itself is being
        // inspected. Keep its PID reserved until an actual exit is reaped.
        let waited = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as u32,
                &mut exit,
                libc::WEXITED | libc::WNOHANG,
            )
        };
        if waited == 0 && unsafe { exit.si_pid() } == pid {
            *reaped = true;
            break;
        }
        if waited < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(error).context("wait for HTTP worker");
            }
        }
        let signal = SIGNAL.swap(0, Ordering::Relaxed);
        if signal != 0 {
            unsafe {
                libc::kill(pid, signal);
            }
            state.stop();
            stopping.get_or_insert_with(Instant::now);
        }
        // Detect a disconnected child independently of acquisition, which may
        // currently be blocked in a kernel read or ptrace wait.
        let mut fd = libc::pollfd {
            fd: shutdown.as_raw_fd(),
            events: libc::POLLRDHUP,
            revents: 0,
        };
        if stopping.is_none()
            && unsafe { libc::poll(&mut fd, 1, 0) } > 0
            && fd.revents & (libc::POLLRDHUP | libc::POLLHUP | libc::POLLERR) != 0
        {
            state.stop();
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
            stopping = Some(Instant::now());
        }
        if service_result.is_none() {
            match completion.try_recv() {
                Ok(result) => {
                    service_result = Some(result);
                    state.stop();
                    unsafe {
                        libc::kill(pid, libc::SIGTERM);
                    }
                    stopping.get_or_insert_with(Instant::now);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    anyhow::bail!("privileged monitor worker panicked")
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if stopping.is_some_and(|at| at.elapsed() > Duration::from_secs(10)) {
            anyhow::bail!("HTTP worker shutdown timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    state.stop();
    let _ = shutdown.shutdown(Shutdown::Both);
    // The service drops every session on disconnect before collector joins.
    let service_result = match service_result {
        Some(result) => result,
        None => completion
            .recv_timeout(Duration::from_secs(5))
            .context("monitor IPC worker shutdown timed out")?,
    };
    worker
        .join()
        .map_err(|_| anyhow::anyhow!("privileged monitor worker panicked"))?;
    let (joined, joined_result) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = joined.send(state.join_collectors());
    });
    joined_result
        .recv_timeout(Duration::from_secs(5))
        .context("monitor collector shutdown timed out")??;
    if let Err(error) = service_result {
        let disconnected = error.downcast_ref::<std::io::Error>().is_some_and(|e| {
            matches!(
                e.kind(),
                std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::NotConnected
            )
        });
        if !disconnected {
            return Err(error).context("privileged monitor IPC failed");
        }
    }
    ensure!(
        exit.si_code == libc::CLD_EXITED && unsafe { exit.si_status() } == 0,
        "HTTP worker exited abnormally (code {}, status {})",
        exit.si_code,
        unsafe { exit.si_status() }
    );
    Ok(())
}
