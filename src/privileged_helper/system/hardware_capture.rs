//! Bounded reads of fixed hardware attributes, without interpreting their values.
//!
//! # Interface
//! [`collect_hardware_files`] (`pub(crate)`): `fn collect_hardware_files(sys: &Path, proc: &Path) -> HardwareFiles`.
//! [`HardwareFiles`] contains only owned text, directory names and resolved paths.
use crate::ipc::hardware::HardwareFiles;
use std::{collections::HashSet, fs, io::Read, path::Path};

const MAX_ENTRIES: usize = 4096;
const MAX_TEXT: u64 = 65536;
const MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Default)]
struct HardwareReader {
    files: HardwareFiles,
    bytes: usize,
}
impl HardwareReader {
    fn reserve(&mut self, bytes: usize) -> bool {
        if self.bytes.saturating_add(bytes) > MAX_BYTES {
            self.files.truncated = true;
            return false;
        }
        self.bytes += bytes;
        true
    }
    fn text(&mut self, path: &Path) {
        if self.bytes >= MAX_BYTES {
            return;
        }
        let Ok(file) = fs::File::open(path) else {
            return;
        };
        let mut value = String::new();
        if file
            .take(MAX_TEXT.min((MAX_BYTES - self.bytes) as u64))
            .read_to_string(&mut value)
            .is_ok()
        {
            let path = path.to_string_lossy().into_owned();
            if self.reserve(path.len() + value.len()) {
                self.files.texts.push((path, value));
            }
        }
    }
    fn names(&mut self, path: &Path) -> Vec<String> {
        let Ok(entries) = fs::read_dir(path) else {
            return Vec::new();
        };
        let key = path.to_string_lossy().into_owned();
        if !self.reserve(key.len()) {
            return Vec::new();
        }
        let mut names = Vec::new();
        for entry in entries.take(MAX_ENTRIES) {
            let Ok(entry) = entry else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            if !self.reserve(name.len()) {
                break;
            }
            names.push(name);
        }
        names.sort();
        self.files.directories.push((key, names.clone()));
        names
    }
    fn link(&mut self, path: &Path) -> Option<std::path::PathBuf> {
        let target = fs::canonicalize(path).ok()?;
        let key = path.to_string_lossy().into_owned();
        let value = target.to_string_lossy().into_owned();
        if !self.reserve(key.len() + value.len()) {
            return None;
        }
        self.files.links.push((key, value));
        Some(target)
    }
    fn node(&mut self, path: &Path) {
        let runtime = path
            .join("power/runtime_status")
            .to_string_lossy()
            .into_owned();
        self.text(Path::new(&runtime));
        let suspended = self
            .files
            .texts
            .last()
            .is_some_and(|(p, v)| p == &runtime && v.trim() == "suspended");
        for attribute in [
            "class",
            "vendor",
            "device",
            "numa_node",
            "power_state",
            "resource",
        ] {
            self.text(&path.join(attribute));
        }
        if !suspended {
            for attribute in [
                "current_link_width",
                "current_link_speed",
                "max_link_width",
                "max_link_speed",
            ] {
                self.text(&path.join(attribute));
            }
        }
        for name in ["driver", "iommu_group"] {
            self.link(&path.join(name));
        }
        for name in ["drm", "net"] {
            self.names(&path.join(name));
        }
        for name in self.names(&path.join("infiniband")) {
            for attribute in ["hca_type", "fw_ver"] {
                self.text(&path.join("infiniband").join(&name).join(attribute));
            }
        }
    }
}

/// Reads only known read-only attributes; never opens PCI config or BAR memory.
pub(crate) fn collect_hardware_files(sys: &Path, proc: &Path) -> HardwareFiles {
    let mut reader = HardwareReader::default();
    reader.text(&sys.join("devices/system/node/online"));
    reader.text(&proc.join("devices"));
    let mut seen = HashSet::new();
    let devices = sys.join("bus/pci/devices");
    for address in reader.names(&devices) {
        if reader.files.truncated {
            break;
        }
        let Some(path) = reader.link(&devices.join(&address)) else {
            continue;
        };
        if !path.starts_with(sys) {
            continue;
        }
        reader.text(
            &proc
                .join("driver/nvidia/gpus")
                .join(&address)
                .join("information"),
        );
        for node in path.ancestors().take(32) {
            if reader.files.truncated {
                break;
            }
            if !node.starts_with(sys) || node == sys || seen.len() >= MAX_ENTRIES {
                break;
            }
            if seen.insert(node.to_owned()) {
                reader.node(node);
            }
        }
    }
    let characters = sys.join("dev/char");
    for name in reader.names(&characters) {
        reader.link(&characters.join(name));
    }
    reader.files
}
