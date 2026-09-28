//! Shared stack frame values, independent of capture and symbol resolution.

use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct StackFrame {
    #[serde(serialize_with = "crate::process::maps::hex")]
    pub address: u64,
    pub symbol: Option<String>,
    pub symbol_offset: Option<String>,
    pub source_file: Option<String>,
    pub line: Option<u32>,
    pub inline_frames: Vec<SourceFrame>,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
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
