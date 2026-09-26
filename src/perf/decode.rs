use crate::inspect::registers;
use anyhow::{Result, ensure};
pub const CALLCHAIN: u64 = 1 << 5;
pub const REGS: u64 = 1 << 12;
#[derive(Clone, Copy, Debug)]
pub struct Format {
    pub sample_type: u64,
    pub regs_mask: u64,
}
impl Format {
    pub fn ip() -> Self {
        Self {
            sample_type: 1 | 2 | 4 | 128,
            regs_mask: 0,
        }
    }
    pub fn registers() -> Self {
        Self {
            sample_type: Self::ip().sample_type | REGS,
            regs_mask: registers::MASK,
        }
    }
    pub fn full() -> Self {
        Self {
            sample_type: Self::registers().sample_type | CALLCHAIN,
            ..Self::registers()
        }
    }
}
#[derive(Debug)]
pub struct Sample {
    pub ip: u64,
    pub tid: i32,
    pub time: u64,
    pub cpu: u32,
    pub regs: Vec<(u32, u64)>,
    pub frames: Vec<u64>,
    pub abi: Option<u64>,
}
#[derive(Debug)]
pub enum Record {
    Sample(Sample),
    Lost(u64),
    Throttle(bool),
    Exit(i32),
    Exec,
    Other,
}
struct Cursor<'a>(&'a [u8]);
impl Cursor<'_> {
    fn u64(&mut self) -> Result<u64> {
        ensure!(self.0.len() >= 8, "Truncated perf field");
        let value = u64::from_ne_bytes(self.0[..8].try_into().unwrap());
        self.0 = &self.0[8..];
        Ok(value)
    }
}
pub fn decode(bytes: &[u8], format: Format) -> Result<Record> {
    ensure!(bytes.len() >= 8, "Truncated header");
    let kind = u32::from_ne_bytes(bytes[..4].try_into().unwrap());
    let misc = u16::from_ne_bytes(bytes[4..6].try_into().unwrap());
    ensure!(
        usize::from(u16::from_ne_bytes(bytes[6..8].try_into().unwrap())) == bytes.len(),
        "Record length mismatch"
    );
    let mut c = Cursor(&bytes[8..]);
    Ok(match kind {
        9 => {
            let ip = c.u64()?;
            let ids = c.u64()?;
            let time = c.u64()?;
            // CPU precedes CALLCHAIN in the UAPI wire layout (not bit order).
            let cpu = c.u64()? as u32;
            let mut frames = Vec::new();
            if format.sample_type & CALLCHAIN != 0 {
                let count = c.u64()?;
                ensure!(count <= (c.0.len() / 8) as u64, "Invalid callchain length");
                let mut user = false;
                for _ in 0..count {
                    let address = c.u64()?;
                    if address >= (-4095i64) as u64 {
                        user = address == (-512i64) as u64;
                    } else if user && frames.len() < 256 {
                        frames.push(address);
                    }
                }
            }
            let mut regs = Vec::new();
            let mut abi = None;
            if format.sample_type & REGS != 0 {
                let a = c.u64()?;
                abi = Some(a);
                ensure!(a <= 2, "Unknown perf register ABI {a}");
                if a != 0 {
                    for bit in 0..64 {
                        if format.regs_mask & (1u64 << bit) != 0 {
                            let value = c.u64()?;
                            if a == 2 {
                                regs.push((bit, value));
                            }
                        }
                    }
                }
            }
            Record::Sample(Sample {
                ip,
                tid: (ids >> 32) as i32,
                time,
                cpu,
                regs,
                frames,
                abi,
            })
        }
        2 => {
            c.u64()?;
            Record::Lost(c.u64()?)
        }
        13 => Record::Lost(c.u64()?),
        5 => Record::Throttle(true),
        6 => Record::Throttle(false),
        4 => {
            ensure!(c.0.len() >= 24, "Truncated EXIT");
            let _processes = c.u64()?;
            let tids = c.u64()?;
            Record::Exit(tids as i32)
        }
        3 if misc & (1 << 13) != 0 => Record::Exec,
        _ => Record::Other,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn record(kind: u32, fields: &[u64]) -> Vec<u8> {
        let mut b = Vec::from(kind.to_ne_bytes());
        b.extend(0u16.to_ne_bytes());
        b.extend(((8 + fields.len() * 8) as u16).to_ne_bytes());
        for v in fields {
            b.extend(v.to_ne_bytes());
        }
        b
    }
    #[test]
    fn formats_and_variable_fields() {
        let b = record(
            9,
            &[
                0x1234,
                7 << 32 | 7,
                123,
                2,
                3,
                (-512i64) as u64,
                0x1234,
                0x5678,
                2,
                0x99,
            ],
        );
        let f = Format {
            sample_type: Format::full().sample_type,
            regs_mask: 1 << 8,
        };
        let Record::Sample(s) = decode(&b, f).unwrap() else {
            panic!()
        };
        assert_eq!(s.frames, [0x1234, 0x5678]);
        assert_eq!(s.regs, [(8, 0x99)]);
        assert_eq!(s.cpu, 2);
        assert!(decode(&b[..b.len() - 1], f).is_err());
        assert!(matches!(
            decode(&record(2, &[0, 42]), f).unwrap(),
            Record::Lost(42)
        ));
        assert!(matches!(
            decode(&record(999, &[]), f).unwrap(),
            Record::Other
        ));
        let Record::Sample(s) =
            decode(&record(9, &[1, 1 << 32, 2, 3, 0]), Format::registers()).unwrap()
        else {
            panic!()
        };
        assert!(s.regs.is_empty());
        assert_eq!(s.abi, Some(0));
        assert!(decode(&record(9, &[1, 0, 2, 999]), Format::full()).is_err());
    }
    #[test]
    fn abi_and_context_boundaries_are_not_fabricated() {
        let f = Format {
            sample_type: Format::registers().sample_type,
            regs_mask: (1 << 8) | (1 << 23),
        };
        for abi in [0, 1, 2] {
            let mut fields = vec![0x1234, 11 << 32 | 11, 9_007_199_254_740_993, 7, abi];
            if abi != 0 {
                fields.extend([0xabcdef, 99]);
            }
            let Record::Sample(s) = decode(&record(9, &fields), f).unwrap() else {
                panic!()
            };
            assert_eq!(s.time, 9_007_199_254_740_993);
            assert_eq!(s.regs.len(), if abi == 2 { 2 } else { 0 });
            if abi == 2 {
                assert_eq!(s.regs, [(8, 0xabcdef), (23, 99)]);
            }
        }
        assert!(decode(&record(9, &[0, 0, 0, 0, 3]), f).is_err());
        let f = Format {
            sample_type: Format::ip().sample_type | CALLCHAIN,
            regs_mask: 0,
        };
        let Record::Sample(s) = decode(
            &record(
                9,
                &[
                    1,
                    0,
                    0,
                    2,
                    6,
                    (-128i64) as u64,
                    0xffff0000,
                    (-512i64) as u64,
                    0x1234,
                    (-2048i64) as u64,
                    0x5678,
                ],
            ),
            f,
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(s.frames, [0x1234]);
    }
}
