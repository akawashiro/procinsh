use serde_json::Value;
use std::{fs, path::Path, process::Command};

fn generate(input: &Path, output: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_internal-api-docs"))
        .args([input, output])
        .output()
        .unwrap()
}

#[test]
fn compiler_resolved_facades_and_errors() {
    let dir = std::env::temp_dir().join(format!("internal-api-docs-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let result = Command::new("rustdoc")
        .args([
            "+nightly-2026-09-28",
            "--edition=2024",
            "--crate-name=facades",
            "--document-private-items",
            "--document-hidden-items",
            "-Zunstable-options",
            "--output-format=json",
        ])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/facades.rs"))
        .arg("-o")
        .arg(&dir)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let input = dir.join("facades.json");
    let output = dir.join("html");
    let result = generate(&input, &output);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let html = fs::read_to_string(output.join("api.html")).unwrap();
    for expected in [
        "pub async fn renamed_run&lt;&#39;a, T, const N: usize&gt;",
        "bytes: [u8; N]",
        "for&lt;&#39;b&gt; unsafe extern &quot;C&quot; fn(&amp;&#39;b str) -&gt; usize",
        "where\n    T: Send + Sync + AsRef&lt;str&gt;",
        "impl Iterator&lt;Item = u8&gt;",
        "dyn std::fmt::Display + Send",
        "&lt;T&gt;::Item",
        "pub fn opaque(value: impl AsRef&lt;str&gt;)",
        "pub struct Record&lt;&#39;a&gt;",
        "private: [u8; 4]",
        "pub struct Tuple(pub u8, pub bool);",
        "pub struct Unit;",
        "pub enum Event",
        "Pair(u8, bool)",
        "Named {",
        "Option&lt;(u8,)&gt;",
        "&lt;script&gt;",
        "Defined in:",
        "facades.rs:",
    ] {
        assert!(html.contains(expected), "missing {expected} in {html}");
    }
    assert!(!html.contains("<script>"));
    assert!(!html.contains("<h3>run</h3>"));
    assert!(!html.contains("<h3>restricted</h3>"));
    let nested = fs::read_to_string(output.join("api-nested.html")).unwrap();
    assert!(nested.contains("pub async fn chained_run"));
    let index = fs::read_to_string(output.join("index.html")).unwrap();
    assert!(index.contains("api-nested.html"));
    assert!(!index.contains("implementation.html"));

    let original: Value = serde_json::from_slice(&fs::read(&input).unwrap()).unwrap();
    let mut invalid = original.clone();
    invalid["format_version"] = 0.into();
    fs::write(&input, serde_json::to_vec(&invalid).unwrap()).unwrap();
    let result = generate(&input, &output);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("unsupported rustdoc JSON version"));
    // A bad input must not replace the last successful site.
    assert_eq!(fs::read_to_string(output.join("api.html")).unwrap(), html);

    for (field, value, message) in [
        ("is_glob", Value::Bool(true), "glob re-export"),
        ("id", Value::Null, "no target ID"),
        ("id", Value::from(u32::MAX), "missing item"),
    ] {
        let mut invalid = original.clone();
        let import = invalid["index"]
            .as_object_mut()
            .unwrap()
            .values_mut()
            .find(|item| item["inner"]["use"]["name"] == "renamed_run")
            .unwrap();
        import["inner"]["use"][field] = value;
        fs::write(&input, serde_json::to_vec(&invalid).unwrap()).unwrap();
        let result = generate(&input, &output);
        assert!(!result.status.success());
        assert!(
            String::from_utf8_lossy(&result.stderr).contains(message),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    fs::write(&input, serde_json::to_vec(&original).unwrap()).unwrap();
    fs::write(output.join("obsolete.html"), "old facade").unwrap();
    fs::write(output.join("keep.txt"), "other artifact").unwrap();
    assert!(generate(&input, &output).status.success());
    assert!(!output.join("obsolete.html").exists());
    assert!(output.join("keep.txt").exists());
    assert_eq!(fs::read_to_string(output.join("api.html")).unwrap(), html);
    fs::remove_dir_all(dir).unwrap();
}
