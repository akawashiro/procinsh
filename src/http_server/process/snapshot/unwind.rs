use super::{
    stack::StackFrame,
    symbol::{ElfCache, elf_address},
};
use crate::http_server::process::{maps::MemoryMap, procfs};
use framehop::{
    Module, Unwinder,
    x86_64::{CacheX86_64, UnwindRegsX86_64, UnwinderX86_64},
};
use std::sync::Arc;

type Mapping = (u64, u64, u64, u64, u32, u32);

#[derive(Default)]
pub(super) struct UnwindState {
    unwinder: UnwinderX86_64<Arc<[u8]>>,
    cache: CacheX86_64<framehop::MayAllocateDuringUnwind>,
    mappings: Vec<Mapping>,
    modules: Vec<(u64, u64, bool)>,
}

impl UnwindState {
    /// Rebuild on executable-map changes, including unload/reload and ASLR.
    /// Failed acquisitions remain diagnostic until executable mappings change.
    pub(super) fn refresh(&mut self, pid: i32, maps: &[MemoryMap], symbols: &mut ElfCache) {
        let mappings: Vec<_> = maps
            .iter()
            .filter(|m| m.executable)
            .map(|m| {
                (
                    m.start,
                    m.end,
                    m.file_offset,
                    m.inode,
                    m.device.major,
                    m.device.minor,
                )
            })
            .collect();
        if mappings == self.mappings {
            return;
        }
        self.unwinder = UnwinderX86_64::new();
        self.cache = CacheX86_64::new();
        self.modules.clear();
        self.mappings = mappings;
        for map in maps.iter().filter(|m| m.executable) {
            let Some(elf) = symbols.get(pid, map) else {
                continue;
            };
            let Some(svma) = elf_address(map.start, map, &elf, procfs::page_size()) else {
                continue;
            };
            let Some(base) = map.start.checked_sub(svma) else {
                continue;
            };
            let metadata = elf.unwind.eh_frame.is_some() || elf.unwind.debug_frame.is_some();
            self.unwinder.add_module(Module::new(
                map.pathname.clone().unwrap_or_default(),
                map.start..map.end,
                base,
                elf.unwind.clone(),
            ));
            self.modules.push((map.start, map.end, metadata));
        }
    }

