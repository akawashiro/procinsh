//! ELF acquisition and caching are separate from address conversion and resolution.
//!
//! Call [`ElfCache::get`] after the target resumes, release the cache lock, then
//! use [`elf_address`] and [`resolve_frame`]. Resolution never mutates its input
//! frame or the cache. addr2line may lazily load DWARF internally; that work is
//! serialized per ELF, not under the cache lock.
//!
//! # Interface
//!
//! Re-export visibility: `pub(super)`. Names link to definitions; **Source** opens their implementation.
//!
//! | Definition | Kind / signature |
//! | --- | --- |
//! | [`ElfCache`] | `struct ElfCache` |
//! | [`SymbolInfo`] | `struct SymbolInfo` |
//! | [`instruction_address`] | `fn instruction_address(address: u64, return_address: bool) -> u64` |
//! | [`elf_address`] | `fn elf_address(address: u64, map: &MemoryMap, elf: &ElfSymbols, page: u64) -> Option<u64>` |
//! | [`resolve_frame`] | `fn resolve_frame(address: u64, elf: &ElfSymbols) -> Option<SymbolInfo>` |
//!
//! Types: [`MemoryMap`](crate::http_server::process::maps::MemoryMap), [`ElfSymbols`](cache::ElfSymbols).

mod cache;
mod resolve;
pub(super) use cache::ElfCache;
pub(super) use resolve::{SymbolInfo, elf_address, instruction_address, resolve_frame};
