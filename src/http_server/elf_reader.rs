//! Acquire only ELF ranges selected by main, preserving one verified file identity.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`acquire_mapped_elf_image`] | `pub(super)` | `async fn acquire_mapped_elf_image( client: &`[`MonitorClient`]`, id: `[`ProcessId`]`, mapping_start: u64, known: `[`Option`]`<`[`ElfFileId`]`>, ) -> `[`Result`]`<`[`Option`]`<`[`ElfImage`]`>>` |
//!

use super::client::MonitorClient;
use crate::{
    http_server::analysis::snapshot::symbol::ElfImage,
    ipc::{
        elf::{ElfFileId, FileRange, MAX_RANGES, MappedFileData},
        schema::{MonitorRequest, MonitorResponse, ProcessId},
    },
};
use anyhow::{Result, bail};

async fn request_mapped_file_ranges(
    client: &MonitorClient,
    id: ProcessId,
    mapping_start: u64,
    ranges: Vec<FileRange>,
) -> Result<Option<MappedFileData>> {
    use crate::http_server::client::exchange_monitor_client;

    match exchange_monitor_client(
        client,
        MonitorRequest::MappedElf {
            id,
            mapping_start,
            ranges,
        },
    )
    .await?
    {
        MonitorResponse::Elf(data) => Ok(data),
        _ => bail!("unexpected mapped file response"),
    }
}

pub(super) async fn acquire_mapped_elf_image(
    client: &MonitorClient,
    id: ProcessId,
    mapping_start: u64,
    known: Option<ElfFileId>,
) -> Result<Option<ElfImage>> {
    use crate::http_server::analysis::snapshot::symbol::{
        create_elf_image, extend_elf_image, names_elf_image, sections_elf_image, tables_elf_image,
    };

    let Some(header) = request_mapped_file_ranges(
        client,
        id,
        mapping_start,
        vec![FileRange {
            offset: 0,
            length: 64,
        }],
    )
    .await?
    else {
        return Ok(None);
    };
    if Some(header.identity) == known {
        return Ok(None);
    }
    let mut image = create_elf_image(header)?;
    for stage in 0..3 {
        let ranges = match stage {
            0 => tables_elf_image(&image)?,
            1 => names_elf_image(&image)?,
            _ => sections_elf_image(&image)?,
        };
        for ranges in ranges.chunks(MAX_RANGES) {
            let Some(data) =
                request_mapped_file_ranges(client, id, mapping_start, ranges.to_vec()).await?
            else {
                return Ok(None);
            };
            extend_elf_image(&mut image, data)?;
        }
    }
    Ok(Some(image))
}
