//! Format compiler-resolved rustdoc types; no Rust parsing or name resolution.
use anyhow::{Result, bail, ensure};
use rustdoc_types::*;

fn joined<T>(items: &[T], separator: &str, f: impl Fn(&T) -> String) -> String {
    items.iter().map(f).collect::<Vec<_>>().join(separator)
}

fn angle(text: String) -> String {
    if text.is_empty() {
        text
    } else {
        format!("<{text}>")
    }
}

fn args(args: &Option<Box<GenericArgs>>) -> String {
    match args.as_deref() {
        None => String::new(),
        Some(GenericArgs::ReturnTypeNotation) => "(..)".into(),
        Some(GenericArgs::Parenthesized { inputs, output }) => {
            format!("({}){}", joined(inputs, ", ", ty), return_type(output))
        }
        Some(GenericArgs::AngleBracketed { args, constraints }) => {
            let mut parts: Vec<_> = args
                .iter()
                .map(|arg| match arg {
                    GenericArg::Lifetime(s) => s.clone(),
                    GenericArg::Type(t) => ty(t),
                    GenericArg::Const(c) => c.expr.clone(),
                    GenericArg::Infer => "_".into(),
                })
                .collect();
            parts.extend(constraints.iter().map(|c| {
                format!(
                    "{}{}{}",
                    c.name,
                    self::args(&c.args),
                    match &c.binding {
                        AssocItemConstraintKind::Equality(t) => format!(" = {}", term(t)),
                        AssocItemConstraintKind::Constraint(b) => format!(": {}", bounds(b)),
                    }
                )
            }));
            angle(parts.join(", "))
        }
    }
}

fn path(p: &Path) -> String {
    format!("{}{}", p.path, args(&p.args))
}

fn term(t: &Term) -> String {
    match t {
        Term::Type(t) => ty(t),
        Term::Constant(c) => c.expr.clone(),
    }
}

fn bounds(b: &[GenericBound]) -> String {
    joined(b, " + ", |b| match b {
        GenericBound::Outlives(s) => s.clone(),
        GenericBound::Use(args) => format!(
            "use<{}>",
            joined(args, ", ", |arg| match arg {
                PreciseCapturingArg::Lifetime(s) | PreciseCapturingArg::Param(s) => s.clone(),
            })
        ),
        GenericBound::TraitBound {
            trait_,
            generic_params,
            modifier,
        } => format!(
            "{}{}{}",
            binder(generic_params),
            match modifier {
                TraitBoundModifier::None => "",
                TraitBoundModifier::Maybe => "?",
                TraitBoundModifier::MaybeConst => "~const ",
            },
            path(trait_)
        ),
    })
}

fn params(p: &[GenericParamDef]) -> String {
    angle(
        p.iter()
            .filter_map(|p| {
                let suffix = match &p.kind {
                    GenericParamDefKind::Lifetime { outlives } => {
                        if outlives.is_empty() {
                            String::new()
                        } else {
                            format!(": {}", outlives.join(" + "))
                        }
                    }
                    GenericParamDefKind::Type {
                        bounds: b,
                        default,
                        is_synthetic,
                    } => {
                        if *is_synthetic {
                            return None;
                        }
                        format!(
                            "{}{}",
                            if b.is_empty() {
                                String::new()
                            } else {
                                format!(": {}", bounds(b))
                            },
                            default
                                .as_ref()
                                .map(|t| format!(" = {}", ty(t)))
                                .unwrap_or_default()
                        )
                    }
                    GenericParamDefKind::Const { type_, default } => {
                        return Some(format!(
                            "const {}: {}{}",
                            p.name,
                            ty(type_),
                            default
                                .as_ref()
                                .map(|d| format!(" = {d}"))
                                .unwrap_or_default()
                        ));
                    }
                };
                Some(format!("{}{suffix}", p.name))
            })
            .collect::<Vec<_>>()
            .join(", "),
    )
}

fn binder(p: &[GenericParamDef]) -> String {
    let p = params(p);
    if p.is_empty() { p } else { format!("for{p} ") }
}

fn predicates(g: &Generics) -> String {
    if g.where_predicates.is_empty() {
        return String::new();
    }
    format!(
        "\nwhere\n    {}",
        joined(&g.where_predicates, ",\n    ", |p| match p {
            WherePredicate::BoundPredicate {
                type_,
                bounds: b,
                generic_params,
            } => format!("{}{}: {}", binder(generic_params), ty(type_), bounds(b)),
            WherePredicate::LifetimePredicate { lifetime, outlives } =>
                format!("{lifetime}: {}", outlives.join(" + ")),
            WherePredicate::EqPredicate { lhs, rhs } => format!("{} = {}", ty(lhs), term(rhs)),
        })
    )
}

