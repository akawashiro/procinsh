use super::permission_help;
use anyhow::{Result, bail, ensure};

pub(super) const MAX_READ: usize = 65536;

pub(super) fn read_raw(pid: i32, address: u64, length: usize) -> Result<Vec<u8>> {
    ensure!(
        (1..=MAX_READ).contains(&length),
        "length must be 1..=65536 bytes"
    );
    ensure!(
        address.checked_add(length as u64).is_some(),
        "address range overflow"
    );
    let mut bytes = vec![0u8; length];
    let local = libc::iovec {
        iov_base: bytes.as_mut_ptr().cast(),
        iov_len: length,
    };
    let remote = libc::iovec {
        iov_base: address as *mut libc::c_void,
        iov_len: length,
    };
    // SAFETY: local points at an allocated buffer; the kernel validates remote memory.
    let count = unsafe { libc::process_vm_readv(pid, &local, 1, &remote, 1, 0) };
    if count < 0 {
        bail!(permission_help(
            "process_vm_readv",
            std::io::Error::last_os_error()
        ));
    }
    bytes.truncate(count as usize);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_own_memory_and_rejects_invalid_ranges() {
        let value = *b"procinsh";
        let id = super::super::identity(std::process::id() as i32).unwrap();
        assert_eq!(
            read_raw(id.pid, value.as_ptr() as u64, value.len()).unwrap(),
            value
        );
        assert!(read_raw(id.pid, 0, MAX_READ + 1).is_err());
        assert!(read_raw(id.pid, u64::MAX, 16).is_err());
    }
}
