//! Reads the build kernel's BTF and emits the field offsets used by the sensors.
//!
//! # Interface
//!
//! - [`KernelLayoutReader`] (`pub(crate) struct`): validates raw BTF and resolves fields.
//!   Its parsing and generation methods are documented on the definition.
use anyhow::{Context, Result, bail, ensure};
use std::ops::Range;

/// Resolves the small set of kernel fields required by the BPF programs.
///
/// Only little-endian, version-1 BTF is accepted (procinsh targets Linux x86-64).
/// All record kinds are skipped by their specified length; unknown kinds, bad
/// references, bitfields and incompatible field types produce build errors.
/// No kernel C declarations or bindgen output are needed.
pub(crate) struct KernelLayoutReader<'a> {
    bytes: &'a [u8],
    strings: &'a [u8],
    types: Vec<TypeInfo>,
}
struct TypeInfo {
    name: u32,
    kind: u32,
    info: u32,
    size_or_type: u32,
    payload: Range<usize>,
}

fn word(bytes: &[u8], offset: usize) -> Result<u32> {
    let data = bytes
        .get(offset..offset + 4)
        .context("truncated BTF word")?;
    Ok(u32::from_le_bytes(data.try_into()?))
}

impl<'a> KernelLayoutReader<'a> {
    /// Parses and bounds-checks a raw `/sys/kernel/btf/vmlinux` image.
    pub(crate) fn parse(bytes: &'a [u8]) -> Result<Self> {
        ensure!(
            bytes.get(..4) == Some(&[0x9f, 0xeb, 1, 0]),
            "expected little-endian BTF v1"
        );
        let header = word(bytes, 4)? as usize;
        ensure!(header >= 24, "short BTF header");
        let section = |offset, len| -> Result<Range<usize>> {
            let start = header
                .checked_add(word(bytes, offset)? as usize)
                .context("BTF offset overflow")?;
            let end = start
                .checked_add(word(bytes, len)? as usize)
                .context("BTF length overflow")?;
            ensure!(end <= bytes.len(), "truncated BTF section");
            Ok(start..end)
        };
        let types_range = section(8, 12)?;
        let strings_range = section(16, 20)?;
        ensure!(
            types_range.end <= strings_range.start,
            "overlapping BTF sections"
        );
        let strings = &bytes[strings_range];
        ensure!(
            strings.first() == Some(&0) && strings.last() == Some(&0),
            "invalid BTF string table"
        );
        let mut types = Vec::new();
        let mut cursor = types_range.start;
        while cursor < types_range.end {
            ensure!(cursor + 12 <= types_range.end, "truncated BTF type");
            let info = word(bytes, cursor + 4)?;
            let kind = (info >> 24) & 0x1f;
            let count = (info & 0xffff) as usize;
            let len = match kind {
                1 | 14 | 17 => 4,
                3 => 12,
                4 | 5 | 15 | 19 => count * 12,
                6 | 13 => count * 8,
                2 | 7..=12 | 16 | 18 => 0,
                _ => bail!("unsupported BTF kind {kind}"),
            };
            let end = cursor + 12 + len;
            ensure!(end <= types_range.end, "truncated BTF payload");
            types.push(TypeInfo {
                name: word(bytes, cursor)?,
                kind,
                info,
                size_or_type: word(bytes, cursor + 8)?,
                payload: cursor + 12..end,
            });
            cursor = end;
        }
        let reader = Self {
            bytes,
            strings,
            types,
        };
        for ty in &reader.types {
            reader.name(ty.name)?;
        }
        Ok(reader)
    }

