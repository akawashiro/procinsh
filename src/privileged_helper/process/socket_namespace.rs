//! Pin a verified process's network namespace and open diagnostic sockets there.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`NetworkNamespaceReader`] | `pub(in crate::privileged_helper)` | `struct NetworkNamespaceReader` |

use super::{ProcessId, validate_process_identity};
use anyhow::{Context, Result, anyhow, ensure};
use std::{
    fs::File,
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::fs::MetadataExt,
    },
    thread,
    time::Instant,
};

/// Owns a namespace FD so an exiting process or reused PID cannot retarget a query.
/// Namespace switching is confined to a fresh thread that exits after creating
/// the diagnostic socket. The caller and runtime workers keep their namespaces.
pub(in crate::privileged_helper) struct NetworkNamespaceReader {
    namespace: File,
    key: (u64, u64),
}

impl NetworkNamespaceReader {
    pub(in crate::privileged_helper) fn open(id: ProcessId) -> Result<Self> {
        validate_process_identity(id)?;
        let namespace = File::open(format!("/proc/{}/ns/net", id.pid))
            .with_context(|| format!("network namespace for PID {}", id.pid))?;
        let metadata = namespace.metadata()?;
        validate_process_identity(id)?;
        Ok(Self {
            namespace,
            key: (metadata.dev(), metadata.ino()),
        })
    }

    pub(in crate::privileged_helper) fn key(&self) -> (u64, u64) {
        self.key
    }

    pub(super) fn diagnostic_socket(&self, deadline: Instant) -> Result<OwnedFd> {
        ensure!(Instant::now() < deadline, "UNIX diag timed out");
        let current = File::open("/proc/thread-self/ns/net")?.metadata()?;
        if self.key == (current.dev(), current.ino()) {
            return super::sockets::diagnostic_socket();
        }
        let namespace = self.namespace.try_clone()?;
        thread::Builder::new()
            .name("unix-diag-netns".into())
            .spawn(move || {
                ensure!(Instant::now() < deadline, "UNIX diag timed out");
                ensure!(
                    unsafe { libc::setns(namespace.as_raw_fd(), libc::CLONE_NEWNET) } == 0,
                    "enter network namespace (requires CAP_SYS_ADMIN): {}",
                    std::io::Error::last_os_error()
                );
                super::sockets::diagnostic_socket()
            })?
            .join()
            .map_err(|_| anyhow!("network namespace diagnostic thread panicked"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::net::UnixStream, time::Duration};

    fn current_namespace() -> (u64, u64) {
        let meta = fs::metadata("/proc/thread-self/ns/net").unwrap();
        (meta.dev(), meta.ino())
    }

    #[test]
    fn netns_reader_checks_identity_and_deadlines() {
        let id = super::super::identity(std::process::id() as i32).unwrap();
        let reader = NetworkNamespaceReader::open(id).unwrap();
        assert_eq!(reader.key(), current_namespace());
        let mut stale = id;
        stale.start_time_ticks += 1;
        assert!(NetworkNamespaceReader::open(stale).is_err());
        let (socket, _peer) = UnixStream::pair().unwrap();
        let inode = fs::metadata(format!("/proc/self/fd/{}", socket.as_raw_fd()))
            .unwrap()
            .ino();
        let diag =
            super::super::sockets::unix_diag(&reader, Instant::now() + Duration::from_millis(500))
                .unwrap();
        assert!(diag[&inode].peer_inode.is_some());
        let error = super::super::sockets::unix_diag(&reader, Instant::now()).unwrap_err();
        assert!(error.to_string().contains("timed out"));
    }

    #[test]
    fn netns_socket_creation_preserves_callers_namespace_and_closes_fds() {
        let fixture = super::super::TestNetworkNamespaceTarget::new(true);
        let reader = NetworkNamespaceReader::open(fixture.target.id).unwrap();
        let original = current_namespace();
        assert_ne!(reader.key(), original);
        for _ in 0..8 {
            let socket = reader
                .diagnostic_socket(Instant::now() + Duration::from_millis(500))
                .unwrap();
            let path = format!("/proc/self/fd/{}", socket.as_raw_fd());
            let inode = fs::metadata(&path).unwrap().ino();
            drop(socket);
            assert!(!fs::metadata(&path).is_ok_and(|meta| meta.ino() == inode));
            assert_eq!(current_namespace(), original);
        }
        let namespace = File::open("/dev/null").unwrap();
        let metadata = namespace.metadata().unwrap();
        let invalid = NetworkNamespaceReader {
            namespace,
            key: (metadata.dev(), metadata.ino()),
        };
        assert!(
            invalid
                .diagnostic_socket(Instant::now() + Duration::from_millis(500))
                .is_err()
        );
        assert_eq!(current_namespace(), original);
        drop(fixture);
        // The pinned namespace remains queryable after its representative exits.
        super::super::sockets::unix_diag(&reader, Instant::now() + Duration::from_millis(500))
            .unwrap();
        assert_eq!(current_namespace(), original);
    }

    #[test]
    fn netns_permission_denial_keeps_partial_results_and_unknown_is_not_unconnected() {
        let fixture = super::super::TestNetworkNamespaceTarget::new(true);
        let id = fixture.target.id;
        thread::spawn(move || {
            // Drop only this thread's effective CAP_SYS_ADMIN; its child worker
            // inherits the reduced capabilities, while other tests are unaffected.
            let mut header = [0x2008_0522u32, 0];
            let mut data = [0u32; 6];
            assert_eq!(
                unsafe { libc::syscall(libc::SYS_capget, header.as_mut_ptr(), data.as_mut_ptr()) },
                0
            );
            data[0] &= !(1 << 21);
            assert_eq!(
                unsafe { libc::syscall(libc::SYS_capset, header.as_mut_ptr(), data.as_ptr()) },
                0
            );
            let original = current_namespace();
            let reader = NetworkNamespaceReader::open(id).unwrap();
            let error = reader
                .diagnostic_socket(Instant::now() + Duration::from_millis(500))
                .unwrap_err();
            assert!(
                error.to_string().contains("Operation not permitted"),
                "{error:#}"
            );
            let details = super::super::fds::read(id).unwrap();
            let socket = details.entries.iter().find(|entry| entry.fd == 61).unwrap();
            assert_eq!(
                socket.state,
                Some(crate::privileged_helper::socket_types::SocketState::Connected)
            );
            assert!(socket.peer_inode.is_none());
            assert_ne!(
                socket.connection_status,
                Some(crate::privileged_helper::socket_types::SocketConnectionStatus::Unconnected)
            );
            assert!(
                details
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("CAP_SYS_ADMIN"))
            );
            assert_eq!(current_namespace(), original);
        })
        .join()
        .unwrap();
    }
}
