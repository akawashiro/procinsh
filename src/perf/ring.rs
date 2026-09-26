use anyhow::{Result, ensure};
use std::{
    ptr::NonNull,
    sync::atomic::{AtomicU64, Ordering, fence},
};

pub struct Ring {
    base: NonNull<u8>,
    length: usize,
    offset: usize,
    size: usize,
    tail: u64,
}
// The mapping is owned by a single worker; the kernel is its only producer.
unsafe impl Send for Ring {}
impl Ring {
    pub fn new(fd: i32) -> Result<Self> {
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
        let length = page * 9;
        let raw = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                length,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        if raw == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error().into());
        }
        let base = NonNull::new(raw.cast::<u8>()).unwrap();
        let mut ring = Self {
            base,
            length,
            offset: page,
            size: page * 8,
            tail: 0,
        };
        // Metadata layout from linux/perf_event.h (data_head at byte 1024).
        let offset = unsafe { base.as_ptr().add(1040).cast::<u64>().read_volatile() } as usize;
        let size = unsafe { base.as_ptr().add(1048).cast::<u64>().read_volatile() } as usize;
        ensure!(
            offset >= page
                && size.is_power_of_two()
                && offset.checked_add(size).is_some_and(|end| end <= length),
            "Invalid perf mmap layout"
        );
        ring.offset = offset;
        ring.size = size;
        Ok(ring)
    }
    pub fn drain(&mut self) -> Result<Vec<Vec<u8>>> {
        // Acquire pairs with kernel publication. Tail must follow all data reads.
        let head =
            unsafe { &*self.base.as_ptr().add(1024).cast::<AtomicU64>() }.load(Ordering::Acquire);
        let available = head.wrapping_sub(self.tail);
        let result = if available > self.size as u64 {
            Err(anyhow::anyhow!(
                "perf ring overrun: unread bytes exceed capacity"
            ))
        } else {
            let bytes = unsafe {
                std::slice::from_raw_parts(self.base.as_ptr().add(self.offset), self.size)
            };
            records(bytes, self.tail, head)
        };
        self.tail = head;
        fence(Ordering::SeqCst);
        unsafe { &*self.base.as_ptr().add(1032).cast::<AtomicU64>() }
            .store(head, Ordering::Release);
        result
    }
}
impl Drop for Ring {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.base.as_ptr().cast(), self.length);
        }
    }
}
fn records(data: &[u8], mut tail: u64, head: u64) -> Result<Vec<Vec<u8>>> {
    ensure!(
        !data.is_empty() && head.wrapping_sub(tail) <= data.len() as u64,
        "Invalid ring distance"
    );
    let mut out = Vec::new();
    while tail != head {
        ensure!(head.wrapping_sub(tail) >= 8, "Truncated perf header");
        let read = |i: usize| data[(tail.wrapping_add(i as u64) % data.len() as u64) as usize];
        let size = u16::from_ne_bytes([read(6), read(7)]) as usize;
        ensure!(
            size >= 8 && size <= data.len() && size as u64 <= head.wrapping_sub(tail),
            "Invalid perf record size {size}"
        );
        out.push((0..size).map(read).collect());
        tail = tail.wrapping_add(size as u64);
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrapped_records_and_bad_lengths() {
        let mut ring = [0u8; 32];
        let record = [9, 0, 0, 0, 0, 0, 16, 0, 1, 2, 3, 4, 5, 6, 7, 8];
        for (i, b) in record.iter().enumerate() {
            ring[(28 + i) % 32] = *b;
        }
        assert_eq!(records(&ring, 28, 44).unwrap(), vec![record.to_vec()]);
        assert!(records(&ring, 28, 35).is_err());
        ring[2] = 0;
        assert!(records(&ring, 28, 44).is_err());
        assert!(records(&ring, 0, 33).is_err());
    }
}
