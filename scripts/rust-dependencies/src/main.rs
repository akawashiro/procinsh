//! Extract source-level module dependencies without bypassing re-export façades.
//!
//! # Interface
//! No Rust items are exported. The executable accepts `ROOT NAME ENTRY` triples
//! after `--cfg-file FILE` and writes a JSON graph to stdout. ROOT is the source
//! directory, NAME the crate name, and ENTRY its root file relative to ROOT.

use anyhow::{Context, Result, bail};
use proc_macro2::Span;
use quote::ToTokens;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{Attribute, Item, Meta, Token, UseTree};

#[derive(Default, Serialize)]
struct DependencyGraph {
    nodes: BTreeSet<String>,
    edges: BTreeSet<(String, String)>,
    diagnostics: BTreeSet<String>,
}

/// The rustc configuration describes one normal build, with cfg(test) disabled.
struct BuildConfig {
    flags: BTreeSet<String>,
}

impl BuildConfig {
    fn active(&self, attrs: &[Attribute]) -> Result<bool> {
        for attr in attrs {
            if !self.active_meta(&attr.meta)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn active_meta(&self, meta: &Meta) -> Result<bool> {
        let Meta::List(list) = meta else {
            return Ok(true);
        };
        if list.path.is_ident("cfg") {
            return self.evaluate(&list.parse_args()?);
        }
        if list.path.is_ident("cfg_attr") {
            let args = list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
            let mut args = args.iter();
            let condition = args.next().context("cfg_attr requires a condition")?;
            if self.evaluate(condition)? {
                for meta in args {
                    if !self.active_meta(meta)? {
                        return Ok(false);
                    }
                }
            }
        }
        Ok(true)
    }

    fn evaluate(&self, meta: &Meta) -> Result<bool> {
        Ok(match meta {
            Meta::Path(path) => !path.is_ident("test") && self.flags.contains(&tokens(path)),
            Meta::NameValue(value) => self.flags.contains(&tokens(value)),
            Meta::List(list) => {
                let values =
                    list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
                let values = values
                    .iter()
                    .map(|value| self.evaluate(value))
                    .collect::<Result<Vec<_>>>()?;
                if list.path.is_ident("all") {
                    values.iter().all(|value| *value)
                } else if list.path.is_ident("any") {
                    values.iter().any(|value| *value)
                } else if list.path.is_ident("not") && values.len() == 1 {
                    !values[0]
                } else {
                    bail!("unsupported cfg predicate: {}", tokens(meta));
                }
            }
        })
    }
}

fn tokens(value: &impl ToTokens) -> String {
    value.to_token_stream().to_string().replace(' ', "")
}

/// Get outer attributes for AST nodes that do not expose a shared attrs accessor.
fn attributes(value: &impl ToTokens) -> Vec<Attribute> {
    (|input: syn::parse::ParseStream<'_>| {
        let attrs = input.call(Attribute::parse_outer)?;
        let _: proc_macro2::TokenStream = input.parse()?;
        Ok(attrs)
    })
    .parse2(value.to_token_stream())
    .unwrap_or_default()
}

#[derive(Clone)]
struct Import {
    path: Vec<String>,
    absolute: bool,
    name: String,
}

#[derive(Default)]
struct ModuleInfo {
    file: PathBuf,
    items: Vec<Item>,
    definitions: BTreeSet<String>,
    imports: BTreeMap<String, Import>,
}

struct DependencyScanner<'a> {
    config: &'a BuildConfig,
    modules: BTreeMap<String, ModuleInfo>,
    graph: DependencyGraph,
}

impl<'a> DependencyScanner<'a> {
    fn new(config: &'a BuildConfig) -> Self {
        Self {
            config,
            modules: BTreeMap::new(),
            graph: DependencyGraph::default(),
        }
    }

