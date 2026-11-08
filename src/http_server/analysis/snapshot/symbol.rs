//! Unprivileged ELF parsing, caching, address conversion and source resolution.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`ElfImage`] | `pub(crate)` | `struct ElfImage` |
//! | [`create_elf_image`] | `pub(crate)` | `fn create_elf_image(data: `[`MappedFileData`](crate::ipc::elf::MappedFileData)`) -> `[`Result`](anyhow::Result)`<`[`ElfImage`]`>` |
//! | [`extend_elf_image`] | `pub(crate)` | `fn extend_elf_image(state: &mut `[`ElfImage`]`, data: `[`MappedFileData`](crate::ipc::elf::MappedFileData)`) -> `[`Result`](anyhow::Result)`<()>` |
//! | [`names_elf_image`] | `pub(crate)` | `fn names_elf_image(state: &`[`ElfImage`]`) -> `[`Result`](anyhow::Result)`<`[`Vec`]`<`[`FileRange`](crate::ipc::elf::FileRange)`>>` |
//! | [`sections_elf_image`] | `pub(crate)` | `fn sections_elf_image(state: &`[`ElfImage`]`) -> `[`Result`](anyhow::Result)`<`[`Vec`]`<`[`FileRange`](crate::ipc::elf::FileRange)`>>` |
//! | [`tables_elf_image`] | `pub(crate)` | `fn tables_elf_image(state: &`[`ElfImage`]`) -> `[`Result`](anyhow::Result)`<`[`Vec`]`<`[`FileRange`](crate::ipc::elf::FileRange)`>>` |
//! | [`ElfCache`] | `pub(crate)` | `struct ElfCache` |
//! | [`cache_elf_symbols_from_image`] | `pub(crate)` | `fn cache_elf_symbols_from_image( state: &mut `[`ElfCache`]`, map: &`[`MemoryMap`](crate::ipc::model::memory::MemoryMap)`, file: &`[`super::image::ElfImage`](image::ElfImage)`, ) -> `[`Option`]`<`[`Arc`](std::sync::Arc)`<`[`ElfSymbols`](cache::ElfSymbols)`>>` |
//! | [`lookup_cached_elf_file_identity`] | `pub(crate)` | `fn lookup_cached_elf_file_identity(state: &`[`ElfCache`]`, map: &`[`MemoryMap`](crate::ipc::model::memory::MemoryMap)`) -> `[`Option`]`<`[`ElfFileId`](crate::ipc::elf::ElfFileId)`>` |
//! | [`lookup_elf_symbols`] | `pub(crate)` | `fn lookup_elf_symbols(state: &`[`ElfCache`]`, map: &`[`MemoryMap`](crate::ipc::model::memory::MemoryMap)`) -> `[`Option`]`<`[`Arc`](std::sync::Arc)`<`[`ElfSymbols`](cache::ElfSymbols)`>>` |
//! | [`elf_address`] | `pub(crate)` | `fn elf_address(address: u64, map: &`[`MemoryMap`](crate::ipc::model::memory::MemoryMap)`, elf: &`[`ElfSymbols`](cache::ElfSymbols)`, page: u64) -> `[`Option`]`<u64>` |
//! | [`instruction_address`] | `pub(crate)` | `fn instruction_address(address: u64, return_address: bool) -> u64` |
//! | [`resolve_frame`] | `pub(crate)` | `fn resolve_frame(address: u64, elf: &`[`ElfSymbols`](cache::ElfSymbols)`) -> `[`Option`]`<`[`SymbolInfo`](resolve::SymbolInfo)`>` |

mod cache;
mod image;
pub(crate) use image::{
    ElfImage, create_elf_image, extend_elf_image, names_elf_image, sections_elf_image,
    tables_elf_image,
};
mod resolve;
#[cfg(test)]
pub(crate) use cache::ElfFile;
#[cfg(test)]
pub(crate) use cache::ElfSymbols;
pub(crate) use cache::{
    ElfCache, cache_elf_symbols_from_image, lookup_cached_elf_file_identity, lookup_elf_symbols,
};
#[cfg(test)]
pub(crate) use resolve::SymbolInfo;
pub(crate) use resolve::{elf_address, instruction_address, resolve_frame};

#[cfg(test)]
pub(crate) use cache::cache_elf_symbols_from_bytes;
