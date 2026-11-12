//! `validate_at` bodies: one `if` per rule, no allocation unless it fails.

use core::fmt::Write as _;

use korek_gen_common::{rust_ident, unsupported};
use korek_ir::naming::pascal;
use korek_ir::{
    Diagnostic, Format, Json, Rule, RuleKind, Type, TypeDef, TypeIndex, TypeKind, Types, Validate,
};

use crate::RT;

/// The JSON text of a rule's operand (`""` for none), for key metadata.
pub(crate) fn operand_json(k: &RuleKind) -> String {
    match k {
        RuleKind::Const { value }
        | RuleKind::Gt { value }
        | RuleKind::Gte { value }
        | RuleKind::Lt { value }
        | RuleKind::Lte { value }
        | RuleKind::MultipleOf { value } => value.to_string(),
        RuleKind::In { values } | RuleKind::NotIn { values } => {
            Json::Array(values.clone()).to_string()
        }
        RuleKind::Len { n }
        | RuleKind::MinLen { n }
        | RuleKind::MaxLen { n }
        | RuleKind::LenBytes { n }
        | RuleKind::MinBytes { n }
        | RuleKind::MaxBytes { n }
        | RuleKind::MinItems { n }
        | RuleKind::MaxItems { n }
        | RuleKind::MinPairs { n }
        | RuleKind::MaxPairs { n } => n.to_string(),
        RuleKind::Pattern { regex } => Json::from(regex.as_str()).to_string(),
        RuleKind::Prefix { value }
        | RuleKind::Suffix { value }
        | RuleKind::Contains { value }
        | RuleKind::NotContains { value } => Json::from(value.as_str()).to_string(),
        RuleKind::Within { duration } => Json::from(duration.as_str()).to_string(),
        RuleKind::Cel { expression, .. } => Json::from(expression.as_str()).to_string(),
        _ => String::new(),
    }
}

fn nested(inner: Option<&Validate>) -> Option<&Validate> {
    inner.filter(|x| !x.is_empty())
}

fn contains_named(ty: &Type) -> bool {
    match ty {
        Type::Named(_) => true,
        Type::List(t) | Type::Map(t) => contains_named(t),
        _ => false,
    }
}

/// The body of `validate_at` for a type.
pub(crate) fn type_body(
    schema: &TypeIndex<'_>,
    t: &TypeDef,
    out: &mut String,
    warnings: &mut Vec<Diagnostic>,
) {
    match &t.kind {
        TypeKind::Enum(_) => {}
        TypeKind::Struct(s) => {
            for f in &s.fields {
                let mut checks = String::new();
                value_checks(schema, &f.ty, &f.validate, "v", &mut checks, 3, warnings);
                if contains_named(&f.ty) {
                    let _ = writeln!(
                        checks,
                        "            {RT}::Validate::validate_at(v, &path, errors);"
                    );
                }
                let required = f.optional
                    && f.default.is_none()
                    && f.validate.has(|k| matches!(k, RuleKind::Required));
                if checks.is_empty() && !required {
                    continue;
                }
                let name = rust_ident(&f.name);
                let _ = writeln!(
                    out,
                    "        {{\n            let path = path.field({:?});",
                    f.name
                );
                if f.optional && f.default.is_none() {
                    if required {
                        let msg = message(
                            f.validate
                                .rules
                                .iter()
                                .find(|r| matches!(r.kind, RuleKind::Required)),
                            "is required",
                        );
                        let _ = writeln!(
                            out,
                            "            if self.{name}.is_none() {{\n                errors.push(&path, \"required\", {msg});\n            }}"
                        );
                    }
                    if !checks.is_empty() {
                        let _ = writeln!(
                            out,
                            "            if let Some(v) = &self.{name} {{\n{}            }}",
                            indent(&checks)
                        );
                    }
                } else {
                    let _ = writeln!(out, "            let v = &self.{name};\n{checks}");
                }
                out.push_str("        }\n");
            }
        }
        TypeKind::Union(u) => {
            let arms: Vec<String> = u
                .variants
                .iter()
                .filter(|v| contains_named(&v.ty))
                .map(|v| {
                    format!(
                        "            Self::{}(v) => {RT}::Validate::validate_at(v, path, errors),",
                        pascal(&v.name)
                    )
                })
                .collect();
            if !arms.is_empty() {
                let rest = if arms.len() < u.variants.len() {
                    "            _ => {}\n"
                } else {
                    ""
                };
                let _ = writeln!(
                    out,
                    "        match self {{\n{}\n{rest}        }}",
                    arms.join("\n")
                );
            }
        }
    }
    for r in &t.validate.rules {
        match &r.kind {
            RuleKind::Cel {
                rust: Some(expr),
                id,
                ..
            } => {
                let msg = message(Some(r), &format!("failed {id}"));
                let _ = writeln!(
                    out,
                    "        {{\n            let v = self;\n            if !({expr}) {{\n                errors.push(path, \"cel\", {msg});\n            }}\n        }}"
                );
            }
            RuleKind::Cel { .. } => warnings.push(unsupported(
                "rust",
                r,
                "a CEL rule needs `rust=\"…\"` for Rust",
            )),
            _ => {}
        }
    }
}

