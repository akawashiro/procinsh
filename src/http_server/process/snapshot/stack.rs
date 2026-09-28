//! Shared stack frame values, independent of capture and symbol resolution.

use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct StackFrame {
    #[serde(serialize_with = "crate::http_server::process::maps::hex")]
    pub(super) address: u64,
    pub(super) symbol: Option<String>,
    pub(super) symbol_offset: Option<String>,
    pub(super) source_file: Option<String>,
    pub(super) line: Option<u32>,
    pub(super) inline_frames: Vec<SourceFrame>,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct SourceFrame {
    pub(super) function: Option<String>,
    pub(super) file: Option<String>,
    pub(super) line: Option<u32>,
}
impl StackFrame {
    pub(super) fn raw(address: u64) -> Self {
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
