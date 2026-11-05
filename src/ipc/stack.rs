//! Stack address and resolved frame data.

//!
//! # Interface
//! - [`SourceFrame`] (`pub`): `struct SourceFrame`.
//! - [`StackFrame`] (`pub`): `struct StackFrame`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFrame {
    pub function: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackFrame {
    pub address: u64,
    pub symbol: Option<String>,
    pub symbol_offset: Option<String>,
    pub source_file: Option<String>,
    pub line: Option<u32>,
    pub inline_frames: Vec<SourceFrame>,
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
