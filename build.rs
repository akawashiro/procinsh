use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
#[path = "src/bpf_build/kernel_layout.rs"]
mod kernel_layout;

fn setting(text: &str, name: &str) -> String {
    text.lines()
        .find_map(|line| {
            line.strip_prefix(&format!("{name} = \""))?
                .strip_suffix('"')
        })
        .unwrap()
        .to_owned()
}
fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    for asset in ["app.js", "space.js", "space-model.js", "display.js"] {
        let path = root.join("dist/web").join(asset);
        println!("cargo:rerun-if-changed={}", path.display());
        assert!(
            path.is_file(),
            "Missing web asset {}. Run `npm ci && npm run build:web` before building with Cargo.",
            path.display()
        );
    }
    for path in [
        "src/ebpf",
        "src/bpf_build",
        "bpf-toolchain.toml",
        "/sys/kernel/btf/vmlinux",
        "/proc/sys/kernel/random/boot_id",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    assert_eq!(
        env::var("CARGO_CFG_TARGET_ARCH").unwrap(),
        "x86_64",
        "procinsh requires Linux x86-64"
    );
    assert_eq!(
        env::var("CARGO_CFG_TARGET_OS").unwrap(),
        "linux",
        "procinsh requires Linux x86-64"
    );
    let config = fs::read_to_string(root.join("bpf-toolchain.toml")).unwrap();
    let toolchain = setting(&config, "channel");
    let linker_version = setting(&config, "linker_version");
    let version = Command::new("bpf-linker").arg("--version").output().expect("Install the prebuilt bpf-linker specified in bpf-toolchain.toml and add it to PATH (see docs/DEVELOPMENT.md)");
    assert!(
        version.status.success()
            && String::from_utf8_lossy(&version.stdout).trim()
                == format!("bpf-linker {linker_version}"),
        "Expected bpf-linker {linker_version}"
    );
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let btf = fs::read("/sys/kernel/btf/vmlinux")
        .expect("Build on the execution kernel with readable /sys/kernel/btf/vmlinux");
    let layout = kernel_layout::KernelLayoutReader::parse(&btf)
        .and_then(|reader| reader.generate())
        .expect("compatible kernel BTF fields");
    let layout_path = out.join("kernel_layout.rs");
    fs::write(&layout_path, layout).unwrap();
    let digest: [u8; 32] = Sha256::digest(&btf).into();
    fs::write(
        out.join("kernel_btf_digest.rs"),
        format!("pub(super) const BUILD_BTF_DIGEST: [u8; 32] = {digest:?};\n"),
    )
    .unwrap();
    let source = out.join("ebpf-source");
    copy_tree(&root.join("src/ebpf"), &source);
    // A manifest template keeps Cargo from excluding this nested package from
    // the procinsh .crate archive. The isolated package only exists in OUT_DIR.
    fs::rename(source.join("Cargo.toml.in"), source.join("Cargo.toml")).unwrap();
    println!("cargo:rerun-if-env-changed=PROCINSH_BPF_CLIPPY");
    let target = out.join("ebpf-target");
    let result = bpf_command(&toolchain, "build", &source, &target, &layout_path)
        .output()
        .expect("rustup is required for BPF compilation");
    assert!(
        result.status.success(),
        "BPF compilation failed. Install {toolchain} with rust-src and bpf-linker {linker_version} (see docs/DEVELOPMENT.md):\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
    if env::var("PROCINSH_BPF_CLIPPY").as_deref() == Ok("1") {
        let result = bpf_command(&toolchain, "clippy", &source, &target, &layout_path)
            .args(["--", "-D", "warnings"])
            .output()
            .expect("BPF Clippy");
        assert!(
            result.status.success(),
            "BPF Clippy failed (install clippy for {toolchain}):\n{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    for name in ["sched", "ipc", "files"] {
        fs::copy(
            target.join("bpfel-unknown-none/release").join(name),
            out.join(format!("{name}.bpf.o")),
        )
        .unwrap();
    }
}
fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), dest).unwrap();
        }
    }
}

fn bpf_command(
    toolchain: &str,
    action: &str,
    source: &Path,
    target: &Path,
    layout_path: &Path,
) -> Command {
    let mut command = Command::new("rustup");
    command.args(["run", toolchain, "cargo", action, "--locked", "--release", "--target", "bpfel-unknown-none", "-Z", "build-std=core"])
        .current_dir(source).env("CARGO_TARGET_DIR", target)
        .env("PROCINSH_KERNEL_LAYOUT", layout_path)
        .env("CARGO_ENCODED_RUSTFLAGS", "-C\x1flinker=bpf-linker\x1f-C\x1flink-arg=--btf\x1f--cfg\x1fbpf_target_arch=\"x86_64\"");
    // Parent Cargo sets these for stable/clippy/host targets. BPF has its own
    // compiler and target; inherited values must not override the pinned nightly.
    for key in [
        "RUSTC",
        "RUSTDOC",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTFLAGS",
        "CARGO_BUILD_RUSTC",
        "CARGO_BUILD_TARGET",
        "RUSTUP_TOOLCHAIN",
    ] {
        command.env_remove(key);
    }
    command
}
