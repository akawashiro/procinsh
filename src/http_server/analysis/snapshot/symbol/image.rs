//! Main-owned sparse ELF bytes and selection of symbol, DWARF and unwind sections.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`ElfImage`] | `pub(crate)` | `struct ElfImage` |
//! | [`create_elf_image`] | `pub(crate)` | `fn create_elf_image(data: `[`MappedFileData`]`) -> `[`Result`]`<`[`ElfImage`]`>` |
//! | [`extend_elf_image`] | `pub(crate)` | `fn extend_elf_image(state: &mut `[`ElfImage`]`, data: `[`MappedFileData`]`) -> `[`Result`]`<()>` |
//! | [`tables_elf_image`] | `pub(crate)` | `fn tables_elf_image(state: &`[`ElfImage`]`) -> `[`Result`]`<`[`Vec`]`<`[`FileRange`]`>>` |
//! | [`names_elf_image`] | `pub(crate)` | `fn names_elf_image(state: &`[`ElfImage`]`) -> `[`Result`]`<`[`Vec`]`<`[`FileRange`]`>>` |
//! | [`sections_elf_image`] | `pub(crate)` | `fn sections_elf_image(state: &`[`ElfImage`]`) -> `[`Result`]`<`[`Vec`]`<`[`FileRange`]`>>` |
//! | [`needed_section`] | `pub(super)` | `fn needed_section(name: &str) -> bool` |

use crate::ipc::elf::{
    ElfFileId, FileChunk, FileRange, MAX_FILE_SIZE, MAX_READ_SIZE, MappedFileData,
};
use anyhow::{Context, Result, ensure};
use object::read::ReadRef;
use std::ops::Range;

pub(crate) struct ElfImage {
    pub(super) identity: ElfFileId,
    chunks: Vec<FileChunk>,
}
pub(crate) fn create_elf_image(data: MappedFileData) -> Result<ElfImage> {
    ensure!(data.identity.2 <= MAX_FILE_SIZE, "ELF file exceeds limit");
    let mut image = ElfImage {
        identity: data.identity,
        chunks: Vec::new(),
    };
    extend_elf_image(&mut image, data)?;
    Ok(image)
}

pub(crate) fn extend_elf_image(state: &mut ElfImage, data: MappedFileData) -> Result<()> {
    ensure!(
        data.identity == state.identity,
        "mapped file changed during acquisition"
    );
    state.chunks.extend(data.chunks);
    state.chunks.sort_by_key(|chunk| chunk.offset);
    let mut merged: Vec<FileChunk> = Vec::new();
    for chunk in state.chunks.drain(..) {
        let end = chunk
            .offset
            .checked_add(chunk.bytes.len() as u64)
            .context("ELF range overflow")?;
        ensure!(end <= state.identity.2, "ELF range exceeds file");
        if let Some(last) = merged.last_mut()
            && chunk.offset <= last.offset + last.bytes.len() as u64
        {
            let overlap = (last.offset + last.bytes.len() as u64 - chunk.offset) as usize;
            if overlap < chunk.bytes.len() {
                last.bytes.extend_from_slice(&chunk.bytes[overlap..]);
            }
        } else {
            merged.push(chunk);
        }
    }
    ensure!(
        merged
            .iter()
            .map(|chunk| chunk.bytes.len() as u64)
            .sum::<u64>()
            <= MAX_READ_SIZE,
        "ELF acquisition exceeds limit"
    );
    state.chunks = merged;
    Ok(())
}

fn bytes_elf_image(state: &ElfImage, offset: u64, length: u64) -> Result<&[u8]> {
    state
        .read_bytes_at(offset, length)
        .map_err(|()| anyhow::anyhow!("missing ELF range"))
}

fn number_elf_image(state: &ElfImage, offset: u64, length: u64) -> Result<u64> {
    let bytes = bytes_elf_image(state, offset, length)?;
    Ok(match length {
        2 => u16::from_le_bytes(bytes.try_into()?) as u64,
        4 => u32::from_le_bytes(bytes.try_into()?) as u64,
        8 => u64::from_le_bytes(bytes.try_into()?),
        _ => unreachable!(),
    })
}

fn is64_elf_image(state: &ElfImage) -> Result<bool> {
    let ident = bytes_elf_image(state, 0, 16)?;
    ensure!(
        &ident[..4] == b"\x7fELF" && ident[5] == 1 && ident[6] == 1,
        "unsupported ELF header"
    );
    match ident[4] {
        1 => Ok(false),
        2 => Ok(true),
        _ => anyhow::bail!("unsupported ELF class"),
    }
}

fn section_table_elf_image(state: &ElfImage) -> Result<(u64, u64, u64, u64)> {
    let wide = is64_elf_image(state)?;
    let offset = number_elf_image(state, if wide { 40 } else { 32 }, if wide { 8 } else { 4 })?;
    let size = number_elf_image(state, if wide { 58 } else { 46 }, 2)?;
    let count = number_elf_image(state, if wide { 60 } else { 48 }, 2)?;
    let names = number_elf_image(state, if wide { 62 } else { 50 }, 2)?;
    ensure!(
        count > 0 && count <= 4096 && size == if wide { 64 } else { 40 } && names < count,
        "unsupported ELF section table"
    );
    Ok((offset, size, count, names))
}

