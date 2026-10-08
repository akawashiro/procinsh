//! Compile-time Git identity and Cargo inputs for the binary's web headers.
//!
//! # Interface
//! - [`emit`]: `pub(super) fn emit(root: &Path)`; emits the validated revision,
//!   tracked-change flag, and Cargo rebuild inputs for the manifest directory.

use std::{env, path::Path, process::Command};

#[derive(Debug, PartialEq)]
struct BuildRevision {
    sha: String,
    dirty: bool,
}

fn git(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

fn git_text(root: &Path, args: &[&str]) -> Option<String> {
    Some(String::from_utf8(git(root, args)?).ok()?.trim().to_owned())
}

fn full_sha(value: &str) -> Option<String> {
    (value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| value.to_ascii_lowercase())
}

fn is_checkout(root: &Path) -> bool {
    // Git otherwise searches ancestors, including when building a packaged crate
    // under another checkout's target directory.
    root.join(".git").exists()
        && git_text(root, &["rev-parse", "--show-toplevel"])
            .and_then(|path| Path::new(&path).canonicalize().ok())
            .is_some_and(|path| Some(path) == root.canonicalize().ok())
}

fn revision(root: &Path, override_sha: Option<&str>) -> Option<BuildRevision> {
    if let Some(value) = override_sha {
        // An explicit empty value requests a version-only build.
        return (!value.is_empty()).then(|| BuildRevision {
            sha: full_sha(value).expect("PROCINSH_GIT_SHA must be a full 40-digit hexadecimal SHA"),
            dirty: false,
        });
    }
    if !is_checkout(root) {
        return None;
    }
    let sha = full_sha(&git_text(root, &["rev-parse", "--verify", "HEAD"])?)?;
    let status = git(root, &["status", "--porcelain", "--untracked-files=no"])?;
    Some(BuildRevision {
        sha,
        dirty: !status.is_empty(),
    })
}

fn watch_git_path(root: &Path, name: &str) {
    if let Some(path) = git_text(root, &["rev-parse", "--git-path", name]) {
        let path = root.join(path);
        if path.is_file() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}

/// An override is authoritative and has no dirty suffix. Without one, only the
/// manifest's own checkout is used; modifications to tracked files mark it dirty.
/// Both environment values are emitted even when no revision is available, so
/// reused Cargo artifacts cannot retain an earlier build's identity.
pub(super) fn emit(root: &Path) {
    println!("cargo:rerun-if-env-changed=PROCINSH_GIT_SHA");
    let override_sha = env::var_os("PROCINSH_GIT_SHA").map(|value| {
        value
            .into_string()
            .expect("PROCINSH_GIT_SHA must be a full 40-digit hexadecimal SHA")
    });
    if override_sha.is_none() && is_checkout(root) {
        if root.join(".git").is_file() {
            println!("cargo:rerun-if-changed={}", root.join(".git").display());
        }
        for name in ["HEAD", "index", "packed-refs"] {
            watch_git_path(root, name);
        }
        if let Some(reference) = git_text(root, &["symbolic-ref", "-q", "HEAD"]) {
            watch_git_path(root, &reference);
        }
        // Index/HEAD alone do not change when an unstaged source file is edited.
        if let Some(files) = git(root, &["ls-files", "-z"]) {
            for file in files
                .split(|byte| *byte == 0)
                .filter(|file| !file.is_empty())
            {
                println!(
                    "cargo:rerun-if-changed={}",
                    root.join(String::from_utf8_lossy(file).as_ref()).display()
                );
            }
        }
    }
    let info = revision(root, override_sha.as_deref());
    println!(
        "cargo:rustc-env=PROCINSH_BUILD_GIT_SHA={}",
        info.as_ref().map_or("", |info| info.sha.as_str())
    );
    println!(
        "cargo:rustc-env=PROCINSH_BUILD_GIT_DIRTY={}",
        info.is_some_and(|info| info.dirty)
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    struct TestRepository {
        root: PathBuf,
    }
    impl TestRepository {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = env::temp_dir().join(format!(
                "procinsh-build-git-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let repo = Self { root };
            repo.run(&["init", "--initial-branch=main"]);
            fs::write(repo.root.join("source"), "first").unwrap();
            repo.commit();
            repo
        }
        fn run(&self, args: &[&str]) -> String {
            git_text(&self.root, args).unwrap()
        }
        fn commit(&self) {
            self.run(&["add", "source"]);
            self.run(&[
                "-c",
                "user.name=Build Test",
                "-c",
                "user.email=build@example.test",
                "commit",
                "-m",
                "test source",
            ]);
        }
    }
    impl Drop for TestRepository {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn validates_override_and_supports_version_only() {
        let sha = "ABCDEF0123456789ABCDEF0123456789ABCDEF01";
        let root = Path::new("/not/a/checkout");
        assert_eq!(
            revision(root, Some(sha)),
            Some(BuildRevision {
                sha: sha.to_ascii_lowercase(),
                dirty: false
            })
        );
        assert_eq!(revision(root, Some("")), None);
        assert_eq!(revision(root, None), None);
        for value in [
            "abcdef0",
            "<script>",
            "000000000000000000000000000000000000000g",
        ] {
            assert!(full_sha(value).is_none());
        }
    }

    #[test]
    #[should_panic(expected = "PROCINSH_GIT_SHA must be a full")]
    fn rejects_invalid_override() {
        revision(Path::new("."), Some("bad"));
    }

    #[test]
    fn checkout_identity_and_tracked_changes() {
        let repo = TestRepository::new();
        let original = revision(&repo.root, None).unwrap();
        assert!(!original.dirty);
        fs::write(repo.root.join("untracked"), "ignored for dirty detection").unwrap();
        assert_eq!(revision(&repo.root, None).as_ref(), Some(&original));
        fs::write(repo.root.join("source"), "second").unwrap();
        assert!(revision(&repo.root, None).unwrap().dirty);
        // An explicit preview revision remains authoritative in a dirty checkout.
        assert!(!revision(&repo.root, Some(&original.sha)).unwrap().dirty);
        repo.commit();
        let next = revision(&repo.root, None).unwrap();
        assert_ne!(next.sha, original.sha);
        assert!(!next.dirty);
    }

    #[test]
    fn ignores_parent_checkout_and_handles_linked_worktree() {
        let repo = TestRepository::new();
        let packaged = repo.root.join("target/package/procinsh");
        fs::create_dir_all(&packaged).unwrap();
        assert_eq!(revision(&packaged, None), None);
        let worktree = repo.root.join("linked");
        repo.run(&[
            "worktree",
            "add",
            "--detach",
            worktree.to_str().unwrap(),
            "HEAD",
        ]);
        assert!(worktree.join(".git").is_file());
        assert_eq!(revision(&worktree, None), revision(&repo.root, None));
        let gitless = repo.root.join("exported");
        fs::create_dir(&gitless).unwrap();
        fs::write(gitless.join(".cargo_vcs_info.json"), "{}").unwrap();
        assert_eq!(revision(&gitless, None), None);
    }
}
