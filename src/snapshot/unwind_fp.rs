use crate::process::{maps::MemoryMap, memory};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct StackFrame {
    #[serde(serialize_with = "crate::process::maps::hex")]
    pub address: u64,
    pub symbol: Option<String>,
    pub symbol_offset: Option<String>,
    pub source_file: Option<String>,
    pub line: Option<u32>,
    pub inline_frames: Vec<SourceFrame>,
}
#[derive(Clone, Debug, Serialize)]
pub struct SourceFrame {
    pub function: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
}
impl StackFrame {
    pub fn raw(address: u64) -> Self {
        Self {
            address,
            symbol: None,
            symbol_offset: None,
            source_file: None,
            line: None,
            inline_frames: Vec::new(),
        }
    }
}

// The reader is injected so the same walker can later consume captured perf stack bytes.
pub fn walk(
    rip: u64,
    rsp: u64,
    rbp: u64,
    maps: &[MemoryMap],
    mut read: impl FnMut(u64) -> Option<[u8; 16]>,
) -> (Vec<StackFrame>, String) {
    let mut frames = vec![StackFrame::raw(rip)];
    let Some(stack) = maps.iter().find(|m| m.contains(rsp) && m.readable) else {
        return (frames, "RSP is outside readable mappings".into());
    };
    let mut bp = rbp;
    loop {
        if frames.len() >= 256 {
            return (frames, "256-frame limit".into());
        }
        if bp == 0 {
            return (frames, "end of frame-pointer chain".into());
        }
        if !bp.is_multiple_of(8)
            || bp < rsp
            || bp < stack.start
            || bp.checked_add(16).is_none_or(|end| end > stack.end)
        {
            return (
                frames,
                "RBP outside stack / unaligned (frame pointers may be omitted)".into(),
            );
        }
        let Some(data) = read(bp) else {
            return (frames, "stack memory could not be read".into());
        };
        let previous = u64::from_ne_bytes(data[..8].try_into().unwrap());
        let address = u64::from_ne_bytes(data[8..].try_into().unwrap());
        if address == 0 {
            return (frames, "end of frame-pointer chain".into());
        }
        if !maps
            .iter()
            .any(|m| m.executable && m.contains(address.saturating_sub(1)))
        {
            return (frames, "return address outside executable mappings".into());
        }
        frames.push(StackFrame::raw(address));
        if previous <= bp {
            return (frames, "end of chain / non-increasing RBP".into());
        }
        bp = previous;
    }
}

pub fn capture(
    pid: i32,
    r: &libc::user_regs_struct,
    maps: &[MemoryMap],
    deadline: std::time::Instant,
) -> (Vec<StackFrame>, String) {
    walk(r.rip, r.rsp, r.rbp, maps, |bp| {
        if std::time::Instant::now() >= deadline {
            return None;
        }
        memory::read_raw(pid, bp, 16).ok()?.try_into().ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn walk_stops_on_cycles_and_never_reads_outside_stack() {
        let maps = vec![
            crate::process::maps::parse_map("1000-2000 rw-p 0 00:00 0 [stack]").unwrap(),
            crate::process::maps::parse_map("3000-4000 r-xp 0 00:00 0 /a").unwrap(),
        ];
        let mut reads = 0;
        let (frames, reason) = walk(0x3010, 0x1000, 0x1100, &maps, |_| {
            reads += 1;
            let mut b = [0; 16];
            b[..8].copy_from_slice(&0x1100u64.to_ne_bytes());
            b[8..].copy_from_slice(&0x3020u64.to_ne_bytes());
            Some(b)
        });
        assert_eq!(frames.len(), 2);
        assert_eq!(reads, 1);
        assert!(reason.contains("non-increasing"));
        let (_, reason) = walk(0x3010, 0x1000, 0x1ff8, &maps, |_| {
            panic!("out of range read")
        });
        assert!(reason.contains("outside"));
    }
}
