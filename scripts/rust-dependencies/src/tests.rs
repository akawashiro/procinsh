//! Behavioral fixtures for dependency extraction and build configuration.
//!
//! # Interface
//! No items are exported.
use super::{BuildConfig, DependencyGraph, DependencyScanner};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn config() -> BuildConfig {
    BuildConfig {
        flags: ["unix", "target_os=\"linux\"", "target_arch=\"x86_64\""]
            .into_iter()
            .map(str::to_owned)
            .collect(),
    }
}

fn graph(source: &str) -> DependencyGraph {
    let config = config();
    let mut scanner = DependencyScanner::new(&config);
    scanner
        .load(
            "fixture",
            Path::new("fixture.rs"),
            Path::new("."),
            Some(syn::parse_file(source).unwrap().items),
        )
        .unwrap();
    scanner.scan().unwrap()
}

fn edges(graph: &DependencyGraph) -> BTreeSet<(&str, &str)> {
    graph
        .edges
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect()
}

#[test]
fn reexports_stop_at_the_named_interface_and_direct_imports_survive() {
    let graph = graph(
        r#"
        mod api {
            pub mod implementation { pub struct Value; }
            pub use implementation::Value;
        }
        mod through_facade { use crate::api::Value; }
        mod direct { use crate::api::implementation::Value; }
        mod both { use crate::api::{Value, implementation::Value as Raw}; }
    "#,
    );
    assert_eq!(
        edges(&graph),
        BTreeSet::from([
            ("fixture::api", "fixture::api::implementation"),
            ("fixture::through_facade", "fixture::api"),
            ("fixture::direct", "fixture::api::implementation"),
            ("fixture::both", "fixture::api"),
            ("fixture::both", "fixture::api::implementation"),
        ])
    );
    assert!(graph.diagnostics.is_empty());
}

#[test]
fn aliases_relative_paths_and_chained_reexports_preserve_the_entry_point() {
    let graph = graph(
        r#"
        mod implementation { pub struct Value; }
        mod first { pub use crate::implementation::Value; }
        mod second { pub use super::first::Value; }
        mod user {
            use super::second::{self as facade, Value as Renamed};
            type A = facade::Value;
            type B = Renamed;
            fn f() { use crate::first as local; let _: local::Value; }
        }
    "#,
    );
    assert_eq!(
        edges(&graph),
        BTreeSet::from([
            ("fixture::first", "fixture::implementation"),
            ("fixture::second", "fixture::first"),
            ("fixture::user", "fixture::second"),
            ("fixture::user", "fixture::first"),
        ])
    );
    assert!(graph.diagnostics.is_empty());
}

#[test]
fn module_reexports_keep_the_outer_facade() {
    let graph = graph(
        r#"
        mod implementation { pub struct Value; }
        mod api { pub use crate::implementation as nested; }
        mod user {
            use crate::api::nested as alias;
            type A = alias::Value;
            type B = crate::api::nested::Value;
        }
    "#,
    );
    assert_eq!(
        edges(&graph),
        BTreeSet::from([
            ("fixture::api", "fixture::implementation"),
            ("fixture::user", "fixture::api"),
        ])
    );
}

#[test]
fn explicit_expression_type_and_trait_paths_are_collected() {
    let graph = graph(
        r#"
        mod api { pub struct Value; pub trait Bound {} pub fn make() {} }
        mod user {
            fn f<T: crate::api::Bound>() -> Option<crate::api::Value> {
                crate::api::make(); None
            }
        }
    "#,
    );
    assert_eq!(
        edges(&graph),
        BTreeSet::from([("fixture::user", "fixture::api")])
    );
}

#[test]
fn build_conditions_exclude_tests_inactive_fields_and_blocks() {
    let graph = graph(
        r#"
        mod api { pub struct Value; pub fn make() {} }
        #[cfg(test)] mod tests { use super::*; }
        #[cfg(target_arch = "aarch64")] mod arm { use crate::api::Value; }
        #[cfg(all(unix, target_arch = "x86_64", not(test)))] mod native { use crate::api::Value; }
        #[cfg_attr(unix, cfg(test))] mod conditional_test { use super::*; }
        mod user {
            struct Data { #[cfg(test)] value: crate::api::Value }
            fn f() { #[cfg(test)] { crate::api::make(); } }
        }
    "#,
    );
    assert!(
        !graph
            .nodes
            .iter()
            .any(|node| node.ends_with("tests") || node.ends_with("arm"))
    );
    assert_eq!(
        edges(&graph),
        BTreeSet::from([("fixture::native", "fixture::api")])
    );
}

#[test]
fn production_globs_are_errors_with_a_location() {
    let config = config();
    let mut scanner = DependencyScanner::new(&config);
    let error = scanner
        .load(
            "fixture",
            Path::new("fixture.rs"),
            Path::new("."),
            Some(syn::parse_file("use crate::api::*;").unwrap().items),
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("wildcard"));
    assert!(format!("{error:#}").contains("line 1"));
}

#[test]
fn unresolved_internal_references_are_diagnosed_without_inventing_edges() {
    let graph = graph("mod user { use crate::missing::Value; }");
    assert!(graph.edges.is_empty());
    assert!(
        graph
            .diagnostics
            .iter()
            .any(|text| text.contains("fixture.rs:1") && text.contains("crate::missing::Value"))
    );
}

#[test]
fn local_aliases_do_not_escape_blocks_and_values_shadow_imported_names() {
    let graph = graph(
        r#"
        mod api { pub struct Value; }
        mod user {
            fn f() {
                { use crate::api as alias; let _: alias::Value; }
                let alias = 0;
                alias;
            }
        }
        mod unrelated { fn f() { let api = 1; api; } }
    "#,
    );
    assert_eq!(
        edges(&graph),
        BTreeSet::from([("fixture::user", "fixture::api")])
    );
}

#[test]
fn external_imports_macros_and_inferred_types_do_not_create_internal_edges() {
    let graph = graph(
        r#"
        use std::{self as standard, path::Path};
        fn f() { standard::mem::drop(1); custom!(crate::missing::Value); }
    "#,
    );
    assert!(graph.edges.is_empty());
    assert!(graph.diagnostics.is_empty());
}

struct SourceFixture(PathBuf);
impl SourceFixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "procinsh-graph-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        Self(dir)
    }
    fn write(&self, name: &str, source: &str) {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }
}
impl Drop for SourceFixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn path_attributes_inline_modules_and_executable_trees_are_separate() {
    let fixture = SourceFixture::new();
    fixture.write("main.rs", "#[path = \"shared/root.rs\"] mod ipc; mod app;");
    fixture.write("shared/root.rs", "pub mod wire; pub struct Value;");
    fixture.write("shared/wire.rs", "use super::Value;");
    fixture.write("app.rs", "mod inner { use crate::ipc::Value; } mod child;");
    fixture.write("app/child.rs", "use crate::ipc::Value;");
    let config = config();
    let mut scanner = DependencyScanner::new(&config);
    for name in ["server", "helper"] {
        scanner
            .load(name, &fixture.0.join("main.rs"), &fixture.0, None)
            .unwrap();
    }
    let graph = scanner.scan().unwrap();
    for name in ["server", "helper"] {
        assert!(graph.nodes.contains(&format!("{name}::ipc::wire")));
        for suffix in ["app::inner", "app::child", "ipc::wire"] {
            assert!(
                graph
                    .edges
                    .contains(&(format!("{name}::{suffix}"), format!("{name}::ipc")))
            );
        }
    }
    assert!(
        graph
            .edges
            .iter()
            .all(|(a, b)| a.split("::").next() == b.split("::").next())
    );
}