    fn name(&self, offset: u32) -> Result<&str> {
        let bytes = self
            .strings
            .get(offset as usize..)
            .context("invalid BTF string offset")?;
        let len = bytes
            .iter()
            .position(|&c| c == 0)
            .context("unterminated BTF name")?;
        Ok(std::str::from_utf8(&bytes[..len])?)
    }
    fn ty(&self, id: u32) -> Result<&TypeInfo> {
        self.types
            .get(id.checked_sub(1).context("unexpected void type")? as usize)
            .context("invalid BTF type id")
    }
    fn resolve(&self, mut id: u32) -> Result<&TypeInfo> {
        for _ in 0..32 {
            let ty = self.ty(id)?;
            if matches!(ty.kind, 8..=11 | 18) {
                id = ty.size_or_type;
            } else {
                return Ok(ty);
            }
        }
        bail!("cyclic BTF type modifiers")
    }
    fn structure(&self, name: &str) -> Result<&TypeInfo> {
        self.types
            .iter()
            .find(|ty| ty.kind == 4 && self.name(ty.name).ok() == Some(name))
            .with_context(|| format!("missing BTF struct {name}"))
    }
    fn member(&self, ty: &TypeInfo, name: &str, depth: usize) -> Result<Option<(u32, u32)>> {
        ensure!(depth < 32, "cyclic BTF anonymous members");
        ensure!(matches!(ty.kind, 4 | 5), "expected BTF struct/union");
        for pos in ty.payload.clone().step_by(12) {
            let member_name = self.name(word(self.bytes, pos)?)?;
            let id = word(self.bytes, pos + 4)?;
            let encoded = word(self.bytes, pos + 8)?;
            let (offset, bits) = if ty.info >> 31 != 0 {
                (encoded & 0xffffff, encoded >> 24)
            } else {
                (encoded, 0)
            };
            if member_name == name {
                ensure!(bits == 0 && offset % 8 == 0, "unsupported bitfield {name}");
                return Ok(Some((offset / 8, id)));
            }
            if member_name.is_empty() {
                let nested = self.resolve(id)?;
                if matches!(nested.kind, 4 | 5)
                    && let Some((inner, id)) = self.member(nested, name, depth + 1)?
                {
                    ensure!(offset % 8 == 0, "unaligned anonymous member");
                    return Ok(Some((
                        (offset / 8)
                            .checked_add(inner)
                            .context("BTF member offset overflow")?,
                        id,
                    )));
                }
            }
        }
        Ok(None)
    }
    fn field(&self, structure: &str, name: &str, expected: FieldType) -> Result<u32> {
        let owner = self.structure(structure)?;
        let (offset, id) = self
            .member(owner, name, 0)?
            .with_context(|| format!("missing BTF field {structure}.{name}"))?;
        let ty = self.resolve(id)?;
        let size = match expected {
            FieldType::Integer(size) => {
                ensure!(
                    ty.kind == 1 && ty.size_or_type == size,
                    "expected {size}-byte integer"
                );
                let encoding = word(self.bytes, ty.payload.start)?;
                ensure!(
                    encoding & 0xffffff == size * 8,
                    "unsupported integer encoding"
                );
                size
            }
            FieldType::Pointer(target) => {
                ensure!(ty.kind == 2, "expected pointer");
                let target_ty = self.resolve(ty.size_or_type)?;
                ensure!(
                    target_ty.kind == 4 && self.name(target_ty.name)? == target,
                    "expected pointer to struct {target}"
                );
                8
            }
            FieldType::Structure(target) => {
                ensure!(
                    ty.kind == 4 && self.name(ty.name)? == target,
                    "expected struct {target}"
                );
                ty.size_or_type
            }
        };
        ensure!(
            offset
                .checked_add(size)
                .is_some_and(|end| end <= owner.size_or_type),
            "field outside structure"
        );
        Ok(offset)
    }

