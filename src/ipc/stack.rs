//! Stack address and resolved frame data.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`SourceFrame`] | `pub(crate)` | `struct SourceFrame` |
//! | [`StackFrame`] | `pub(crate)` | `struct StackFrame` |

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceFrame {
    pub(crate) function: Option<String>,
    pub(crate) file: Option<String>,
    pub(crate) line: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StackFrame {
    pub(crate) address: u64,
    pub(crate) symbol: Option<String>,
    pub(crate) symbol_offset: Option<String>,
    pub(crate) source_file: Option<String>,
    pub(crate) line: Option<u32>,
    pub(crate) inline_frames: Vec<SourceFrame>,
}

crate::ipc::codec::wire_struct!(SourceFrame {
    function,
    file,
    line
});

crate::ipc::codec::wire_struct!(StackFrame {
    address,
    symbol,
    symbol_offset,
    source_file,
    line,
    inline_frames
});
