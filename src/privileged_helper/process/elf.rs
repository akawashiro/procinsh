//! Acquire ELF bytes from verified regular files without parsing ELF or DWARF.
//!
//! # Interface
//!
//! [`read_mapped_file_ranges`] (`pub(super)`): `fn read_mapped_file_ranges(pid: i32, map: &`[`MemoryMap`]`, ranges: &[`[`FileRange`]`]) -> `[`anyhow::Result`]`<`[`MonitorResponse`](crate::ipc::schema::MonitorResponse)`>`.

use crate::ipc::elf::{
    FileChunk, FileRange, MAX_FILE_SIZE, MAX_RANGES, MAX_READ_SIZE, MappedFileData,
};
use crate::ipc::model::memory::MemoryMap;
#[cfg(test)]
use std::io::Read;
use std::os::unix::fs::FileExt;
use std::{
    fs,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::PathBuf,
};

fn file_identity(meta: &fs::Metadata) -> (u64, u64, u64, i64, i64) {
    (
        meta.dev(),
        meta.ino(),
        meta.len(),
        meta.mtime(),
        meta.mtime_nsec(),
    )
}

fn matching_file(pid: i32, map: &MemoryMap) -> Option<(fs::File, fs::Metadata)> {
    let mut paths = vec![
        PathBuf::from(format!(
            "/proc/{pid}/map_files/{:x}-{:x}",
            map.start, map.end
        )),
        PathBuf::from(format!("/proc/{pid}/exe")),
    ];
    if let Some(path) = map
        .pathname
        .as_deref()
        .filter(|p| p.starts_with('/') && !p.ends_with(" (deleted)"))
    {
        paths.push(PathBuf::from(format!("/proc/{pid}/root{path}")));
    }
    paths.into_iter().find_map(|path| {
        // O_PATH pins the selected inode without opening devices or blocking on
        // FIFOs. Only verified regular files may then be opened for reading.
        let pinned = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_PATH | libc::O_CLOEXEC)
            .open(&path)
            .ok()?;
        let meta = pinned.metadata().ok()?;
        if !meta.is_file()
            || meta.ino() != map.inode
            || libc::major(meta.dev()) != map.device.major
            || libc::minor(meta.dev()) != map.device.minor
        {
            return None;
        }
        let descriptor = fs::File::open(format!("/proc/self/fd/{}", pinned.as_raw_fd())).ok()?;
        (file_identity(&descriptor.metadata().ok()?) == file_identity(&meta))
            .then_some((descriptor, meta))
    })
}

pub(super) fn read_mapped_file_ranges(
    pid: i32,
    map: &MemoryMap,
    ranges: &[FileRange],
) -> anyhow::Result<crate::ipc::schema::MonitorResponse> {
    use crate::ipc::schema::MonitorResponse;
    let Some((descriptor, before)) = matching_file(pid, map) else {
        return Ok(MonitorResponse::Elf(None));
    };
    if before.len() > MAX_FILE_SIZE {
        return Ok(MonitorResponse::Elf(None));
    }
    let identity = file_identity(&before);
    anyhow::ensure!(ranges.len() <= MAX_RANGES, "too many mapped file ranges");
    let mut total = 0u64;
    for range in ranges {
        anyhow::ensure!(
            range
                .offset
                .checked_add(range.length)
                .is_some_and(|end| end <= before.len()),
            "mapped file range out of bounds"
        );
        total = total
            .checked_add(range.length)
            .ok_or_else(|| anyhow::anyhow!("mapped file read overflow"))?;
        anyhow::ensure!(total <= MAX_READ_SIZE, "mapped file read exceeds limit");
    }
    let mut chunks = Vec::with_capacity(ranges.len());
    for range in ranges {
        let mut bytes = vec![0; range.length as usize];
        descriptor.read_exact_at(&mut bytes, range.offset)?;
        chunks.push(FileChunk {
            offset: range.offset,
            bytes,
        });
    }
    if identity != file_identity(&descriptor.metadata()?) {
        return Ok(MonitorResponse::Elf(None));
    }
    Ok(MonitorResponse::Elf(Some(MappedFileData {
        identity,
        chunks,
    })))
}

#[cfg(test)]
pub(in crate::privileged_helper::process) fn read_mapped_elf_file(
    pid: i32,
    map: &MemoryMap,
) -> Option<crate::http_server::analysis::snapshot::symbol::ElfFile> {
    let length = matching_file(pid, map)?.1.len();
    match read_mapped_file_ranges(pid, map, &[FileRange { offset: 0, length }]).ok()? {
        crate::ipc::schema::MonitorResponse::Elf(Some(file)) => {
            Some(crate::http_server::analysis::snapshot::symbol::ElfFile {
                identity: file.identity,
                bytes: file.chunks.into_iter().next()?.bytes,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verifies_identity_pins_regular_files_and_bounds_reads() {
        let root =
            std::env::temp_dir().join(format!("procinsh-elf-acquisition-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("mapped");
        fs::write(&path, b"captured file").unwrap();
        let metadata = fs::metadata(&path).unwrap();
        let map = crate::ipc::model::memory::parse_map(&format!(
            "1000-2000 r-xp 0 {:x}:{:x} {} {}",
            libc::major(metadata.dev()),
            libc::minor(metadata.dev()),
            metadata.ino(),
            path.display()
        ))
        .unwrap();
        let pid = std::process::id() as i32;
        let captured = read_mapped_elf_file(pid, &map).unwrap();
        assert_eq!(captured.bytes, b"captured file");
        let crate::ipc::schema::MonitorResponse::Elf(Some(partial)) = read_mapped_file_ranges(
            pid,
            &map,
            &[FileRange {
                offset: 2,
                length: 4,
            }],
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(partial.chunks[0].bytes, b"ptur");
        assert!(
            read_mapped_file_ranges(
                pid,
                &map,
                &[FileRange {
                    offset: metadata.len(),
                    length: 1
                }]
            )
            .is_err()
        );
        assert!(
            read_mapped_file_ranges(
                pid,
                &map,
                &vec![
                    FileRange {
                        offset: 0,
                        length: 1
                    };
                    MAX_RANGES + 1
                ]
            )
            .is_err()
        );
        let (mut descriptor, _) = matching_file(pid, &map).unwrap();
        fs::rename(&path, root.join("pinned")).unwrap();
        fs::write(&path, b"replacement").unwrap();
        assert!(read_mapped_elf_file(pid, &map).is_none());
        let mut pinned = Vec::new();
        descriptor.read_to_end(&mut pinned).unwrap();
        assert_eq!(pinned, b"captured file");
        let mut wrong = map.clone();
        wrong.inode = 0;
        assert!(read_mapped_elf_file(pid, &wrong).is_none());
        let large = root.join("large");
        fs::File::create(&large)
            .unwrap()
            .set_len(MAX_FILE_SIZE + 1)
            .unwrap();
        let meta = fs::metadata(&large).unwrap();
        let large_map = crate::ipc::model::memory::parse_map(&format!(
            "1000-2000 r-xp 0 {:x}:{:x} {} {}",
            libc::major(meta.dev()),
            libc::minor(meta.dev()),
            meta.ino(),
            large.display()
        ))
        .unwrap();
        assert!(read_mapped_elf_file(pid, &large_map).is_none());
        fs::remove_dir_all(root).unwrap();
    }
}