    fn load(
        &mut self,
        name: &str,
        file: &Path,
        directory: &Path,
        inline: Option<Vec<Item>>,
    ) -> Result<()> {
        let is_inline = inline.is_some();
        let items = match inline {
            Some(items) => items,
            None => {
                let source =
                    std::fs::read_to_string(file).with_context(|| file.display().to_string())?;
                let parsed =
                    syn::parse_file(&source).with_context(|| file.display().to_string())?;
                if !self.config.active(&parsed.attrs)? {
                    return Ok(());
                }
                parsed.items
            }
        };
        let mut info = ModuleInfo {
            file: file.to_owned(),
            ..ModuleInfo::default()
        };
        for item in items {
            if !self.config.active(&attributes(&item))? {
                continue;
            }
            if let Item::Mod(module) = &item {
                if !self.config.active(&module.attrs)? {
                    continue;
                }
                let child_name = format!("{name}::{}", module.ident);
                let path_attr = module
                    .attrs
                    .iter()
                    .find(|attr| attr.path().is_ident("path"));
                if module
                    .attrs
                    .iter()
                    .any(|attr| attr.path().is_ident("cfg_attr") && tokens(attr).contains("path="))
                {
                    bail!("{}: conditional #[path] is not supported", file.display());
                }
                if let Some((_, items)) = &module.content {
                    self.load(
                        &child_name,
                        file,
                        &directory.join(module.ident.to_string()),
                        Some(items.clone()),
                    )?;
                } else {
                    let child_file = if let Some(attr) = path_attr {
                        let Meta::NameValue(value) = &attr.meta else {
                            bail!("invalid path attribute");
                        };
                        let syn::Expr::Lit(value) = &value.value else {
                            bail!("invalid path attribute");
                        };
                        let syn::Lit::Str(value) = &value.lit else {
                            bail!("invalid path attribute");
                        };
                        let base = if is_inline {
                            directory
                        } else {
                            file.parent().unwrap()
                        };
                        base.join(value.value())
                    } else {
                        let plain = directory.join(format!("{}.rs", module.ident));
                        let root = directory.join(module.ident.to_string()).join("mod.rs");
                        match (plain.exists(), root.exists()) {
                            (true, false) => plain,
                            (false, true) => root,
                            _ => bail!(
                                "{}: ambiguous or missing source for {child_name}",
                                file.display()
                            ),
                        }
                    };
                    let child_directory = if path_attr.is_some()
                        || child_file.file_name().is_some_and(|name| name == "mod.rs")
                    {
                        child_file.parent().unwrap().to_owned()
                    } else {
                        child_file.with_extension("")
                    };
                    self.load(&child_name, &child_file, &child_directory, None)?;
                }
            }
            if let Item::Use(item) = &item {
                for import in imports(item).with_context(|| file.display().to_string())? {
                    info.imports.insert(import.name.clone(), import);
                }
            }
            if let Some(ident) = definition(&item) {
                info.definitions.insert(ident);
            }
            info.items.push(item);
        }
        self.graph.nodes.insert(name.to_owned());
        self.modules.insert(name.to_owned(), info);
        Ok(())
    }

    fn scan(mut self) -> Result<DependencyGraph> {
        let mut graph = DependencyGraph::default();
        for (module, info) in &self.modules {
            let mut visitor = ReferenceCollector {
                scanner: &self,
                module,
                file: &info.file,
                scopes: vec![info.imports.clone()],
                shadows: vec![BTreeSet::new()],
                edges: BTreeSet::new(),
                diagnostics: BTreeSet::new(),
                error: None,
            };
            for item in &info.items {
                if !matches!(item, Item::Mod(_)) {
                    visitor.visit_item(item);
                }
            }
            if let Some(error) = visitor.error {
                return Err(error);
            }
            graph.edges.extend(visitor.edges);
            graph.diagnostics.extend(visitor.diagnostics);
        }
        self.graph.edges = graph.edges;
        self.graph.diagnostics = graph.diagnostics;
        Ok(self.graph)
    }