pub(crate) fn tables_elf_image(state: &ElfImage) -> Result<Vec<FileRange>> {
    let (offset, size, count, _) = section_table_elf_image(state)?;
    let wide = is64_elf_image(state)?;
    let programs = number_elf_image(state, if wide { 32 } else { 28 }, if wide { 8 } else { 4 })?;
    let entry_size = number_elf_image(state, if wide { 54 } else { 42 }, 2)?;
    let entries = number_elf_image(state, if wide { 56 } else { 44 }, 2)?;
    ensure!(entries <= 4096, "too many ELF segments");
    ranges_elf_image(
        state,
        vec![
            FileRange {
                offset,
                length: size * count,
            },
            FileRange {
                offset: programs,
                length: entry_size * entries,
            },
        ],
    )
}

fn section_elf_image(state: &ElfImage, index: u64) -> Result<(u64, u64, u64, u64)> {
    let (table, size, count, _) = section_table_elf_image(state)?;
    ensure!(index < count, "invalid ELF section index");
    let at = table + index * size;
    let wide = is64_elf_image(state)?;
    Ok((
        number_elf_image(state, at, 4)?,
        number_elf_image(state, at + 4, 4)?,
        number_elf_image(
            state,
            at + if wide { 24 } else { 16 },
            if wide { 8 } else { 4 },
        )?,
        number_elf_image(
            state,
            at + if wide { 32 } else { 20 },
            if wide { 8 } else { 4 },
        )?,
    ))
}

pub(crate) fn names_elf_image(state: &ElfImage) -> Result<Vec<FileRange>> {
    let (_, _, _, index) = section_table_elf_image(state)?;
    let (_, _, offset, length) = section_elf_image(state, index)?;
    ranges_elf_image(state, vec![FileRange { offset, length }])
}

pub(crate) fn sections_elf_image(state: &ElfImage) -> Result<Vec<FileRange>> {
    let (_, _, count, index) = section_table_elf_image(state)?;
    let (_, _, offset, length) = section_elf_image(state, index)?;
    let names = bytes_elf_image(state, offset, length)?;
    let mut ranges = Vec::new();
    for index in 0..count {
        let (name, kind, offset, length) = section_elf_image(state, index)?;
        if kind == 8 || length == 0 {
            continue;
        } // SHT_NOBITS has no file bytes.
        let name = names
            .get(name as usize..)
            .context("invalid ELF section name")?;
        let end = name
            .iter()
            .position(|byte| *byte == 0)
            .context("unterminated ELF section name")?;
        let name = std::str::from_utf8(&name[..end])?;
        if needed_section(name) || matches!(kind, 2 | 3 | 11 | 18) {
            ensure!(length <= 32 * 1024 * 1024, "ELF section exceeds limit");
            ranges.push(FileRange { offset, length });
        }
    }
    ranges_elf_image(state, ranges)
}

fn ranges_elf_image(state: &ElfImage, mut ranges: Vec<FileRange>) -> Result<Vec<FileRange>> {
    ranges.retain(|range| {
        range.length != 0 && state.read_bytes_at(range.offset, range.length).is_err()
    });
    ranges.sort_by_key(|range| range.offset);
    let mut merged: Vec<FileRange> = Vec::new();
    for range in ranges {
        let end = range
            .offset
            .checked_add(range.length)
            .context("ELF range overflow")?;
        ensure!(end <= state.identity.2, "ELF range exceeds file");
        if let Some(last) = merged.last_mut()
            && range.offset <= last.offset + last.length
        {
            last.length = last.length.max(end - last.offset);
        } else {
            merged.push(range);
        }
    }
    ensure!(
        merged.iter().map(|range| range.length).sum::<u64>() <= MAX_READ_SIZE,
        "ELF ranges exceed limit"
    );
    Ok(merged)
}

pub(super) fn needed_section(name: &str) -> bool {
    name.starts_with(".debug_")
        || name.starts_with(".zdebug_")
        || matches!(
            name,
            ".eh_frame"
                | ".eh_frame_hdr"
                | ".symtab"
                | ".dynsym"
                | ".strtab"
                | ".dynstr"
                | ".shstrtab"
                | ".symtab_shndx"
        )
}

impl<'a> ReadRef<'a> for &'a ElfImage {
    fn len(self) -> std::result::Result<u64, ()> {
        Ok(self.identity.2)
    }
    fn read_bytes_at(self, offset: u64, size: u64) -> std::result::Result<&'a [u8], ()> {
        if offset > self.identity.2 {
            return Err(());
        }
        if size == 0 {
            return Ok(&[]);
        }
        let end = offset.checked_add(size).ok_or(())?;
        self.chunks
            .iter()
            .find_map(|chunk| {
                (offset >= chunk.offset && end <= chunk.offset + chunk.bytes.len() as u64).then(
                    || {
                        &chunk.bytes
                            [(offset - chunk.offset) as usize..(end - chunk.offset) as usize]
                    },
                )
            })
            .ok_or(())
    }
    fn read_bytes_at_until(
        self,
        range: Range<u64>,
        delimiter: u8,
    ) -> std::result::Result<&'a [u8], ()> {
        if range.start > range.end || range.end > self.identity.2 {
            return Err(());
        }
        let chunk = self
            .chunks
            .iter()
            .find(|chunk| {
                range.start >= chunk.offset && range.start < chunk.offset + chunk.bytes.len() as u64
            })
            .ok_or(())?;
        let end = range.end.min(chunk.offset + chunk.bytes.len() as u64);
        let bytes = chunk
            .bytes
            .get((range.start - chunk.offset) as usize..(end - chunk.offset) as usize)
            .ok_or(())?;
        let length = bytes.iter().position(|byte| *byte == delimiter).ok_or(())?;
        Ok(&bytes[..length])
    }
}
