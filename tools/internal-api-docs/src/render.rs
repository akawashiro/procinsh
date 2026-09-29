use crate::extract::Facade;
use anyhow::Result;
use std::{fs, path::Path};

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn filename(path: &str) -> String {
    format!("{}.html", path.replace("::", "-"))
}

fn page(title: &str, body: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title} — Internal API</title>
<style>
:root {{ color-scheme: light dark; font-family: system-ui, sans-serif; }}
body {{ max-width: 72rem; margin: 2rem auto; padding: 0 1.5rem; line-height: 1.6; }}
a {{ color: light-dark(#164eae, #9cc5ff); }}
pre {{ padding: 1rem; overflow-x: auto; background: light-dark(#f3f5f8, #20252d); border-radius: .4rem; }}
article {{ border-top: 1px solid #8886; padding-bottom: 1rem; }}
code {{ overflow-wrap: anywhere; }}
</style></head><body>
<nav><a href="../">All documentation</a> · <a href="index.html">Internal API</a></nav>
<h1>{title}</h1>{body}</body></html>
"#,
        title = escape(title)
    )
}

pub fn write(facades: &[Facade], output: &Path) -> Result<()> {
    // All extraction/rendering succeeds before writing any output.
    let mut pages = Vec::new();
    let mut index = String::from(
        "<p>Module interfaces declared by explicit re-exports. Visibility is relative to each module; private ancestor modules still limit access.</p><ul>",
    );
    for facade in facades {
        let file = filename(&facade.path);
        index.push_str(&format!(
            "<li><a href=\"{}\">{}</a> ({} items)</li>",
            escape(&file),
            escape(&facade.path),
            facade.items.len()
        ));
        let mut body = String::new();
        let mut kind = "";
        for item in &facade.items {
            if kind != item.kind {
                kind = item.kind;
                body.push_str(&format!("<h2>{kind}</h2>"));
            }
            body.push_str(&format!(
                "<article><h3>{}</h3><pre><code>{}</code></pre><p>Re-export visibility: <code>{}</code><br>Defined in: <code>{}</code></p></article>",
                escape(&item.name), escape(&item.signature), escape(&item.visibility), escape(&item.source)
            ));
        }
        pages.push((file, page(&facade.path, &body)));
    }
    index.push_str("</ul>");
    pages.push(("index.html".into(), page("Internal API", &index)));
    fs::create_dir_all(output)?;
    // This directory belongs to the generator; remove obsolete facade pages on rebuild.
    for entry in fs::read_dir(output)? {
        let path = entry?.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "html") {
            fs::remove_file(path)?;
        }
    }
    for (file, contents) in pages {
        fs::write(output.join(file), contents)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn escapes_rust_and_html() {
        assert_eq!(
            super::escape("&'a Vec<\"x\">"),
            "&amp;&#39;a Vec&lt;&quot;x&quot;&gt;"
        );
    }
}
