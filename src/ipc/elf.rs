//! Bounded ranges of a verified mapped file, transferred without descriptors.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`ElfFileId`] | `pub(crate)` | `type ElfFileId` |
//! | [`MAX_FILE_SIZE`] | `pub(crate)` | `const MAX_FILE_SIZE: u64` |
//! | [`MAX_READ_SIZE`] | `pub(crate)` | `const MAX_READ_SIZE: u64` |
//! | [`MAX_RANGES`] | `pub(crate)` | `const MAX_RANGES: usize` |
//! | [`FileRange`] | `pub(crate)` | `struct FileRange` |
//! | [`FileChunk`] | `pub(crate)` | `struct FileChunk` |
//! | [`MappedFileData`] | `pub(crate)` | `struct MappedFileData` |

/// Device, inode, length and nanosecond modification time of a verified file.
pub(crate) type ElfFileId = (u64, u64, u64, i64, i64);
pub(crate) const MAX_FILE_SIZE: u64 = 128 * 1024 * 1024;
pub(crate) const MAX_READ_SIZE: u64 = 64 * 1024 * 1024;
pub(crate) const MAX_RANGES: usize = 60;
#[derive(Clone, Copy, Debug)]
pub(crate) struct FileRange {
    pub(crate) offset: u64,
    pub(crate) length: u64,
}
pub(crate) struct FileChunk {
    pub(crate) offset: u64,
    pub(crate) bytes: Vec<u8>,
}
pub(crate) struct MappedFileData {
    pub(crate) identity: ElfFileId,
    pub(crate) chunks: Vec<FileChunk>,
}
crate::ipc::codec::wire_struct!(FileRange { offset, length });
crate::ipc::codec::wire_struct!(MappedFileData { identity, chunks });
impl crate::ipc::codec::Wire for FileChunk {
    fn write(&self, o: &mut crate::ipc::codec::Encoder) -> anyhow::Result<()> {
        self.offset.write(o)?;
        crate::ipc::codec::blob_encoder(&mut *o, &self.bytes)
    }
    fn read(i: &mut crate::ipc::codec::Decoder<'_>) -> anyhow::Result<Self> {
        let offset = crate::ipc::codec::Wire::read(i)?;
        let bytes = crate::ipc::codec::blob_decoder(&mut *i)?;
        anyhow::ensure!(
            bytes.len() as u64 <= MAX_READ_SIZE,
            "mapped file read exceeds limit"
        );
        Ok(Self {
            offset,
            bytes: bytes.to_vec(),
        })
    }
}
