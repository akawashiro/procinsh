mod extract;
mod render;
mod signature;

use anyhow::{Context, Result, ensure};
use std::{env, fs, path::PathBuf};

fn main() -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    ensure!(
        args.len() == 2,
        "usage: internal-api-docs INPUT.json OUTPUT_DIRECTORY"
    );
    let input = PathBuf::from(&args[0]);
    let value: serde_json::Value = serde_json::from_slice(
        &fs::read(&input).with_context(|| format!("reading {}", input.display()))?,
    )?;
    ensure!(
        value["format_version"].as_u64() == Some(u64::from(rustdoc_types::FORMAT_VERSION)),
        "unsupported rustdoc JSON version {}; expected {} (nightly-2026-09-28)",
        value["format_version"],
        rustdoc_types::FORMAT_VERSION,
    );
    let krate: rustdoc_types::Crate = serde_json::from_value(value)?;
    ensure!(
        krate.includes_private,
        "generate JSON with --document-private-items"
    );
    let facades = extract::extract(&krate)?;
    ensure!(
        !facades.is_empty(),
        "no facade re-exports found; rustdoc omits restricted imports, use pub use and pub definitions"
    );
    render::write(&facades, &PathBuf::from(&args[1]))?;
    println!("Generated {} facade pages", facades.len());
    Ok(())
}