fn indent(s: &str) -> String {
    s.lines().fold(String::new(), |mut o, l| {
        let _ = writeln!(o, "    {l}");
        o
    })
}

/// A Rust string literal for a rule's message.
fn message(r: Option<&Rule>, default: &str) -> String {
    format!(
        "{:?}",
        r.and_then(|r| r.message.clone())
            .unwrap_or_else(|| default.to_owned())
    )
}

fn num_lit(ty: &Type, j: &Json) -> String {
    if ty.is_float() {
        format!("{:?}_f64", j.as_f64().unwrap_or_default())
    } else {
        j.to_string()
    }
}

/// Statements checking `var` (a `&T`) against `v`, with `path` and `errors`
/// in scope.
#[allow(clippy::cognitive_complexity)]
pub(crate) fn value_checks(
    schema: &TypeIndex<'_>,
    ty: &Type,
    v: &Validate,
    var: &str,
    out: &mut String,
    depth: usize,
    warnings: &mut Vec<Diagnostic>,
) {
    let pad = "    ".repeat(depth);
    let is_enum = matches!(ty, Type::Named(n) if matches!(schema.get(n).map(|t| &t.kind), Some(TypeKind::Enum(_))));
    for r in &v.rules {
        let Some((cond, default)) = condition(ty, &r.kind, var, is_enum) else {
            match &r.kind {
                RuleKind::Required | RuleKind::DefinedOnly | RuleKind::Finite
                    if !ty.is_float() || !matches!(r.kind, RuleKind::Finite) => {}
                RuleKind::Cel { .. } => warnings.push(unsupported(
                    "rust",
                    r,
                    "a CEL rule needs `rust=\"…\"` for Rust",
                )),
                _ => warnings.push(unsupported("rust", r, "not supported for this type")),
            }
            continue;
        };
        let msg = message(Some(r), &default);
        let _ = writeln!(
            out,
            "{pad}if {cond} {{\n{pad}    errors.push(&path, {:?}, {msg});\n{pad}}}",
            r.kind.name()
        );
    }
    match ty {
        Type::List(t) => {
            if let Some(items) = nested(v.items.as_deref()) {
                let mut body = String::new();
                value_checks(schema, t, items, "v", &mut body, depth + 1, warnings);
                let _ = writeln!(
                    out,
                    "{pad}for (i, v) in {var}.iter().enumerate() {{\n{pad}    let path = path.index(i);\n{body}{pad}}}"
                );
            }
        }
        Type::Map(t) => {
            let (keys, values) = (nested(v.keys.as_deref()), nested(v.values.as_deref()));
            if keys.is_some() || values.is_some() {
                let mut body = String::new();
                if let Some(k) = keys {
                    let mut kb = String::new();
                    value_checks(schema, &Type::String, k, "k", &mut kb, depth + 1, warnings);
                    body.push_str(&kb);
                }
                if let Some(vals) = values {
                    value_checks(schema, t, vals, "v", &mut body, depth + 1, warnings);
                }
                let _ = writeln!(
                    out,
                    "{pad}for (k, v) in {var} {{\n{pad}    let path = path.key(k);\n{pad}    let _ = (&k, &v);\n{body}{pad}}}"
                );
            }
        }
        _ => {}
    }
}

