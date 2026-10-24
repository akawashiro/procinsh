use super::{client::MonitorClient, privileges};
use anyhow::{Context, Result, ensure};
use procinsh_protocol::ipc::{schema::*, transport};
use std::{
    net::{Shutdown, SocketAddr},
    os::{
        fd::AsRawFd,
        unix::{net::UnixStream, process::CommandExt},
    },
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};

fn validate_helper_path(path: &Path) -> Result<()> {
    ensure!(path.is_absolute(), "helper path must be absolute");
    ensure!(
        std::fs::metadata(path)?.is_file(),
        "helper must be a regular file"
    );
    Ok(())
}

struct HelperGuard {
    child: Child,
    socket: UnixStream,
}
impl Drop for HelperGuard {
    fn drop(&mut self) {
        let _ = self.socket.shutdown(Shutdown::Both);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct HelperSupervisor {
    stop: Option<std::sync::mpsc::Sender<()>>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl HelperSupervisor {
    fn new(mut guard: HelperGuard) -> Result<Self> {
        let (stop, stopping) = std::sync::mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("helper-supervisor".into())
            .spawn(move || {
                loop {
                    match guard.child.try_wait() {
                        Ok(Some(status)) => {
                            log::info!("privileged helper exited: {status}");
                            break;
                        }
                        Err(error) => {
                            log::warn!("helper wait failed: {error}");
                            break;
                        }
                        Ok(None) => {}
                    }
                    if stopping.recv_timeout(Duration::from_millis(50))
                        != Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                    {
                        break;
                    }
                }
                // Guard closes IPC and bounds shutdown, including on worker panic.
            })?;
        Ok(Self {
            stop: Some(stop),
            worker: Some(worker),
        })
    }
}
impl Drop for HelperSupervisor {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub(crate) fn run(listen: &[SocketAddr]) -> Result<()> {
    let path = std::env::current_exe()?.with_file_name("procinsh-privileged-helper");
    privileges::close_inherited()?;
    // This runs before Tokio creates any threads. Keep no_new_privs unset until
    // after helper exec, otherwise Linux suppresses its file capabilities.
    privileges::drop_application_capabilities()?;
    let connection = (|| -> Result<(UnixStream, HelperGuard)> {
        validate_helper_path(&path)?;
        let (mut application, helper) = UnixStream::pair()?;
        let inherited = helper.as_raw_fd();
        let mut command = Command::new(&path);
        command
            .env_clear()
            .env(
                "RUST_LOG",
                std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
            )
            .stdin(Stdio::null())
            .args([
                "--ipc-fd",
                "3",
                "--parent-pid",
                &std::process::id().to_string(),
            ]);
        // Only the private endpoint and stdio survive exec. No inspection FDs
        // exist in this process, and none are ever passed over the socket.
        unsafe {
            command.pre_exec(move || {
                if inherited != 3 && libc::dup2(inherited, 3) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::fcntl(3, libc::F_SETFD, 0) < 0
                    || libc::syscall(libc::SYS_close_range, 4_u32, u32::MAX, 0_u32) < 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn().context("execute privileged helper")?;
        drop(helper);
        let guard = HelperGuard {
            child,
            socket: application.try_clone()?,
        };
        let hello: HelperIdentity =
            transport::read_timeout(&mut application, MAX_REQUEST, Duration::from_secs(5))?;
        ensure!(
            hello.pid == guard.child.id() && hello.version == env!("CARGO_PKG_VERSION"),
            "helper identity/version mismatch"
        );
        log::info!(
            "privileged helper pid={} capabilities={:#x}",
            hello.pid,
            hello.capabilities
        );
        Ok((application, guard))
    })();
    privileges::seal_application()?;
    let (client, _helper) = match connection {
        Ok((socket, guard)) => (
            MonitorClient::new(socket)?,
            Some(HelperSupervisor::new(guard)?),
        ),
        Err(error) => {
            log::warn!(
                "privileged helper unavailable: {error:#}; inspection requests will report unavailable"
            );
            (MonitorClient::unavailable(), None)
        }
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(super::server::run(listen, client));
    runtime.shutdown_timeout(Duration::from_secs(1));
    result
}
