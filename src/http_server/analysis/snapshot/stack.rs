//! Shared stack frame values, independent of capture and symbol resolution.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`create_raw_stack_frame`] | `pub` | `fn create_raw_stack_frame(address: u64) -> `[`StackFrame`] |

use crate::ipc::stack::{SourceFrame, StackFrame};

#[derive(serde::Serialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::stack::StackFrame")]
struct StackFrameJson {
    #[serde(serialize_with = "crate::http_server::model::memory::hex")]
    pub address: u64,
    pub symbol: Option<String>,
    pub symbol_offset: Option<String>,
    pub source_file: Option<String>,
    pub line: Option<u32>,
    pub inline_frames: Vec<SourceFrame>,
}
impl serde::Serialize for crate::ipc::stack::StackFrame {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        StackFrameJson::serialize(self, s)
    }
}

#[derive(serde::Serialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::stack::SourceFrame")]
struct SourceFrameJson {
    pub function: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
}
impl serde::Serialize for crate::ipc::stack::SourceFrame {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        SourceFrameJson::serialize(self, s)
    }
}

pub fn create_raw_stack_frame(address: u64) -> StackFrame {
    StackFrame {
        address,
        symbol: None,
        symbol_offset: None,
        source_file: None,
        line: None,
        inline_frames: Vec::new(),
    }
}