/// The failure condition and default message of a rule, or `None` when it
/// is always satisfied or not expressible.
#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
fn condition(ty: &Type, k: &RuleKind, var: &str, is_enum: bool) -> Option<(String, String)> {
    let v = var;
    let is_bytes = *ty == Type::Bytes;
    let len = if is_bytes || matches!(ty, Type::List(_) | Type::Map(_)) {
        format!("{v}.len()")
    } else {
        format!("{v}.chars().count()")
    };
    let ts = |f: &str| match ty {
        Type::Timestamp => format!("{RT}::time::parse_timestamp({f})"),
        _ => format!("{RT}::time::parse_duration({f})"),
    };
    let bytes_lit = |j: &Json| {
        format!(
            "&{:?}[..]",
            j.as_str()
                .and_then(korek_runtime::base64::decode)
                .unwrap_or_default()
        )
    };
    let enum_pat = |j: &Json| match ty {
        Type::Named(n) => format!("{n}::{}", pascal(j.as_str().unwrap_or(""))),
        _ => String::new(),
    };
    let lit = |j: &Json| match ty {
        Type::String | Type::Timestamp | Type::Duration => {
            format!("{:?}", j.as_str().unwrap_or(""))
        }
        Type::Bytes => bytes_lit(j),
        _ if is_enum => enum_pat(j),
        _ => num_lit(ty, j),
    };
    let text = if is_bytes {
        format!("String::from_utf8_lossy({v})")
    } else {
        format!("{v}.as_str()")
    };
    Some(match k {
        RuleKind::Const { value } if is_enum => (
            format!("!matches!({v}, {})", enum_pat(value)),
            format!("must be {value}"),
        ),
        RuleKind::Const { value } if is_bytes => (
            format!("{v}.as_slice() != {}", bytes_lit(value)),
            "must equal the constant".into(),
        ),
        RuleKind::Const { value }
            if matches!(ty, Type::String | Type::Timestamp | Type::Duration) =>
        {
            (format!("{v} != {}", lit(value)), format!("must be {value}"))
        }
        RuleKind::Const { value } => (
            format!("*{v} != {}", lit(value)),
            format!("must be {value}"),
        ),
        RuleKind::In { values } | RuleKind::NotIn { values } => {
            let neg = matches!(k, RuleKind::In { .. });
            let cond = if is_enum {
                format!(
                    "matches!({v}, {})",
                    values.iter().map(enum_pat).collect::<Vec<_>>().join(" | ")
                )
            } else if is_bytes {
                format!(
                    "[{}].contains(&{v}.as_slice())",
                    values.iter().map(bytes_lit).collect::<Vec<_>>().join(", ")
                )
            } else if matches!(ty, Type::String | Type::Duration) {
                format!(
                    "[{}].contains(&{v}.as_str())",
                    values.iter().map(&lit).collect::<Vec<_>>().join(", ")
                )
            } else {
                format!(
                    "[{}].contains({v})",
                    values.iter().map(&lit).collect::<Vec<_>>().join(", ")
                )
            };
            let all = Json::Array(values.clone());
            if neg {
                (format!("!{cond}"), format!("must be one of {all}"))
            } else {
                (cond, format!("must not be one of {all}"))
            }
        }
        RuleKind::Len { n } => (format!("{len} != {n}"), format!("length must be {n}")),
        RuleKind::MinLen { n } => (
            format!("{len} < {n}"),
            format!("length must be at least {n}"),
        ),
        RuleKind::MaxLen { n } => (
            format!("{len} > {n}"),
            format!("length must be at most {n}"),
        ),
        RuleKind::LenBytes { n } => (format!("{v}.len() != {n}"), format!("must be {n} bytes")),
        RuleKind::MinBytes { n } => (
            format!("{v}.len() < {n}"),
            format!("must be at least {n} bytes"),
        ),
        RuleKind::MaxBytes { n } => (
            format!("{v}.len() > {n}"),
            format!("must be at most {n} bytes"),
        ),
        RuleKind::Pattern { regex } => (
            format!(
                "{{ static RE: ::std::sync::OnceLock<{RT}::regex::Regex> = ::std::sync::OnceLock::new(); !{RT}::matches(&RE, {regex:?}, &{text}) }}"
            ),
            format!("must match {regex:?}"),
        ),
        RuleKind::Prefix { value } if is_bytes => (
            format!("!{v}.starts_with({value:?}.as_bytes())"),
            format!("must start with {value:?}"),
        ),
        RuleKind::Suffix { value } if is_bytes => (
            format!("!{v}.ends_with({value:?}.as_bytes())"),
            format!("must end with {value:?}"),
        ),
        RuleKind::Contains { value } if is_bytes => (
            format!("!{text}.contains({value:?})"),
            format!("must contain {value:?}"),
        ),
        RuleKind::Prefix { value } => (
            format!("!{v}.starts_with({value:?})"),
            format!("must start with {value:?}"),
        ),
        RuleKind::Suffix { value } => (
            format!("!{v}.ends_with({value:?})"),
            format!("must end with {value:?}"),
        ),
        RuleKind::Contains { value } => (
            format!("!{v}.contains({value:?})"),
            format!("must contain {value:?}"),
        ),
        RuleKind::NotContains { value } => (
            format!("{v}.contains({value:?})"),
            format!("must not contain {value:?}"),
        ),
        RuleKind::Format { format } if is_bytes => {
            let ok = match format {
                Format::Ipv4 => "== 4",
                Format::Ipv6 => "== 16",
                _ => "!= usize::MAX",
            };
            let cond = if *format == Format::Ip {
                format!("!matches!({v}.len(), 4 | 16)")
            } else {
                format!("!({v}.len() {ok})")
            };
            (cond, format!("must be a valid {}", format.name()))
        }
        RuleKind::Format { format } => (
            format!("!{RT}::format::{}({v})", format.name()),
            format!("must be a valid {}", format.name()),
        ),
        RuleKind::Gt { value }
        | RuleKind::Gte { value }
        | RuleKind::Lt { value }
        | RuleKind::Lte { value } => {
            let (op, word) = match k {
                RuleKind::Gt { .. } => (">", "greater than"),
                RuleKind::Gte { .. } => (">=", "at least"),
                RuleKind::Lt { .. } => ("<", "less than"),
                _ => ("<=", "at most"),
            };
            if matches!(ty, Type::Timestamp | Type::Duration) {
                let bound = value.as_str().and_then(|s| match ty {
                    Type::Timestamp => korek_runtime::time::parse_timestamp(s),
                    _ => korek_runtime::time::parse_duration(s),
                })?;
                (
                    format!("{}.is_some_and(|x| !(x {op} {bound:?}))", ts(v)),
                    format!("must be {word} {value}"),
                )
            } else {
                (
                    format!("!(*{v} {op} {})", num_lit(ty, value)),
                    format!("must be {word} {value}"),
                )
            }
        }
        RuleKind::Finite if ty.is_float() => (format!("!{v}.is_finite()"), "must be finite".into()),
        RuleKind::MultipleOf { value } if ty.is_float() => {
            let m = num_lit(ty, value);
            (
                format!("{{ let q = f64::from(*{v}) / {m}; (q - q.round()).abs() > 1e-9 }}"),
                format!("must be a multiple of {value}"),
            )
        }
        RuleKind::MultipleOf { value } => (
            format!("*{v} % {} != 0", num_lit(ty, value)),
            format!("must be a multiple of {value}"),
        ),
        RuleKind::MinItems { n } | RuleKind::MinPairs { n } => (
            format!("{v}.len() < {n}"),
            format!("must have at least {n} entries"),
        ),
        RuleKind::MaxItems { n } | RuleKind::MaxPairs { n } => (
            format!("{v}.len() > {n}"),
            format!("must have at most {n} entries"),
        ),
        RuleKind::Unique => (format!("!{RT}::unique({v})"), "items must be unique".into()),
        RuleKind::GtNow => (
            format!("{}.is_some_and(|x| x <= {RT}::time::now())", ts(v)),
            "must be in the future".into(),
        ),
        RuleKind::LtNow => (
            format!("{}.is_some_and(|x| x >= {RT}::time::now())", ts(v)),
            "must be in the past".into(),
        ),
        RuleKind::Within { duration } => {
            let d = korek_runtime::time::parse_duration(duration)?;
            (
                format!(
                    "{}.is_some_and(|x| (x - {RT}::time::now()).abs() > {d:?})",
                    ts(v)
                ),
                format!("must be within {duration} of now"),
            )
        }
        RuleKind::Cel {
            rust: Some(expr),
            id,
            ..
        } => (
            format!("!{{ let v = {v}; {expr} }}"),
            format!("failed {id}"),
        ),
        _ => return None,
    })
}