    /// Reads only the immutable perf snapshot; never reads the live target stack.
    pub(super) fn walk(
        &mut self,
        rip: u64,
        rsp: u64,
        rbp: u64,
        maps: &[MemoryMap],
        stack: &[u8],
    ) -> (Vec<StackFrame>, String) {
        let mut frames = Vec::new();
        if !maps.iter().any(|m| m.readable && m.contains(rsp)) {
            return (
                vec![StackFrame::raw(rip)],
                "RSP is outside readable mappings".into(),
            );
        }
        let mut read = |address: u64| -> Result<u64, ()> {
            let offset = usize::try_from(address.checked_sub(rsp).ok_or(())?).map_err(|_| ())?;
            let bytes = stack
                .get(offset..offset.checked_add(8).ok_or(())?)
                .ok_or(())?;
            Ok(u64::from_ne_bytes(bytes.try_into().map_err(|_| ())?))
        };
        let mut iter = self.unwinder.iter_frames(
            rip,
            UnwindRegsX86_64::new(rip, rsp, rbp),
            &mut self.cache,
            &mut read,
        );
        let mut diagnostic = None;
        let reason = loop {
            match iter.next() {
                Ok(Some(frame)) => {
                    let address = frame.address();
                    let lookup = frame.address_for_lookup();
                    if !maps.iter().any(|m| m.executable && m.contains(lookup)) {
                        if frames.is_empty() {
                            frames.push(StackFrame::raw(address));
                        }
                        break "address outside executable mappings".into();
                    }
                    match self
                        .modules
                        .iter()
                        .find(|&&(start, end, _)| start <= lookup && lookup < end)
                    {
                        None => diagnostic = Some("module lookup failure; frame-pointer fallback"),
                        Some((_, _, false)) => {
                            diagnostic = Some("unwind metadata unavailable; frame-pointer fallback")
                        }
                        _ => {}
                    }
                    frames.push(StackFrame::raw(address));
                    if frames.len() >= 256 {
                        break "256-frame limit".into();
                    }
                }
                Ok(None) => break "end of stack".into(),
                Err(framehop::Error::CouldNotReadStack(address)) => {
                    let end = rsp.saturating_add(stack.len() as u64);
                    break if address >= rsp && address.saturating_add(8) > end {
                        format!("stack snapshot exhausted at 0x{address:x}")
                    } else {
                        format!("stack address outside snapshot at 0x{address:x}")
                    };
                }
                Err(error) => break format!("unwind failed: {error}"),
            }
        };
        let reason = match diagnostic {
            Some(diagnostic) => format!("{reason}; {diagnostic}"),
            None => reason,
        };
        (frames, reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http_server::process::{self, TestTarget};
    use iced_x86::{Decoder, DecoderOptions, Mnemonic, Register};
    use object::{Object, ObjectSection, ObjectSymbol};

    // Reproduce the fixture's call-site stack from its actual instructions and
    // unwind metadata. This exercises CFI even where perf privileges are absent.
    #[test]
    fn omitted_frame_pointer_snapshot_reaches_main() {
        for name in [
            "recursive_no_fp",
            "recursive_no_fp_nopie",
            "recursive_debug_frame",
        ] {
            let target = TestTarget::new(name);
            let maps = process::maps::read(target.id.pid, false).unwrap();
            let mut symbols = ElfCache::default();
            let map = maps
                .iter()
                .find(|m| m.executable && m.pathname.as_deref().is_some_and(|p| p.ends_with(name)))
                .unwrap();
            let elf = symbols.get(target.id.pid, map).unwrap();
            let base = map.start - elf_address(map.start, map, &elf, procfs::page_size()).unwrap();
            let bytes = std::fs::read(format!("tests/targets/bin/{name}")).unwrap();
            let file = object::File::parse(bytes.as_slice()).unwrap();
            let symbol = |name: &str| {
                file.symbols()
                    .find(|s| s.name().ok() == Some(name))
                    .unwrap()
            };
            let text = file.section_by_name(".text").unwrap();
            let data = text.data().unwrap();
            let mut stack = Vec::new();
            for (caller, callee) in [("bar", "baz"), ("foo", "bar"), ("main", "foo")] {
                let function = symbol(caller);
                let start = (function.address() - text.address()) as usize;
                let mut decoder = Decoder::with_ip(
                    64,
                    &data[start..start + function.size() as usize],
                    function.address(),
                    DecoderOptions::NONE,
                );
                let mut allocation = 0;
                let return_address = loop {
                    assert!(decoder.can_decode(), "missing call {caller} -> {callee}");
                    let instruction = decoder.decode();
                    if instruction.mnemonic() == Mnemonic::Call
                        && instruction.near_branch_target() == symbol(callee).address()
                    {
                        break base + instruction.next_ip();
                    }
                    if instruction.mnemonic() == Mnemonic::Push {
                        allocation += 8;
                    }
                    if instruction.mnemonic() == Mnemonic::Sub
                        && instruction.op0_register() == Register::RSP
                    {
                        allocation += instruction.immediate8to64() as usize;
                    }
                };
                stack.extend_from_slice(&return_address.to_ne_bytes());
                // Include this caller's local allocation before its saved return.
                stack.resize(stack.len() + allocation, 0);
            }
            let rsp = maps
                .iter()
                .find(|m| m.pathname.as_deref() == Some("[stack]"))
                .unwrap()
                .start;
            for indexed in [true, false] {
                let mut state = UnwindState::default();
                state.refresh(target.id.pid, &maps, &mut symbols);
                if !indexed {
                    let mut sections = elf.unwind.clone();
                    sections.eh_frame_hdr = None;
                    sections.eh_frame_hdr_svma = None;
                    state.unwinder.add_module(Module::new(
                        name.into(),
                        map.start..map.end,
                        base,
                        sections,
                    ));
                }
                let (frames, stop) =
                    state.walk(base + symbol("baz").address(), rsp, 1, &maps, &stack);
                let (short_frames, short_stop) =
                    state.walk(base + symbol("baz").address(), rsp, 1, &maps, &stack[..8]);
                assert_eq!(short_frames.len(), 2, "{short_frames:?}");
                assert!(
                    short_stop.contains("stack snapshot exhausted"),
                    "{short_stop}"
                );
                let expected: Vec<_> = ["baz", "bar", "foo", "main"]
                    .map(|name| base + symbol(name).address())
                    .into();
                assert!(frames.len() >= 4, "{name}: {frames:?}, {stop}");
                for (frame, start) in frames.iter().zip(expected) {
                    assert!(
                        frame.address >= start && frame.address - start < 256,
                        "{name}: {frames:?}"
                    );
                }
                assert!(
                    stop == "end of stack" || stop.contains("stack snapshot exhausted"),
                    "{stop}"
                );
            }
            let mut state = UnwindState::default();
            state.refresh(target.id.pid, &maps, &mut symbols);
            assert!(!state.modules.is_empty());
            state.refresh(target.id.pid, &[], &mut symbols);
            assert!(state.modules.is_empty());
            target.assert_detached();
        }
    }

    #[test]
    fn fallback_reports_module_failure_and_bounds_stack_reads() {
        let maps = vec![
            process::maps::parse_map("1000-2000 rw-p 0 00:00 0 [stack]").unwrap(),
            process::maps::parse_map("3000-4000 r-xp 0 00:00 0 /missing").unwrap(),
        ];
        let mut state = UnwindState::default();
        let (_, reason) = state.walk(0x3010, 0x1000, 0x1100, &maps, &[]);
        assert!(reason.contains("snapshot exhausted"), "{reason}");
        assert!(reason.contains("module lookup failure"), "{reason}");
        state.modules.push((0x3000, 0x4000, false));
        let (_, reason) = state.walk(0x3010, 0x1000, 0x1100, &maps, &[]);
        assert!(reason.contains("unwind metadata unavailable"), "{reason}");
        let (frames, reason) = state.walk(0x3010, 0x2000, 0, &maps, &[]);
        assert_eq!(frames.len(), 1);
        assert!(reason.contains("RSP"));
    }
}
