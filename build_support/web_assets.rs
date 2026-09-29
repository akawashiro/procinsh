use std::{collections::BTreeSet, fs, path::Path};

fn inputs(root: &Path, directory: &Path, paths: &mut BTreeSet<String>) -> Result<(), String> {
    for entry in fs::read_dir(root.join(directory)).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = directory.join(entry.file_name());
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            if entry.file_name() != "vendor" {
                inputs(root, &path, paths)?;
            }
        } else if matches!(
            path.extension().and_then(|s| s.to_str()),
            Some("ts" | "html" | "css")
        ) {
            paths.insert(path.to_str().ok_or("Invalid web asset path")?.to_owned());
        }
    }
    Ok(())
}

pub fn validate(root: &Path) -> Result<(), String> {
    let manifest = fs::read(root.join("dist/web/build-manifest.json"))
        .map_err(|e| format!("Missing web build manifest: {e}"))?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&manifest).map_err(|e| e.to_string())?;
    if manifest["version"] != 1 {
        return Err("Unsupported web build manifest".into());
    }
    let files = manifest["files"]
        .as_object()
        .ok_or("Invalid web build manifest")?;
    let mut expected: BTreeSet<String> = [
        "package.json",
        "package-lock.json",
        "tsconfig.json",
        "scripts/build-web.mjs",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    inputs(root, Path::new("src/web"), &mut expected)?;
    for entry in fs::read_dir(root.join("dist/web")).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_some_and(|ext| ext == "js") {
            expected.insert(format!(
                "dist/web/{}",
                path.file_name().unwrap().to_str().unwrap()
            ));
        }
    }
    for asset in ["app.js", "space.js", "space-model.js"] {
        expected.insert(format!("dist/web/{asset}"));
    }
    if expected != files.keys().cloned().collect() {
        return Err("Web build inputs or outputs changed".into());
    }
    for path in expected {
        let contents = fs::read_to_string(root.join(&path)).map_err(|e| format!("{path}: {e}"))?;
        if files[&path].as_str() != Some(&contents) {
            return Err(format!("Stale web build: {path} changed"));
        }
    }
    Ok(())
}
