//! Bounded ranges of a verified mapped file, transferred without descriptors.

//!
//! # Interface
//! - [`ElfFileId`] (`pub`): `type ElfFileId = (u64,u64,u64,i64,i64)`.
//! - [`FileRange`] (`pub`): `struct FileRange`.
//! - [`FileChunk`] (`pub`): `struct FileChunk`.
//! - [`MappedFileData`] (`pub`): `struct MappedFileData`.
//! - [`MAX_FILE_SIZE`] (`pub`): `const MAX_FILE_SIZE: u64`.
//! - [`MAX_READ_SIZE`] (`pub`): `const MAX_READ_SIZE: u64`.
//! - [`MAX_RANGES`] (`pub`): `const MAX_RANGES: usize`.
pub type ElfFileId = (u64, u64, u64, i64, i64);
pub const MAX_FILE_SIZE: u64 = 128 * 1024 * 1024;
pub const MAX_READ_SIZE: u64 = 64 * 1024 * 1024;
pub const MAX_RANGES: usize = 60;
#[derive(Clone, Copy, Debug)]
pub struct FileRange {
    pub offset: u64,
    pub length: u64,
}
pub struct FileChunk {
    pub offset: u64,
    pub bytes: Vec<u8>,
}
pub struct MappedFileData {
    pub identity: ElfFileId,
    pub chunks: Vec<FileChunk>,
}
crate::ipc::codec::wire_struct!(FileRange { offset, length });
crate::ipc::codec::wire_struct!(MappedFileData { identity, chunks });
impl crate::ipc::codec::Wire for FileChunk {
    fn write(&self, o: &mut crate::ipc::codec::Encoder) -> anyhow::Result<()> {
        self.offset.write(o)?;
        o.blob(&self.bytes)
    }
    fn read(i: &mut crate::ipc::codec::Decoder<'_>) -> anyhow::Result<Self> {
        let offset = crate::ipc::codec::Wire::read(i)?;
        let bytes = i.blob()?;
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
