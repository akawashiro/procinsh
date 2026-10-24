use super::cache::ElfSymbols;
use crate::privileged_helper::process::{maps::MemoryMap, snapshot::stack::SourceFrame};
/// Symbol and source information for one ELF address.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::privileged_helper::process::snapshot) struct SymbolInfo {
    pub(in crate::privileged_helper::process::snapshot) name: Option<String>,
    pub(in crate::privileged_helper::process::snapshot) offset: Option<u64>,
    pub(in crate::privileged_helper::process::snapshot) file: Option<String>,
    pub(in crate::privileged_helper::process::snapshot) line: Option<u32>,
    pub(in crate::privileged_helper::process::snapshot) inline_frames: Vec<SourceFrame>,
}

/// Correct a saved return address before selecting its mapping.
/// The currently executing instruction must be passed with `return_address = false`.
pub(in crate::privileged_helper::process::snapshot) fn instruction_address(
    address: u64,
    return_address: bool,
) -> u64 {
    address.saturating_sub(u64::from(return_address))
}

/// Convert a corrected runtime address into an ELF address, accounting for the
/// mapping offset, PIE/ASLR load bias, and page-aligned segment mappings.
pub(in crate::privileged_helper::process::snapshot) fn elf_address(
    address: u64,
    map: &MemoryMap,
    elf: &ElfSymbols,
    page: u64,
) -> Option<u64> {
    if page == 0 || !map.contains(address) {
        return None;
    }
    let offset = address
        .checked_sub(map.start)?
        .checked_add(map.file_offset)?;
    let &(file_offset, _, virtual_address) = elf.segments.iter().find(|&&(off, size, _)| {
        offset >= off / page * page
            && (offset as u128) < (off as u128 + size as u128).div_ceil(page as u128) * page as u128
    })?;
    (offset as i128 + virtual_address as i128 - file_offset as i128)
        .try_into()
        .ok()
}

/// Resolve an ELF address without changing a frame or the ELF cache.
/// Returns `None` only when no symbol, source location, or inline frame is found.
/// DWARF reads only bounded in-memory sections under the ELF's private lock.
pub(in crate::privileged_helper::process::snapshot) fn resolve_frame(
    address: u64,
    elf: &ElfSymbols,
) -> Option<SymbolInfo> {
    let mut info = SymbolInfo::default();
    if let Some((start, _, name)) = elf.symbols.iter().rev().find(|(start, size, _)| {
        *start <= address && (*size == 0 || address < start.saturating_add(*size))
    }) {
        info.name = Some(name.clone());
        info.offset = Some(address - start);
    }
    if let Some(loader) = &elf.dwarf {
        let loader = loader.lock().unwrap_or_else(|e| e.into_inner());
        if let Ok(mut iter) = loader.find_frames(address).skip_all_loads() {
            while info.inline_frames.len() < 256 {
                let Ok(Some(f)) = iter.next() else {
                    break;
                };
                let function = f
                    .function
                    .and_then(|f| f.demangle().ok().map(|v| v.chars().take(4096).collect()));
                let file = f
                    .location
                    .as_ref()
                    .and_then(|l| l.file.map(|v| v.chars().take(4096).collect()));
                let line = f.location.and_then(|l| l.line);
                info.inline_frames.push(SourceFrame {
                    function,
                    file,
                    line,
                });
            }
        }
        if let Some(source) = info.inline_frames.first() {
            if source.function.is_some() {
                info.name = source.function.clone();
            }
            info.file = source.file.clone();
            info.line = source.line;
        } else if let Ok(Some(location)) = loader.find_location(address) {
            info.file = location.file.map(|v| v.chars().take(4096).collect());
            info.line = location.line;
        }
    }
    (info.name.is_some()
        || info.file.is_some()
        || info.line.is_some()
        || !info.inline_frames.is_empty())
    .then_some(info)
}
