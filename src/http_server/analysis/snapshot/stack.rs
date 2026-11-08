//! Shared stack frame values, independent of capture and symbol resolution.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`create_raw_stack_frame`] | `pub(crate)` | `fn create_raw_stack_frame(address: u64) -> `[`StackFrame`] |

use crate::ipc::stack::{SourceFrame, StackFrame};

#[derive(serde::Serialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::stack::StackFrame")]
struct StackFrameJson {
    #[serde(serialize_with = "crate::http_server::model::memory::hex")]
    address: u64,
    symbol: Option<String>,
    symbol_offset: Option<String>,
    source_file: Option<String>,
    line: Option<u32>,
    inline_frames: Vec<SourceFrame>,
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
    function: Option<String>,
    file: Option<String>,
    line: Option<u32>,
}
impl serde::Serialize for crate::ipc::stack::SourceFrame {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        SourceFrameJson::serialize(self, s)
    }
}

pub(crate) fn create_raw_stack_frame(address: u64) -> StackFrame {
    StackFrame {
        address,
        symbol: None,
        symbol_offset: None,
        source_file: None,
        line: None,
        inline_frames: Vec::new(),
    }
}