#[test]
fn type_parameters_and_local_types_do_not_resolve_as_modules() {
    let graph = graph(
        r#"
        mod api { pub fn make() {} }
        fn f<api>() { api::make(); }
        fn g() { struct api; api::make(); }
        fn h(api: i32) { api; }
    "#,
    );
    assert!(graph.edges.is_empty());
}

#[test]
fn value_bindings_do_not_shadow_module_paths() {
    let graph = graph(
        r#"
        mod api { pub fn make() {} }
        fn f() { let api = 0; api::make(); }
    "#,
    );
    assert_eq!(edges(&graph), BTreeSet::from([("fixture", "fixture::api")]));
}

#[test]
fn chained_block_aliases_are_resolved_in_their_lexical_scope() {
    let graph = graph(
        r#"
        mod api { pub struct Value; }
        fn f() {
            use crate::api as a;
            use a::Value as V;
            let _: V;
        }
    "#,
    );
    assert_eq!(edges(&graph), BTreeSet::from([("fixture", "fixture::api")]));
    assert!(graph.diagnostics.is_empty());
}

#[test]
fn missing_members_of_reexported_modules_are_reported() {
    let graph = graph(
        r#"
        mod implementation { pub struct Value; }
        mod api { pub use crate::implementation as nested; }
        mod user { use crate::api::nested::Missing; }
    "#,
    );
    assert!(
        !graph
            .edges
            .contains(&("fixture::user".into(), "fixture::api".into()))
    );
    assert!(!graph.diagnostics.is_empty());
}

#[test]
fn inner_and_nested_conditional_attributes_are_respected() {
    let graph = graph(
        r#"
        mod api { pub struct Value; }
        mod tests { #![cfg(test)] use super::*; }
        #[cfg_attr(unix, cfg_attr(target_os = "linux", cfg(test)))]
        mod also_tests { use super::*; }
    "#,
    );
    assert_eq!(
        graph.nodes,
        BTreeSet::from(["fixture".into(), "fixture::api".into()])
    );
}

#[test]
fn path_attributes_in_non_root_files_are_relative_to_the_source_file() {
    let fixture = SourceFixture::new();
    fixture.write("main.rs", "mod json;");
    fixture.write(
        "json.rs",
        "#[path = \"json/acquisition.rs\"] mod acquisition;",
    );
    fixture.write("json/acquisition.rs", "pub struct Value;");
    let config = config();
    let mut scanner = DependencyScanner::new(&config);
    scanner
        .load("fixture", &fixture.0.join("main.rs"), &fixture.0, None)
        .unwrap();
    assert!(
        scanner
            .scan()
            .unwrap()
            .nodes
            .contains("fixture::json::acquisition")
    );
}

#[test]
fn external_items_reexported_by_internal_facades_still_depend_on_the_facade() {
    let graph = graph(
        r#"
        mod api {
            pub use external::Value;
            pub use external::module as nested;
            pub use ::other::Value as Another;
        }
        mod user { use crate::api::{Value, Another, nested::Other}; }
    "#,
    );
    assert_eq!(
        edges(&graph),
        BTreeSet::from([("fixture::user", "fixture::api")])
    );
    assert!(graph.diagnostics.is_empty());
}

#[test]
fn self_qualified_module_aliases_do_not_hide_direct_implementation_access() {
    let graph = graph(
        r#"
        mod api { pub mod inner { pub struct Value; } }
        mod user { use crate::api as alias; type A = self::alias::inner::Value; }
    "#,
    );
    assert_eq!(
        edges(&graph),
        BTreeSet::from([
            ("fixture::user", "fixture::api"),
            ("fixture::user", "fixture::api::inner"),
        ])
    );
}