    fn resolve(
        &self,
        module: &str,
        path: &[String],
        absolute: bool,
        scopes: &[BTreeMap<String, Import>],
        seen: &mut BTreeSet<String>,
    ) -> Option<ReferenceTarget> {
        let first = path.first()?;
        let root = module.split("::").next()?;
        let mut current = module.to_owned();
        let mut index = 0;
        let mut anchored = false;
        let mut owner = current.clone();
        if first == "crate" || first == root {
            current = root.to_owned();
            index = 1;
        } else if first == "self" {
            index = 1;
        } else if first == "super" {
            while path.get(index).is_some_and(|part| part == "super") {
                current = current.rsplit_once("::")?.0.to_owned();
                index += 1;
            }
        } else if absolute {
            // In edition 2024, ::foo names an external crate.
            return Some(ReferenceTarget {
                owner: None,
                module: None,
            });
        } else if let Some(import) = scopes.iter().rev().find_map(|scope| scope.get(first)) {
            let key = format!("{module}::{first}");
            if !seen.insert(key.clone()) {
                return None;
            }
            let target = self.resolve(module, &import.path, import.absolute, scopes, seen);
            seen.remove(&key);
            let target = target?;
            if path.len() == 1 || target.module.is_none() {
                return Some(target);
            }
            current = target.module?;
            owner = target.owner?;
            anchored = owner != current;
            index = 1;
        }
        if index == path.len() {
            return Some(ReferenceTarget {
                owner: Some(current.clone()),
                module: Some(current),
            });
        }
        while let Some(part) = path.get(index) {
            let candidate = format!("{current}::{part}");
            if self.modules.contains_key(&candidate) {
                current = candidate;
                if !anchored {
                    owner = current.clone();
                }
            } else {
                let info = self.modules.get(&current)?;
                if info.definitions.contains(part) {
                    return Some(ReferenceTarget {
                        owner: Some(if anchored { owner } else { current }),
                        module: None,
                    });
                }
                {
                    let Some(import) = info.imports.get(part) else {
                        // An unqualified unknown root belongs to the extern/prelude
                        // namespace. Explicit internal paths remain diagnostic.
                        return (index == 0).then_some(ReferenceTarget {
                            owner: None,
                            module: None,
                        });
                    };
                    let key = format!("{current}::{part}");
                    if !seen.insert(key.clone()) {
                        return None;
                    }
                    let target = self.resolve(
                        &current,
                        &import.path,
                        import.absolute,
                        std::slice::from_ref(&info.imports),
                        seen,
                    );
                    seen.remove(&key);
                    let target = target?;
                    // A member reached through another module is exposed by that
                    // module, even when the member is a re-exported module alias.
                    if index > 0 && current != module {
                        if !anchored {
                            owner = current.clone();
                        }
                        let Some(next) = target.module else {
                            return Some(ReferenceTarget {
                                owner: Some(owner),
                                module: None,
                            });
                        };
                        current = next;
                        anchored = true;
                    } else {
                        if path.len() == 1 || target.module.is_none() {
                            return Some(target);
                        }
                        current = target.module?;
                        owner = target.owner?;
                        anchored = owner != current;
                    }
                }
            }
            index += 1;
        }
        Some(ReferenceTarget {
            owner: Some(if anchored { owner } else { current.clone() }),
            module: Some(current),
        })
    }
}

/// The visible interface and, for module aliases, the module used to walk suffixes.
/// External references have no owner, but can be exposed by an internal façade.
struct ReferenceTarget {
    owner: Option<String>,
    module: Option<String>,
}

fn definition(item: &Item) -> Option<String> {
    Some(
        match item {
            Item::Const(item) => &item.ident,
            Item::Enum(item) => &item.ident,
            Item::Fn(item) => &item.sig.ident,
            Item::Mod(item) => &item.ident,
            Item::Static(item) => &item.ident,
            Item::Struct(item) => &item.ident,
            Item::Trait(item) => &item.ident,
            Item::TraitAlias(item) => &item.ident,
            Item::Type(item) => &item.ident,
            Item::Union(item) => &item.ident,
            Item::Macro(item) => item.ident.as_ref()?,
            _ => return None,
        }
        .to_string(),
    )
}

