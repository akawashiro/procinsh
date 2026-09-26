use std::{env, path::PathBuf, process::Command};
fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    for asset in ["app.js", "space.js", "space-model.js"] {
        let path = root.join("dist/web").join(asset);
        println!("cargo:rerun-if-changed={}", path.display());
        assert!(
            path.is_file(),
            "Missing web asset {}. Run `npm ci && npm run build:web` before building with Cargo.",
            path.display()
        );
    }
    println!("cargo:rerun-if-changed=src/system/activity.bpf.c");
    println!("cargo:rerun-if-changed=src/system/files.bpf.c");
    println!("cargo:rerun-if-env-changed=BPFTOOL");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let bpftool = env::var_os("BPFTOOL").unwrap_or_else(|| "bpftool".into());
    let btf = Command::new(&bpftool)
        .args([
            "btf",
            "dump",
            "file",
            "/sys/kernel/btf/vmlinux",
            "format",
            "c",
        ])
        .output()
        .unwrap_or_else(|error| {
            panic!(
                "Failed to execute {bpftool:?}: {error}. Install bpftool or set BPFTOOL to its executable path."
            )
        });
    assert!(
        btf.status.success(),
        "{bpftool:?} btf dump file /sys/kernel/btf/vmlinux format c failed ({}):\n{}",
        btf.status,
        String::from_utf8_lossy(&btf.stderr)
    );
    std::fs::write(out.join("vmlinux.h"), btf.stdout).unwrap();
    for name in ["activity", "files"] {
        libbpf_cargo::SkeletonBuilder::new()
            .source(format!("src/system/{name}.bpf.c"))
            .clang_args([format!("-I{}", out.display()), "-D__TARGET_ARCH_x86".into()])
            .obj(out.join(format!("{name}.bpf.o")))
            .build()
            .expect("BPF compile");
    }
}
