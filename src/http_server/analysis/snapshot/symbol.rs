//! Unprivileged ELF parsing, caching, address conversion and source resolution.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`ElfImage`] | `pub` | `struct ElfImage` |
//! | [`ElfCache`] | `pub` | `struct ElfCache` |
//! | [`elf_address`] | `pub` | `fn elf_address(address: u64, map: &MemoryMap, elf: &ElfSymbols, page: u64) -> Option<u64>` |
//! | [`instruction_address`] | `pub` | `fn instruction_address(address: u64, return_address: bool) -> u64` |
//! | [`resolve_frame`] | `pub` | `fn resolve_frame(address: u64, elf: &ElfSymbols) -> Option<SymbolInfo>` |
//!
//! Lookup helpers use [`cache::ElfSymbols`] and [`resolve::SymbolInfo`].
//! Mapping arguments use [`MemoryMap`](crate::ipc::model::memory::MemoryMap).

mod cache;
mod image;
pub use image::ElfImage;
mod resolve;
pub use cache::ElfCache;
#[cfg(test)]
pub use cache::ElfFile;
#[cfg(test)]
pub use cache::ElfSymbols;
#[cfg(test)]
pub use resolve::SymbolInfo;
pub use resolve::{elf_address, instruction_address, resolve_frame};
