//! ELF acquisition and caching are separate from address conversion and resolution.
//!
//! Call [`ElfCache::get`] after the target resumes, release the cache lock, then
//! use [`elf_address`] and [`resolve_frame`]. Resolution never mutates its input
//! frame or the cache. addr2line may lazily load DWARF internally; that work is
//! serialized per ELF, not under the cache lock.

mod cache;
mod resolve;
pub(super) use cache::ElfCache;
pub(super) use resolve::{SymbolInfo, elf_address, instruction_address, resolve_frame};
