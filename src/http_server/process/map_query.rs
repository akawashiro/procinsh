//! Reads structural virtual-memory mappings with Linux's binary PROCMAP_QUERY API.
//!
//! # Interface
//!
//! | Definition | Visibility | Signature |
//! | --- | --- | --- |
//! | [`read_maps`] | `pub(super)` | `fn read_maps(file: &`[`File`]`) -> `[`io::Result`]`<`[`Option`]`<`[`Vec`]`<`[`MemoryMap`]`>>>` |

use super::maps::MemoryMap;
use crate::http_server::resource::DeviceId;
use std::{fs::File, io, os::fd::AsRawFd};

const COVERING_OR_NEXT_VMA: u64 = 0x10;
const MAX_NAME_BYTES: usize = 1024 * 1024;

/// linux/fs.h UAPI layout, available since Linux 6.11. Optional build IDs are not requested.
#[repr(C)]
#[derive(Default)]
struct ProcmapQuery {
    size: u64,
    query_flags: u64,
    query_addr: u64,
    vma_start: u64,
    vma_end: u64,
    vma_flags: u64,
    vma_page_size: u64,
    vma_offset: u64,
    inode: u64,
    dev_major: u32,
    dev_minor: u32,
    vma_name_size: u32,
    build_id_size: u32,
    vma_name_addr: u64,
    build_id_addr: u64,
}

const _: () = assert!(size_of::<ProcmapQuery>() == 104);
const PROCMAP_QUERY: libc::c_ulong = libc::_IOWR::<ProcmapQuery>(b'f' as u32, 17);

/// Returns None only when the file does not support PROCMAP_QUERY (ENOTTY).
/// Other errors propagate, including errors after a partially completed traversal.
/// Each returned VMA is self-consistent; concurrent mmap/munmap can change the
/// address space between ioctls, so the traversal is not an atomic snapshot.
/// The kernel's x86 `[vsyscall]` gate is a pseudo-mapping outside this API's VMA tree.
pub(super) fn read_maps(file: &File) -> io::Result<Option<Vec<MemoryMap>>> {
    let mut maps = Vec::new();
    let mut address = 0;
    let mut name = vec![0u8; 256];
    loop {
        let mut query = ProcmapQuery {
            size: size_of::<ProcmapQuery>() as u64,
            query_flags: COVERING_OR_NEXT_VMA,
            query_addr: address,
            vma_name_size: name.len() as u32,
            vma_name_addr: name.as_mut_ptr() as u64,
            ..Default::default()
        };
        // SAFETY: query has the UAPI layout and size; its sole nonzero buffer
        // pointer refers to name, which stays allocated and writable during ioctl.
        let result = unsafe { libc::ioctl(file.as_raw_fd(), PROCMAP_QUERY, &mut query) };
        if result < 0 {
            let error = io::Error::last_os_error();
            match error.raw_os_error() {
                Some(libc::ENOENT) => return Ok(Some(maps)),
                Some(libc::ENOTTY) if maps.is_empty() => return Ok(None),
                Some(libc::EINTR) => continue,
                // d_path can report ENAMETOOLONG before the ioctl's E2BIG check.
                Some(libc::E2BIG | libc::ENAMETOOLONG) if name.len() < MAX_NAME_BYTES => {
                    name.resize(name.len() * 2, 0);
                    continue;
                }
                _ => return Err(error),
            }
        }
        if query.vma_start >= query.vma_end || query.vma_end <= address {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "non-progressing VMA query",
            ));
        }
        let pathname = if query.vma_name_size == 0 {
            None
        } else {
            let bytes = name.get(..query.vma_name_size as usize).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid VMA name length")
            })?;
            let bytes = bytes.strip_suffix(&[0]).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "unterminated VMA name")
            })?;
            let path = std::str::from_utf8(bytes)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            // Match the text reader's newline escaping and trailing whitespace handling.
            Some(if path.contains('\n') {
                path.replace('\n', "\\012").trim_end().to_owned()
            } else {
                path.trim_end().to_owned()
            })
        };
        maps.push(MemoryMap {
            start: query.vma_start,
            end: query.vma_end,
            readable: query.vma_flags & 0x01 != 0,
            writable: query.vma_flags & 0x02 != 0,
            executable: query.vma_flags & 0x04 != 0,
            private: query.vma_flags & 0x08 == 0,
            file_offset: query.vma_offset,
            device: DeviceId {
                major: query.dev_major,
                minor: query.dev_minor,
            },
            inode: query.inode,
            pathname,
        });
        address = query.vma_end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http_server::process::{maps::parse_map, test_support::Target};

    #[test]
    fn binary_maps_match_text_for_a_stable_process() {
        let target = Target::new("mmap_test");
        let path = format!("/proc/{}/maps", target.id.pid);
        let text: Vec<_> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| parse_map(line).unwrap())
            .filter(|map| map.pathname.as_deref() != Some("[vsyscall]"))
            .collect();
        let binary = read_maps(&File::open(path).unwrap())
            .unwrap()
            .expect("PROCMAP_QUERY is required for this test");
        assert_eq!(binary, text);
        assert!(
            binary
                .iter()
                .any(|m| !m.readable && !m.writable && !m.executable)
        );
    }

    #[test]
    fn long_deleted_path_with_spaces_and_newline_matches_text() {
        use std::{io::Write, os::unix::ffi::OsStrExt};
        let dir = std::env::temp_dir().join(format!("procinsh-map-query-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let nested = dir.join("x".repeat(100)).join("z".repeat(100));
        std::fs::create_dir_all(&nested).unwrap();
        let path = nested.join(format!("{} spaces\nand newline", "y".repeat(100)));
        let mut file = File::create(&path).unwrap();
        file.write_all(&[0; 8192]).unwrap();
        let file = File::open(&path).unwrap();
        // SAFETY: mmap owns a fresh read-only page, released below after querying.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                4096,
                libc::PROT_READ,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                4096,
            )
        };
        assert_ne!(ptr, libc::MAP_FAILED);
        std::fs::remove_file(&path).unwrap();
        let binary = read_maps(&File::open("/proc/self/maps").unwrap())
            .unwrap()
            .expect("PROCMAP_QUERY is required for this test");
        let actual = binary
            .iter()
            .find(|m| m.start <= ptr as u64 && (ptr as u64) < m.end)
            .unwrap();
        let text = std::fs::read_to_string("/proc/self/maps").unwrap();
        let expected = text
            .lines()
            .filter_map(|line| parse_map(line).ok())
            .find(|m| m.start == actual.start)
            .unwrap();
        assert_eq!(*actual, expected);
        assert!(!actual.private);
        assert_eq!(actual.file_offset, 4096);
        assert!(actual.pathname.as_ref().unwrap().ends_with(" (deleted)"));
        assert!(path.as_os_str().as_bytes().len() > 256);
        // SAFETY: ptr and length are exactly the successful mmap above.
        assert_eq!(unsafe { libc::munmap(ptr, 4096) }, 0);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
