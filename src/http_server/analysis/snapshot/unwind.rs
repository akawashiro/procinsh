//! Unwind captured stack bytes using main-owned ELF metadata.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`StackUnwinder`] | `pub` | `struct StackUnwinder` |

use super::{
    stack::StackFrame,
    symbol::{ElfCache, elf_address},
};
use crate::http_server::model::memory::MemoryMap;
use crate::ipc::registers::RegisterSet;
#[cfg(target_arch = "aarch64")]
use framehop::aarch64::{
    CacheAarch64 as NativeCache, PtrAuthMask, UnwindRegsAarch64, UnwinderAarch64 as NativeUnwinder,
};
#[cfg(target_arch = "x86_64")]
use framehop::x86_64::{
    CacheX86_64 as NativeCache, UnwindRegsX86_64, UnwinderX86_64 as NativeUnwinder,
};
use framehop::{Module, Unwinder};
use std::sync::Arc;

type Mapping = (u64, u64, u64, u64, u32, u32);

#[derive(Default)]
/// Unwinds sampled stacks using cached rules and executable mapping metadata.
pub struct StackUnwinder {
    unwinder: NativeUnwinder<Arc<[u8]>>,
    cache: NativeCache<framehop::MayAllocateDuringUnwind>,
    mappings: Vec<Mapping>,
    modules: Vec<(u64, u64, bool)>,
}

