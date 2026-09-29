use anyhow::{Context, Result, bail, ensure};
use rustdoc_types::{Crate, Id, Item, ItemEnum, Visibility};
use std::collections::HashSet;

pub struct Interface {
    pub name: String,
    pub kind: &'static str,
    pub signature: String,
    pub visibility: String,
    pub source: String,
}

pub struct Facade {
    pub path: String,
    pub items: Vec<Interface>,
}

pub fn item(krate: &Crate, id: Id) -> Result<&Item> {
    krate.index.get(&id).with_context(|| {
        format!(
            "missing item {id:?}; external re-exports require dependency JSON and are not supported"
        )
    })
}

pub fn visibility(vis: &Visibility) -> String {
    match vis {
        Visibility::Public => "pub".into(),
        Visibility::Default => "private".into(),
        Visibility::Crate => "pub(crate)".into(),
        Visibility::Restricted { path, .. } => {
            let path = if path.starts_with("::") {
                format!("crate{path}")
            } else {
                path.clone()
            };
            format!("pub(in {path})")
        }
    }
}

// Follow compiler-resolved IDs, never source paths or Rust import syntax.
fn resolve(krate: &Crate, mut id: Id) -> Result<&Item> {
    let mut seen = HashSet::new();
    loop {
        ensure!(seen.insert(id), "cyclic re-export at {id:?}");
        let target = item(krate, id)?;
        match &target.inner {
            ItemEnum::Use(import) => {
                ensure!(
                    !import.is_glob,
                    "glob re-exports are unsupported; list interface items explicitly"
                );
                id = import.id.context("re-export has no resolved item ID")?;
            }
            _ => return Ok(target),
        }
    }
}

pub fn extract(krate: &Crate) -> Result<Vec<Facade>> {
    let mut facades = Vec::new();
    visit(krate, krate.root, "", &mut facades)?;
    facades.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(facades)
}

fn visit(krate: &Crate, id: Id, path: &str, facades: &mut Vec<Facade>) -> Result<()> {
    let module = item(krate, id)?;
    let ItemEnum::Module(module_data) = &module.inner else {
        bail!("expected module at {id:?}")
    };
    let mut entries = Vec::new();
    for child_id in &module_data.items {
        let child = item(krate, *child_id)?;
        match &child.inner {
            ItemEnum::Module(_) => {
                let name = child.name.as_deref().context("unnamed module")?;
                let next = if path.is_empty() {
                    name.into()
                } else {
                    format!("{path}::{name}")
                };
                visit(krate, *child_id, &next, facades)?;
            }
            ItemEnum::Use(import) if child.visibility != Visibility::Default => {
                ensure!(
                    !import.is_glob,
                    "{path}: glob re-export {} is unsupported; list interface items explicitly",
                    import.source
                );
                let target = resolve(krate, import.id.context("re-export has no target ID")?)?;
                let (kind, signature) =
                    crate::signature::item_signature(krate, target, &import.name)
                        .with_context(|| format!("rendering {path}::{}", import.name))?;
                let span = target
                    .span
                    .as_ref()
                    .context("re-export target has no source span")?;
                entries.push(Interface {
                    name: import.name.clone(),
                    kind,
                    signature,
                    visibility: visibility(&child.visibility),
                    source: format!("{}:{}", span.filename.display(), span.begin.0),
                });
            }
            _ => {}
        }
    }
    if !entries.is_empty() {
        entries.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
        let path = if path.is_empty() {
            module.name.clone().context("unnamed crate")?
        } else {
            path.into()
        };
        facades.push(Facade {
            path,
            items: entries,
        });
    }
    Ok(())
}