fn imports(item: &syn::ItemUse) -> Result<Vec<Import>> {
    fn walk(
        tree: &UseTree,
        prefix: &[String],
        absolute: bool,
        output: &mut Vec<Import>,
    ) -> Result<()> {
        match tree {
            UseTree::Path(path) => {
                let mut prefix = prefix.to_vec();
                prefix.push(path.ident.to_string());
                walk(&path.tree, &prefix, absolute, output)?;
            }
            UseTree::Group(group) => {
                for item in &group.items {
                    walk(item, prefix, absolute, output)?;
                }
            }
            UseTree::Glob(glob) => bail!(
                "line {}: unsupported wildcard import; use explicit imports",
                glob.star_token.span.start().line
            ),
            UseTree::Name(name) => {
                let mut path = prefix.to_vec();
                let name = if name.ident == "self" {
                    prefix.last().context("use self needs a prefix")?.clone()
                } else {
                    path.push(name.ident.to_string());
                    name.ident.to_string()
                };
                output.push(Import {
                    path,
                    absolute,
                    name,
                });
            }
            UseTree::Rename(rename) => {
                let mut path = prefix.to_vec();
                if rename.ident != "self" {
                    path.push(rename.ident.to_string());
                }
                output.push(Import {
                    path,
                    absolute,
                    name: rename.rename.to_string(),
                });
            }
        }
        Ok(())
    }
    let mut output = Vec::new();
    walk(&item.tree, &[], item.leading_colon.is_some(), &mut output)?;
    Ok(output)
}

struct ReferenceCollector<'a> {
    scanner: &'a DependencyScanner<'a>,
    module: &'a str,
    file: &'a Path,
    scopes: Vec<BTreeMap<String, Import>>,
    shadows: Vec<BTreeSet<String>>,
    edges: BTreeSet<(String, String)>,
    diagnostics: BTreeSet<String>,
    error: Option<anyhow::Error>,
}

impl ReferenceCollector<'_> {
    fn active(&mut self, attrs: &[Attribute]) -> bool {
        match self.scanner.config.active(attrs) {
            Ok(active) => active,
            Err(error) => {
                self.error = Some(error.context(self.file.display().to_string()));
                false
            }
        }
    }

    fn reference(&mut self, path: &[String], absolute: bool, span: Span, declaration: bool) {
        if !declaration
            && path
                .first()
                .is_some_and(|name| self.shadows.iter().rev().any(|scope| scope.contains(name)))
        {
            return;
        }
        if let Some(target) = self.scanner.resolve(
            self.module,
            path,
            absolute,
            &self.scopes,
            &mut BTreeSet::new(),
        ) {
            if let Some(owner) = target.owner
                && owner != self.module
            {
                self.edges.insert((self.module.to_owned(), owner));
            }
        } else if path
            .first()
            .is_some_and(|part| matches!(part.as_str(), "crate" | "self" | "super"))
        {
            self.diagnostics.insert(format!(
                "{}:{}: unresolved source path {}",
                self.file.display(),
                span.start().line,
                path.join("::")
            ));
        }
    }
}