impl StackUnwinder {
    /// Rebuild on executable-map changes, including unload/reload and ASLR.
    /// Failed acquisitions remain diagnostic until executable mappings change.
    pub fn refresh(&mut self, maps: &[MemoryMap], symbols: &ElfCache) {
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
        self.unwinder = NativeUnwinder::new();
        self.cache = NativeCache::new();
        self.modules.clear();
        self.mappings = mappings;
        for map in maps.iter().filter(|m| m.executable) {
            let Some(elf) = symbols.get(map) else {
                continue;
            };
            let Some(svma) = elf_address(map.start, map, &elf, unsafe {
                libc::sysconf(libc::_SC_PAGESIZE) as u64
            }) else {
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

    /// Unwinds from the supplied register values and captured stack bytes.
    pub fn walk(
        &mut self,
        registers: &RegisterSet,
        maps: &[MemoryMap],
        stack: &[u8],
    ) -> (Vec<StackFrame>, String) {
        let (rip, rsp, rbp) = (registers.pc(), registers.sp(), registers.fp());
        let mut frames = Vec::new();
        if !maps.iter().any(|m| m.readable && m.contains(rsp)) {
            return (
                vec![StackFrame::raw(rip)],
                "SP is outside readable mappings".into(),
            );
        }
        let mut read = |address: u64| -> Result<u64, ()> {
            let offset = usize::try_from(address.checked_sub(rsp).ok_or(())?).map_err(|_| ())?;
            let bytes = stack
                .get(offset..offset.checked_add(8).ok_or(())?)
                .ok_or(())?;
            Ok(u64::from_ne_bytes(bytes.try_into().map_err(|_| ())?))
        };
        #[cfg(target_arch = "x86_64")]
        let regs = UnwindRegsX86_64::new(rip, rsp, rbp);
        #[cfg(target_arch = "aarch64")]
        let regs = UnwindRegsAarch64::new_with_ptr_auth_mask(
            registers
                .code_pac_mask
                .map_or(PtrAuthMask::new_no_strip(), |mask| PtrAuthMask(!mask)),
            registers.lr(),
            rsp,
            rbp,
        );
        let mut iter = self
            .unwinder
            .iter_frames(rip, regs, &mut self.cache, &mut read);
        let mut diagnostic = None;
        let reason = loop {
            match iter.next() {
                Ok(Some(frame)) => {
                    let address = frame.address();
                    let lookup = frame.address_for_lookup();
                    if cfg!(target_arch = "aarch64") && !address.is_multiple_of(4) {
                        if frames.is_empty() {
                            frames.push(StackFrame::raw(address));
                        }
                        break "unaligned AArch64 instruction address".into();
                    }
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
    use crate::http_server::analysis::test_support::process::{self, TestTarget};
    use object::{Object, ObjectSymbol};
    use process::procfs;

    // Capture actual native call-site state, exercising CFI without assuming a
    // compiler's prologue instructions or instruction-set-specific stack layout.
    #[test]
    fn omitted_frame_pointer_snapshot_reaches_main() {
        for name in [
            "recursive_no_fp",
            "recursive_no_fp_nopie",
            "recursive_debug_frame",
        ] {
            let target = TestTarget::new(name);
            let maps = process::maps::read_maps(target.id.pid).unwrap();
            let mut symbols = ElfCache::default();
            for map in maps.iter().filter(|map| map.executable && map.inode != 0) {
                let _ = process::load(&mut symbols, target.id.pid, map);
            }
            let map = maps
                .iter()
                .find(|m| m.executable && m.pathname.as_deref().is_some_and(|p| p.ends_with(name)))
                .unwrap();
            let elf = process::load(&mut symbols, target.id.pid, map).unwrap();
            let base = map.start - elf_address(map.start, map, &elf, procfs::page_size()).unwrap();
            let bytes = std::fs::read(format!("tests/targets/bin/{name}")).unwrap();
            let file = object::File::parse(bytes.as_slice()).unwrap();
            let expected: Vec<_> = ["baz", "bar", "foo", "main"]
                .map(|name| {
                    base + file
                        .symbols()
                        .find(|s| s.name().ok() == Some(name))
                        .unwrap()
                        .address()
                })
                .into();
            let (registers, stack) = process::capture_registers(target.id.pid, &maps).unwrap();
            for indexed in [true, false] {
                let mut state = StackUnwinder::default();
                state.refresh(&maps, &symbols);
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
                let (frames, stop) = state.walk(&registers, &maps, &stack);
                assert!(frames.len() >= 4, "{name}: {frames:?}, {stop}");
                for (frame, start) in frames.iter().zip(&expected) {
                    assert!(
                        frame.address >= *start && frame.address - start < 256,
                        "{name}: {frames:?}"
                    );
                }
                let (_, short_stop) = state.walk(&registers, &maps, &[]);
                assert!(
                    short_stop.contains("stack snapshot exhausted"),
                    "{short_stop}"
                );
            }
            let mut state = StackUnwinder::default();
            state.refresh(&maps, &symbols);
            assert!(!state.modules.is_empty());
            state.refresh(&[], &symbols);
            assert!(state.modules.is_empty());
            target.assert_detached();
        }
    }

    fn registers(pc: u64, sp: u64, fp: u64) -> RegisterSet {
        let mut r = RegisterSet::default();
        r.values[RegisterSet::PC] = pc;
        r.values[RegisterSet::SP] = sp;
        r.values[RegisterSet::FP] = fp;
        r
    }

    #[test]
    fn fallback_reports_module_failure_and_bounds_stack_reads() {
        let maps = vec![
            process::maps::parse_map("1000-2000 rw-p 0 00:00 0 [stack]").unwrap(),
            process::maps::parse_map("3000-4000 r-xp 0 00:00 0 /missing").unwrap(),
        ];
        let mut state = StackUnwinder::default();
        let (_, reason) = state.walk(&registers(0x3010, 0x1000, 0x1100), &maps, &[]);
        assert!(reason.contains("snapshot exhausted"), "{reason}");
        assert!(reason.contains("module lookup failure"), "{reason}");
        state.modules.push((0x3000, 0x4000, false));
        let (_, reason) = state.walk(&registers(0x3010, 0x1000, 0x1100), &maps, &[]);
        assert!(reason.contains("unwind metadata unavailable"), "{reason}");
        let (frames, reason) = state.walk(&registers(0x3010, 0x2000, 0), &maps, &[]);
        assert_eq!(frames.len(), 1);
        assert!(reason.contains("SP"));
    }
    #[cfg(target_arch = "aarch64")]
    #[test]
    fn kernel_pac_mask_strips_saved_return_addresses_without_guessing() {
        let maps = vec![
            process::maps::parse_map("1000-2000 rw-p 0 00:00 0 [stack]").unwrap(),
            process::maps::parse_map("3000-4000 r-xp 0 00:00 0 /missing").unwrap(),
        ];
        let mut r = registers(0x3010, 0x1000, 0x1010);
        let mut stack = vec![0; 64];
        stack[16..24].copy_from_slice(&0x1020u64.to_ne_bytes());
        stack[24..32].copy_from_slice(&0xab00_0000_0000_3020u64.to_ne_bytes());
        let (frames, reason) = StackUnwinder::default().walk(&r, &maps, &stack);
        assert_eq!(frames.len(), 1);
        assert!(reason.contains("outside executable"), "{reason}");
        r.code_pac_mask = Some(0xff00_0000_0000_0000);
        let (frames, reason) = StackUnwinder::default().walk(&r, &maps, &stack);
        assert_eq!(
            frames.iter().map(|f| f.address).collect::<Vec<_>>(),
            [0x3010, 0x3020],
            "{reason}"
        );
    }
}
