#[path = "../build_support/web_assets.rs"]
mod web_assets;

#[test]
fn rejects_stale_missing_and_mixed_web_builds() {
    use std::{fs, path::PathBuf};
    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let root =
        Fixture(std::env::temp_dir().join(format!("procinsh-assets-{}", std::process::id())));
    let manifest = include_str!("../dist/web/build-manifest.json");
    let value: serde_json::Value = serde_json::from_str(manifest).unwrap();
    for (path, contents) in value["files"].as_object().unwrap() {
        let path = root.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents.as_str().unwrap()).unwrap();
    }
    let manifest_path = root.0.join("dist/web/build-manifest.json");
    fs::write(&manifest_path, manifest).unwrap();
    assert!(web_assets::validate(&root.0).is_ok());
    for path in [
        "src/web/app.ts",
        "src/web/index.html",
        "src/web/api-types.ts",
        "tsconfig.json",
        "package-lock.json",
        "dist/web/app.js",
    ] {
        let file = root.0.join(path);
        let original = fs::read(&file).unwrap();
        fs::write(&file, "stale or mismatched build").unwrap();
        assert!(web_assets::validate(&root.0).unwrap_err().contains(path));
        fs::remove_file(&file).unwrap();
        assert!(web_assets::validate(&root.0).is_err());
        fs::write(file, original).unwrap();
    }
    let added = root.0.join("src/web/new.ts");
    fs::write(&added, "const added = true;").unwrap();
    assert!(web_assets::validate(&root.0).is_err());
    fs::remove_file(added).unwrap();
    assert!(web_assets::validate(&root.0).is_ok());
    fs::remove_file(manifest_path).unwrap();
    assert!(
        web_assets::validate(&root.0)
            .unwrap_err()
            .contains("Missing web build manifest")
    );
}
