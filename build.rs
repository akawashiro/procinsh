use std::{env, path::PathBuf, process::Command};
fn main() {
    println!("cargo:rerun-if-changed=src/space/activity.bpf.c");
    println!("cargo:rerun-if-changed=src/space/ibs.c");
    println!("cargo:rerun-if-changed=src/space/files.bpf.c");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let btf = Command::new("bpftool")
        .args([
            "btf",
            "dump",
            "file",
            "/sys/kernel/btf/vmlinux",
            "format",
            "c",
        ])
        .output()
        .expect("bpftool is required to build the CO-RE collector");
    assert!(btf.status.success(), "BTF header generation failed");
    std::fs::write(out.join("vmlinux.h"), btf.stdout).unwrap();
    for name in ["activity", "files"] {
        libbpf_cargo::SkeletonBuilder::new()
            .source(format!("src/space/{name}.bpf.c"))
            .clang_args([format!("-I{}", out.display()), "-D__TARGET_ARCH_x86".into()])
            .obj(out.join(format!("{name}.bpf.o")))
            .build()
            .expect("BPF compile");
    }
    assert!(
        Command::new("cc")
            .args([
                "-O2",
                "-Wall",
                "-Wextra",
                "-Werror",
                "-c",
                "src/space/ibs.c",
                "-o"
            ])
            .arg(out.join("ibs.o"))
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("ar")
            .arg("crs")
            .arg(out.join("libprocinsh_ibs.a"))
            .arg(out.join("ibs.o"))
            .status()
            .unwrap()
            .success()
    );
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=procinsh_ibs");
}