    /// Emits constant byte offsets, validating field widths and pointer targets.
    pub(crate) fn generate(&self) -> Result<String> {
        use FieldType::*;
        let mut output = String::from("// Generated from the build kernel BTF. Do not edit.\n");
        for (structure, field, expected) in [
            ("task_struct", "group_leader", Pointer("task_struct")),
            ("task_struct", "start_boottime", Integer(8)),
            ("task_struct", "tgid", Integer(4)),
            ("task_struct", "pid", Integer(4)),
            ("task_struct", "flags", Integer(4)),
            ("file", "f_inode", Pointer("inode")),
            ("file", "f_path", Structure("path")),
            ("inode", "i_ino", Integer(8)),
            ("inode", "i_sb", Pointer("super_block")),
            ("inode", "i_mode", Integer(2)),
            ("inode", "i_generation", Integer(4)),
            ("super_block", "s_dev", Integer(4)),
            ("kiocb", "ki_filp", Pointer("file")),
            ("sock", "sk_socket", Pointer("socket")),
            ("socket", "file", Pointer("file")),
        ] {
            let offset = self
                .field(structure, field, expected)
                .with_context(|| format!("BTF field {structure}.{field}"))?;
            output.push_str(&format!(
                "pub const {}_{}: usize = {offset};\n",
                structure.to_uppercase(),
                field.to_uppercase()
            ));
        }
        Ok(output)
    }
}
enum FieldType {
    Integer(u32),
    Pointer(&'static str),
    Structure(&'static str),
}

#[cfg(test)]
mod tests {
    use super::*;
    fn image(types: &[u32], strings: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0x9f, 0xeb, 1, 0];
        for n in [
            24,
            0,
            types.len() as u32 * 4,
            types.len() as u32 * 4,
            strings.len() as u32,
        ] {
            bytes.extend(n.to_le_bytes());
        }
        for n in types {
            bytes.extend(n.to_le_bytes());
        }
        bytes.extend(strings);
        bytes
    }
    #[test]
    fn resolves_modified_types_and_anonymous_union_members() {
        // int -> typedef -> const; an anonymous union inside struct owner.
        let bytes = image(
            &[
                1,
                1 << 24,
                4,
                32,
                0,
                8 << 24,
                1,
                0,
                10 << 24,
                2,
                0,
                (5 << 24) | 1,
                4,
                5,
                3,
                0,
                11,
                (4 << 24) | 1,
                16,
                0,
                4,
                64,
            ],
            b"\0int\0value\0owner\0",
        );
        let reader = KernelLayoutReader::parse(&bytes).unwrap();
        assert_eq!(
            reader
                .field("owner", "value", FieldType::Integer(4))
                .unwrap(),
            8
        );
        assert!(
            reader
                .field("owner", "value", FieldType::Integer(8))
                .is_err()
        );
        assert!(
            reader
                .field("owner", "missing", FieldType::Integer(4))
                .is_err()
        );
    }
    #[test]
    fn rejects_bitfields_bad_references_and_truncated_images() {
        let bytes = image(
            &[
                1,
                1 << 24,
                4,
                32,
                11,
                (1 << 31) | (4 << 24) | 1,
                4,
                5,
                1,
                3 << 24,
            ],
            b"\0int\0value\0owner\0",
        );
        let reader = KernelLayoutReader::parse(&bytes).unwrap();
        assert!(
            reader
                .field("owner", "value", FieldType::Integer(4))
                .is_err()
        );
        assert!(reader.resolve(99).is_err());
        for len in 0..bytes.len() {
            assert!(KernelLayoutReader::parse(&bytes[..len]).is_err());
        }
    }
    #[test]
    fn validates_pointer_targets_and_rejects_modifier_cycles() {
        let bytes = image(
            &[
                1,
                1 << 24,
                4,
                32,
                5,
                4 << 24,
                16,
                0,
                10 << 24,
                2,
                0,
                2 << 24,
                3,
                11,
                (4 << 24) | 1,
                8,
                16,
                4,
                0,
            ],
            b"\0int\0inode\0file\0f_inode\0",
        );
        let reader = KernelLayoutReader::parse(&bytes).unwrap();
        assert_eq!(
            reader
                .field("file", "f_inode", FieldType::Pointer("inode"))
                .unwrap(),
            0
        );
        assert!(
            reader
                .field("file", "f_inode", FieldType::Pointer("file"))
                .is_err()
        );
        assert!(
            reader
                .field("file", "f_inode", FieldType::Integer(8))
                .is_err()
        );
        let bytes = image(&[0, 8 << 24, 1], b"\0");
        assert!(
            KernelLayoutReader::parse(&bytes)
                .unwrap()
                .resolve(1)
                .is_err()
        );
    }
    #[test]
    fn running_kernel_has_required_fields() {
        let bytes = std::fs::read("/sys/kernel/btf/vmlinux").unwrap();
        assert!(
            KernelLayoutReader::parse(&bytes)
                .unwrap()
                .generate()
                .unwrap()
                .contains("FILE_F_PATH")
        );
    }
}