impl<'ast> Visit<'ast> for ReferenceCollector<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if !self.active(&attributes(item)) {
            return;
        }
        if let Item::Mod(module) = item {
            self.error = Some(anyhow::anyhow!(
                "{}:{}: block-local modules are not supported",
                self.file.display(),
                module.ident.span().start().line
            ));
            return;
        }
        let generics = match item {
            Item::Fn(item) => Some(&item.sig.generics),
            Item::Struct(item) => Some(&item.generics),
            Item::Enum(item) => Some(&item.generics),
            Item::Union(item) => Some(&item.generics),
            Item::Type(item) => Some(&item.generics),
            Item::Trait(item) => Some(&item.generics),
            Item::Impl(item) => Some(&item.generics),
            _ => None,
        };
        self.shadows.push(type_parameters(generics));
        visit::visit_item(self, item);
        self.shadows.pop();
    }

    fn visit_attribute(&mut self, _: &'ast Attribute) {}

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        match imports(item).with_context(|| self.file.display().to_string()) {
            Ok(imports) => {
                for import in imports {
                    self.reference(&import.path, import.absolute, item.use_token.span, true);
                }
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        let parts = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>();
        if parts.len() > 1 {
            self.reference(
                &parts,
                path.leading_colon.is_some(),
                path.segments.first().unwrap().ident.span(),
                false,
            );
        }
        visit::visit_path(self, path);
    }

    fn visit_block(&mut self, block: &'ast syn::Block) {
        let mut scope = BTreeMap::new();
        for stmt in &block.stmts {
            if let syn::Stmt::Item(Item::Use(item)) = stmt
                && self.active(&item.attrs)
            {
                match imports(item) {
                    Ok(imports) => {
                        for import in imports {
                            scope.insert(import.name.clone(), import);
                        }
                    }
                    Err(error) => self.error = Some(error.context(self.file.display().to_string())),
                }
            }
        }
        self.scopes.push(scope);
        let local_types = block
            .stmts
            .iter()
            .filter_map(|stmt| match stmt {
                syn::Stmt::Item(
                    item @ (Item::Struct(_)
                    | Item::Enum(_)
                    | Item::Union(_)
                    | Item::Type(_)
                    | Item::Trait(_)),
                ) => definition(item),
                _ => None,
            })
            .collect();
        self.shadows.push(local_types);
        visit::visit_block(self, block);
        self.scopes.pop();
        self.shadows.pop();
    }

    fn visit_expr(&mut self, expr: &'ast syn::Expr) {
        if self.active(&attributes(expr)) {
            visit::visit_expr(self, expr);
        }
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        if !self.active(&local.attrs) {
            return;
        }
        visit::visit_local(self, local);
    }

    fn visit_field(&mut self, field: &'ast syn::Field) {
        if self.active(&field.attrs) {
            visit::visit_field(self, field);
        }
    }

    fn visit_variant(&mut self, variant: &'ast syn::Variant) {
        if self.active(&variant.attrs) {
            visit::visit_variant(self, variant);
        }
    }

    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        if self.active(&attributes(item)) {
            let generics = match item {
                syn::ImplItem::Fn(item) => Some(&item.sig.generics),
                syn::ImplItem::Type(item) => Some(&item.generics),
                _ => None,
            };
            self.shadows.push(type_parameters(generics));
            visit::visit_impl_item(self, item);
            self.shadows.pop();
        }
    }

    fn visit_trait_item(&mut self, item: &'ast syn::TraitItem) {
        if self.active(&attributes(item)) {
            let generics = match item {
                syn::TraitItem::Fn(item) => Some(&item.sig.generics),
                syn::TraitItem::Type(item) => Some(&item.generics),
                _ => None,
            };
            self.shadows.push(type_parameters(generics));
            visit::visit_trait_item(self, item);
            self.shadows.pop();
        }
    }
}

fn type_parameters(generics: Option<&syn::Generics>) -> BTreeSet<String> {
    generics
        .into_iter()
        .flat_map(|generics| generics.type_params())
        .map(|param| param.ident.to_string())
        .collect()
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("--cfg-file") {
        bail!("expected --cfg-file FILE followed by ROOT NAME ENTRY triples");
    }
    let cfg = std::fs::read_to_string(args.next().context("missing cfg file")?)?;
    let config = BuildConfig {
        flags: cfg.lines().map(|line| line.replace(' ', "")).collect(),
    };
    let mut scanner = DependencyScanner::new(&config);
    let mut count = 0;
    while let Some(root) = args.next() {
        let name = args.next().context("missing crate name")?;
        let entry = PathBuf::from(root).join(args.next().context("missing entry file")?);
        scanner.load(&name, &entry, entry.parent().unwrap(), None)?;
        count += 1;
    }
    if count == 0 {
        bail!("at least one crate is required");
    }
    let graph = scanner.scan()?;
    for diagnostic in &graph.diagnostics {
        eprintln!("warning: {diagnostic}");
    }
    println!("{}", serde_json::to_string_pretty(&graph)?);
    Ok(())
}

#[cfg(test)]
mod tests;
