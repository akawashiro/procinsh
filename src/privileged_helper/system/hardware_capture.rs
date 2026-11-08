//! Bounded reads of fixed hardware attributes, without interpreting their values.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`collect_hardware_files`] | `pub(crate)` | `fn collect_hardware_files(sys: &`[`Path`]`, proc: &`[`Path`]`) -> `[`HardwareFiles`] |
//!
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
fn reserve_hardware_reader(state: &mut HardwareReader, bytes: usize) -> bool {
    if state.bytes.saturating_add(bytes) > MAX_BYTES {
        state.files.truncated = true;
        return false;
    }
    state.bytes += bytes;
    true
}

fn text_hardware_reader(state: &mut HardwareReader, path: &Path) {
    if state.bytes >= MAX_BYTES {
        return;
    }
    let Ok(file) = fs::File::open(path) else {
        return;
    };
    let mut value = String::new();
    if file
        .take(MAX_TEXT.min((MAX_BYTES - state.bytes) as u64))
        .read_to_string(&mut value)
        .is_ok()
    {
        let path = path.to_string_lossy().into_owned();
        if reserve_hardware_reader(state, path.len() + value.len()) {
            state.files.texts.push((path, value));
        }
    }
}

fn names_hardware_reader(state: &mut HardwareReader, path: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(path) else {
        return Vec::new();
    };
    let key = path.to_string_lossy().into_owned();
    if !reserve_hardware_reader(state, key.len()) {
        return Vec::new();
    }
    let mut names = Vec::new();
    for entry in entries.take(MAX_ENTRIES) {
        let Ok(entry) = entry else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if !reserve_hardware_reader(state, name.len()) {
            break;
        }
        names.push(name);
    }
    names.sort();
    state.files.directories.push((key, names.clone()));
    names
}

fn link_hardware_reader(state: &mut HardwareReader, path: &Path) -> Option<std::path::PathBuf> {
    let target = fs::canonicalize(path).ok()?;
    let key = path.to_string_lossy().into_owned();
    let value = target.to_string_lossy().into_owned();
    if !reserve_hardware_reader(state, key.len() + value.len()) {
        return None;
    }
    state.files.links.push((key, value));
    Some(target)
}

fn node_hardware_reader(state: &mut HardwareReader, path: &Path) {
    let runtime = path
        .join("power/runtime_status")
        .to_string_lossy()
        .into_owned();
    text_hardware_reader(state, Path::new(&runtime));
    let suspended = state
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
        text_hardware_reader(state, &path.join(attribute));
    }
    if !suspended {
        for attribute in [
            "current_link_width",
            "current_link_speed",
            "max_link_width",
            "max_link_speed",
        ] {
            text_hardware_reader(state, &path.join(attribute));
        }
    }
    for name in ["driver", "iommu_group"] {
        link_hardware_reader(state, &path.join(name));
    }
    for name in ["drm", "net"] {
        names_hardware_reader(state, &path.join(name));
    }
    for name in names_hardware_reader(state, &path.join("infiniband")) {
        for attribute in ["hca_type", "fw_ver"] {
            text_hardware_reader(state, &path.join("infiniband").join(&name).join(attribute));
        }
    }
}

/// Reads only known read-only attributes; never opens PCI config or BAR memory.
pub(crate) fn collect_hardware_files(sys: &Path, proc: &Path) -> HardwareFiles {
    let mut reader = HardwareReader::default();
    text_hardware_reader(&mut reader, &sys.join("devices/system/node/online"));
    text_hardware_reader(&mut reader, &proc.join("devices"));
    let mut seen = HashSet::new();
    let devices = sys.join("bus/pci/devices");
    for address in names_hardware_reader(&mut reader, &devices) {
        if reader.files.truncated {
            break;
        }
        let Some(path) = link_hardware_reader(&mut reader, &devices.join(&address)) else {
            continue;
        };
        if !path.starts_with(sys) {
            continue;
        }
        text_hardware_reader(
            &mut reader,
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
                node_hardware_reader(&mut reader, node);
            }
        }
    }
    let characters = sys.join("dev/char");
    for name in names_hardware_reader(&mut reader, &characters) {
        link_hardware_reader(&mut reader, &characters.join(name));
    }
    reader.files
}
