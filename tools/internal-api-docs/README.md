# Internal API documentation

This project-specific frontend displays each module's explicit `pub use` items
as its facade interface. It traverses rustdoc's module items and follows resolved
item IDs (including aliases and chained re-exports). It does not parse Rust,
resolve names, or read source files. Modules are discovered automatically.

## Build

From the repository root, after the usual web/native build prerequisites:

```sh
rustup toolchain install nightly-2026-09-28 --profile minimal
cargo +nightly-2026-09-28 rustdoc --locked --bin procinsh \
  --target-dir target/internal-api-json -- \
  --document-private-items --document-hidden-items -Z unstable-options --output-format json
cargo run --locked --manifest-path tools/internal-api-docs/Cargo.toml -- \
  target/internal-api-json/doc/procinsh.json target/doc/internal-api
```

Open `target/doc/internal-api/index.html`. The output directory is owned by the
generator: its HTML files are replaced on each successful generation to remove
obsolete facade pages. Other file types are left alone. Input/extraction failures
leave the previous site intact.

JSON format **61**, `rustdoc-types` **0.61.0**, and **nightly-2026-09-28** are a
matched set. Update them together and rerun the tests and production generation.
The generator rejects a different format version. Production build/test and
ordinary HTML rustdoc continue to use **Rust 1.98.1** from the root toolchain file.
The tool has its own manifest and lockfile and is not a production dependency.

## Visibility and scope

Even with `--document-private-items --document-hidden-items`, the pinned rustdoc
removes restricted imports (`pub(super) use`, `pub(crate) use`, `pub(in ...) use`)
from JSON. Consequently, facade re-exports **and their definitions must be `pub`**.
This intentionally revises the original visibility requirement in issue #41.
Implementation module declarations remain private. Private ancestors still
restrict accessibility, but this change can widen access inside those boundaries.
Use ordinary `use` for implementation imports; they do not become interface items.
The compiler fixture in `tests/fixtures/facades.rs` exercises this distinction.

The pages show re-export names, item kinds, declarations, re-export visibility,
and the target's source file and line. Function declarations include qualifiers,
inputs, return types, generic parameters and bounds. Type spelling and normalized
bounds come from rustdoc JSON, so they may differ from the source spelling.
Struct/enum declarations include fields/variants. The renderer supports functions,
structs, enums, aliases and constants, which cover the current facades. It does
not enumerate methods or trait implementations, and does not collect doc comments.
The JSON uses the normal documentation configuration, not `cfg(test)`.

Glob re-exports, external targets absent from this crate's JSON, stripped fields,
and unsupported item kinds fail explicitly instead of producing incomplete
signatures. Add renderer support when the project's interfaces need these forms.

## Validation

```sh
cargo fmt --manifest-path tools/internal-api-docs/Cargo.toml --check
cargo test --locked --manifest-path tools/internal-api-docs/Cargo.toml
cargo clippy --locked --manifest-path tools/internal-api-docs/Cargo.toml --all-targets -- -D warnings
```

The integration test invokes the pinned nightly rustdoc on a small fixture and
checks the actual generated HTML, aliases, nested facades, generic signatures,
escaping, unsupported input errors and regeneration. No native/BPF dependencies
are needed for this fixture. The documentation workflow also generates all
production facade pages and publishes them alongside rustdoc and architecture.
