use std::{env, path::PathBuf, process::Command};

#[path = "build/git.rs"]
mod git;

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    git::emit(&root);
    let dist = root.join("web/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    for page in ["list", "process", "space"] {
        let path = dist.join(page).join("index.html");
        assert!(
            path.is_file(),
            "Missing frontend output {}. Run `npm --prefix web ci && npm --prefix web run build` before building with Cargo.",
            path.display()
        );
    }
    assert!(
        dist.join("assets").is_dir(),
        "Missing frontend assets. Run `npm --prefix web ci && npm --prefix web run build` before building with Cargo."
    );
    println!("cargo:rerun-if-changed=src/http_server/system/sched.bpf.c");
    println!("cargo:rerun-if-changed=src/http_server/system/ipc.bpf.c");
    println!("cargo:rerun-if-changed=src/http_server/system/files.bpf.c");
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
    for name in ["sched", "ipc", "files"] {
        libbpf_cargo::SkeletonBuilder::new()
            .source(format!("src/http_server/system/{name}.bpf.c"))
            .clang_args([format!("-I{}", out.display()), "-D__TARGET_ARCH_x86".into()])
            .obj(out.join(format!("{name}.bpf.o")))
            .build()
            .expect("BPF compile");
    }
}