fn header(h: &FunctionHeader) -> String {
    let abi = match &h.abi {
        Abi::Rust => String::new(),
        Abi::Other(s) => format!("extern {s:?} "),
        a => {
            let (name, unwind) = match a {
                Abi::C { unwind } => ("C", unwind),
                Abi::Cdecl { unwind } => ("cdecl", unwind),
                Abi::Stdcall { unwind } => ("stdcall", unwind),
                Abi::Fastcall { unwind } => ("fastcall", unwind),
                Abi::Aapcs { unwind } => ("aapcs", unwind),
                Abi::Win64 { unwind } => ("win64", unwind),
                Abi::SysV64 { unwind } => ("sysv64", unwind),
                Abi::System { unwind } => ("system", unwind),
                Abi::Rust | Abi::Other(_) => unreachable!(),
            };
            format!("extern \"{name}{}\" ", if *unwind { "-unwind" } else { "" })
        }
    };
    format!(
        "{}{}{}{abi}",
        if h.is_const { "const " } else { "" },
        if h.is_async { "async " } else { "" },
        if h.is_unsafe { "unsafe " } else { "" }
    )
}

fn return_type(output: &Option<Type>) -> String {
    output
        .as_ref()
        .map(|t| format!(" -> {}", ty(t)))
        .unwrap_or_default()
}

fn signature(sig: &FunctionSignature, names: bool) -> String {
    let mut inputs: Vec<_> = sig
        .inputs
        .iter()
        .map(|(name, t)| {
            if names {
                format!("{name}: {}", ty(t))
            } else {
                ty(t)
            }
        })
        .collect();
    if sig.is_c_variadic {
        inputs.push("...".into());
    }
    format!("({}){}", inputs.join(", "), return_type(&sig.output))
}

fn ty(t: &Type) -> String {
    match t {
        Type::ResolvedPath(p) => path(p),
        Type::Generic(s) | Type::Primitive(s) => s.clone(),
        Type::Infer => "_".into(),
        Type::Tuple(types) => format!(
            "({}{})",
            joined(types, ", ", ty),
            if types.len() == 1 { "," } else { "" }
        ),
        Type::Slice(t) => format!("[{}]", ty(t)),
        Type::Array { type_, len } => format!("[{}; {len}]", ty(type_)),
        Type::Pat {
            type_,
            __pat_unstable_do_not_use,
        } => format!("{} is {__pat_unstable_do_not_use}", ty(type_)),
        Type::ImplTrait(b) => format!("impl {}", bounds(b)),
        Type::DynTrait(d) => {
            let mut b = d
                .traits
                .iter()
                .map(|p| format!("{}{}", binder(&p.generic_params), path(&p.trait_)))
                .collect::<Vec<_>>();
            b.extend(d.lifetime.iter().cloned());
            format!("dyn {}", b.join(" + "))
        }
        Type::BorrowedRef {
            lifetime,
            is_mutable,
            type_,
        } => format!(
            "&{}{}{}",
            lifetime
                .as_ref()
                .map(|s| format!("{s} "))
                .unwrap_or_default(),
            if *is_mutable { "mut " } else { "" },
            pointee(type_)
        ),
        Type::RawPointer { is_mutable, type_ } => format!(
            "*{} {}",
            if *is_mutable { "mut" } else { "const" },
            pointee(type_)
        ),
        Type::FunctionPointer(f) => format!(
            "{}{}fn{}",
            binder(&f.generic_params),
            header(&f.header),
            signature(&f.sig, false)
        ),
        Type::QualifiedPath {
            name,
            args: a,
            self_type,
            trait_,
        } => {
            let base = match trait_ {
                Some(p) if !p.path.is_empty() => format!("<{} as {}>", ty(self_type), path(p)),
                // rustdoc uses an empty trait path for shorthand such as T::Item.
                _ => format!("<{}>", ty(self_type)),
            };
            format!("{base}::{name}{}", args(a))
        }
    }
}

fn pointee(t: &Type) -> String {
    match t {
        Type::DynTrait(_) | Type::ImplTrait(_) => format!("({})", ty(t)),
        _ => ty(t),
    }
}

