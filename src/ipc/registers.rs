//! Register values shared with native acquisition.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`RegisterSet`] | `pub` | `struct RegisterSet` |
//! | [`RegisterRole`] | `pub` | `enum RegisterRole` |
//! | [`Register`] | `pub` | `struct Register` |
//! | [`RegisterMapping`] | `pub` | `struct RegisterMapping` |
//! | [`validate_register_set`] | `pub` | `fn validate_register_set(state: &`[`RegisterSet`]`) -> `[`anyhow::Result`]`<()>` |
//! | [`pc_register_set`] | `pub` | `fn pc_register_set(state: &`[`RegisterSet`]`) -> u64` |
//! | [`sp_register_set`] | `pub` | `fn sp_register_set(state: &`[`RegisterSet`]`) -> u64` |
//! | [`fp_register_set`] | `pub` | `fn fp_register_set(state: &`[`RegisterSet`]`) -> u64` |
//! | [`lr_register_set`] | `pub` | `fn lr_register_set(state: &`[`RegisterSet`]`) -> u64` |

use crate::ipc::model::memory::MemoryKind;

/// Native Linux PERF_REG_* values. Ptrace also supplies ARM64 PSTATE at
/// index 33; perf never does. The helper and HTTP process use the same build.
/// PAC metadata describes the target thread and may be retained across samples;
/// register values themselves always come from one sample.
#[derive(Clone, Debug)]
pub struct RegisterSet {
    pub values: Vec<u64>,
    pub flags_available: bool,
    /// Bits occupied by instruction-pointer authentication, as reported by Linux.
    pub code_pac_mask: Option<u64>,
}

impl RegisterSet {
    pub const COUNT: usize = if cfg!(target_arch = "aarch64") {
        34
    } else {
        24
    };
    pub const PC: usize = if cfg!(target_arch = "aarch64") { 32 } else { 8 };
    pub const SP: usize = if cfg!(target_arch = "aarch64") { 31 } else { 7 };
    pub const FP: usize = if cfg!(target_arch = "aarch64") { 29 } else { 6 };
}

pub fn validate_register_set(state: &RegisterSet) -> anyhow::Result<()> {
    anyhow::ensure!(
        state.values.len() == RegisterSet::COUNT,
        "Invalid native register count"
    );
    Ok(())
}

pub fn pc_register_set(state: &RegisterSet) -> u64 {
    state.values[RegisterSet::PC]
}

pub fn sp_register_set(state: &RegisterSet) -> u64 {
    state.values[RegisterSet::SP]
}

pub fn fp_register_set(state: &RegisterSet) -> u64 {
    state.values[RegisterSet::FP]
}

pub fn lr_register_set(state: &RegisterSet) -> u64 {
    if cfg!(target_arch = "aarch64") {
        state.values[30]
    } else {
        0
    }
}

impl Default for RegisterSet {
    fn default() -> Self {
        Self {
            values: vec![0; Self::COUNT],
            flags_available: false,
            code_pac_mask: None,
        }
    }
}

/// Architecture-independent register purpose, separate from its memory mapping kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterRole {
    Pc,
    Sp,
    Fp,
    Lr,
    Flags,
    General,
}
crate::ipc::codec::wire_enum!(RegisterRole{0=>Pc,1=>Sp,2=>Fp,3=>Lr,4=>Flags,5=>General});

#[derive(Clone, Debug)]
pub struct Register {
    pub name: String,
    pub role: RegisterRole,
    pub value: u64,
    pub decimal: String,
    pub kind: MemoryKind,
    pub mapping: Option<RegisterMapping>,
    pub offset: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct RegisterMapping {
    pub pathname: Option<String>,
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
    pub private: bool,
}

impl crate::ipc::codec::Wire for RegisterSet {
    fn write(&self, o: &mut crate::ipc::codec::Encoder) -> anyhow::Result<()> {
        validate_register_set(self)?;
        self.values.write(o)?;
        self.flags_available.write(o)?;
        self.code_pac_mask.write(o)
    }
    fn read(i: &mut crate::ipc::codec::Decoder<'_>) -> anyhow::Result<Self> {
        let result = Self {
            values: crate::ipc::codec::Wire::read(i)?,
            flags_available: crate::ipc::codec::Wire::read(i)?,
            code_pac_mask: crate::ipc::codec::Wire::read(i)?,
        };
        validate_register_set(&result)?;
        Ok(result)
    }
}

crate::ipc::codec::wire_struct!(Register {
    name,
    role,
    value,
    decimal,
    kind,
    mapping,
    offset
});

crate::ipc::codec::wire_struct!(RegisterMapping {
    pathname,
    readable,
    writable,
    executable,
    private
});