fn field(krate: &Crate, id: Id, named: bool, vis: bool) -> Result<String> {
    let item = crate::extract::item(krate, id)?;
    let ItemEnum::StructField(t) = &item.inner else {
        bail!("expected field at {id:?}")
    };
    let visibility = if vis && item.visibility != Visibility::Default {
        format!("{} ", crate::extract::visibility(&item.visibility))
    } else {
        String::new()
    };
    let name = if named {
        format!("{}: ", item.name.as_deref().unwrap_or("_"))
    } else {
        String::new()
    };
    Ok(format!("{visibility}{name}{}", ty(t)))
}

fn tuple_fields(krate: &Crate, fields: &[Option<Id>], vis: bool) -> Result<String> {
    Ok(format!(
        "({})",
        fields
            .iter()
            .map(|id| {
                field(
                    krate,
                    id.ok_or_else(|| {
                        anyhow::anyhow!("stripped tuple field; use --document-hidden-items")
                    })?,
                    false,
                    vis,
                )
            })
            .collect::<Result<Vec<_>>>()?
            .join(", ")
    ))
}

fn named_fields(krate: &Crate, fields: &[Id], stripped: bool, vis: bool) -> Result<String> {
    ensure!(
        !stripped,
        "stripped fields; use --document-private-items --document-hidden-items"
    );
    Ok(format!(
        " {{\n    {}\n}}",
        fields
            .iter()
            .map(|id| field(krate, *id, true, vis))
            .collect::<Result<Vec<_>>>()?
            .join(",\n    ")
    ))
}

pub fn item_signature(krate: &Crate, item: &Item, name: &str) -> Result<(&'static str, String)> {
    // The caller displays re-export visibility separately; this is the target declaration.
    let vis = crate::extract::visibility(&item.visibility);
    let result = match &item.inner {
        ItemEnum::Function(f) => (
            "Functions",
            format!(
                "{vis} {}fn {name}{}{}{}",
                header(&f.header),
                params(&f.generics.params),
                signature(&f.sig, true),
                predicates(&f.generics)
            ),
        ),
        ItemEnum::Struct(s) => {
            let base = format!("{vis} struct {name}{}", params(&s.generics.params));
            let declaration = match &s.kind {
                StructKind::Unit => format!("{base}{};", predicates(&s.generics)),
                StructKind::Tuple(fields) => format!(
                    "{base}{}{};",
                    tuple_fields(krate, fields, true)?,
                    predicates(&s.generics)
                ),
                StructKind::Plain {
                    fields,
                    has_stripped_fields,
                } => format!(
                    "{base}{}{}",
                    predicates(&s.generics),
                    named_fields(krate, fields, *has_stripped_fields, true)?
                ),
            };
            ("Structs", declaration)
        }
        ItemEnum::Enum(e) => {
            ensure!(
                !e.has_stripped_variants,
                "stripped variants; use --document-hidden-items"
            );
            let variants = e
                .variants
                .iter()
                .map(|id| {
                    let v = crate::extract::item(krate, *id)?;
                    let ItemEnum::Variant(data) = &v.inner else {
                        bail!("expected variant")
                    };
                    let body = match &data.kind {
                        VariantKind::Plain => String::new(),
                        VariantKind::Tuple(fields) => tuple_fields(krate, fields, false)?,
                        VariantKind::Struct {
                            fields,
                            has_stripped_fields,
                        } => named_fields(krate, fields, *has_stripped_fields, false)?,
                    };
                    Ok(format!(
                        "{}{body}{}",
                        v.name.as_deref().unwrap_or("_"),
                        data.discriminant
                            .as_ref()
                            .map(|d| format!(" = {}", d.expr))
                            .unwrap_or_default()
                    ))
                })
                .collect::<Result<Vec<_>>>()?
                .join(",\n")
                .replace('\n', "\n    ");
            (
                "Enums",
                format!(
                    "{vis} enum {name}{}{} {{\n    {variants}\n}}",
                    params(&e.generics.params),
                    predicates(&e.generics)
                ),
            )
        }
        ItemEnum::TypeAlias(a) => (
            "Type aliases",
            format!(
                "{vis} type {name}{}{} = {};",
                params(&a.generics.params),
                predicates(&a.generics),
                ty(&a.type_)
            ),
        ),
        ItemEnum::Constant { type_, const_ } => (
            "Constants",
            format!("{vis} const {name}: {} = {};", ty(type_), const_.expr),
        ),
        _ => bail!(
            "unsupported facade item kind for {name}; extend the renderer before exposing this item"
        ),
    };
    Ok(result)
}
