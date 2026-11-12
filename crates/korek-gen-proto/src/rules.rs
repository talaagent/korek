//! korek rules → protovalidate (`buf.validate`) field rules, as one
//! text-format aggregate per field.

use core::fmt::Write as _;

use korek_gen_common::{format_regex, unsupported};
use korek_ir::{Diagnostic, Format, Json, RuleKind, Type, TypeKind, Validate};

use crate::quote;

/// A protovalidate rule tree: `required: true`, `string: {min_len: 1}`.
#[derive(Default)]
pub(crate) struct Tree {
    entries: Vec<(String, Entry)>,
}

enum Entry {
    Value(String),
    Tree(Tree),
}

impl Tree {
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn set(&mut self, key: &str, value: String) {
        self.entries.push((key.to_owned(), Entry::Value(value)));
    }

    #[allow(clippy::single_match_else)]
    fn sub(&mut self, key: &str) -> &mut Tree {
        let i = match self
            .entries
            .iter()
            .position(|(k, e)| k == key && matches!(e, Entry::Tree(_)))
        {
            Some(i) => i,
            None => {
                self.entries
                    .push((key.to_owned(), Entry::Tree(Tree::default())));
                self.entries.len() - 1
            }
        };
        match &mut self.entries[i].1 {
            Entry::Tree(t) => t,
            Entry::Value(_) => unreachable!(),
        }
    }

    /// Text format: `{required: true, string: {min_len: 1}}`.
    pub(crate) fn render(&self) -> String {
        let parts: Vec<String> = self
            .entries
            .iter()
            .map(|(k, e)| match e {
                Entry::Value(v) => format!("{k}: {v}"),
                Entry::Tree(t) => format!("{k}: {}", t.render()),
            })
            .collect();
        format!("{{{}}}", parts.join(", "))
    }
}

fn kind_name(ty: &Type, kind: Option<&TypeKind>) -> &'static str {
    match ty {
        Type::String => "string",
        Type::Bytes => "bytes",
        Type::Bool => "bool",
        Type::I32 => "int32",
        Type::I64 => "int64",
        Type::U32 => "uint32",
        Type::U64 => "uint64",
        Type::F32 => "float",
        Type::F64 => "double",
        Type::Timestamp => "timestamp",
        Type::Duration => "duration",
        Type::List(_) => "repeated",
        Type::Map(_) => "map",
        Type::Named(_) if matches!(kind, Some(TypeKind::Enum(_))) => "enum",
        Type::Named(_) => "",
    }
}

/// `\xNN` escapes for a bytes literal.
fn bytes_lit(b: &[u8]) -> String {
    let mut s = String::from('"');
    for c in b {
        let _ = write!(s, "\\x{c:02x}");
    }
    s.push('"');
    s
}

/// `{seconds: N, nanos: M}`.
fn seconds(x: f64) -> String {
    let s = x.floor();
    #[allow(clippy::cast_possible_truncation)]
    let nanos = ((x - s) * 1e9).round() as i64;
    #[allow(clippy::cast_possible_truncation)]
    let s = s as i64;
    if nanos == 0 {
        format!("{{seconds: {s}}}")
    } else {
        format!("{{seconds: {s}, nanos: {nanos}}}")
    }
}

/// A rule operand in text format for the value type.
fn lit(ty: &Type, kind: Option<&TypeKind>, j: &Json) -> Option<String> {
    Some(match ty {
        Type::String => quote(j.as_str()?),
        Type::Bytes => bytes_lit(&korek_runtime::base64::decode(j.as_str()?)?),
        Type::Timestamp => seconds(korek_runtime::time::parse_timestamp(j.as_str()?)?),
        Type::Duration => seconds(korek_runtime::time::parse_duration(j.as_str()?)?),
        Type::Named(_) => match kind {
            Some(TypeKind::Enum(e)) => e
                .variants
                .iter()
                .find(|v| Some(v.name.as_str()) == j.as_str())?
                .number
                .to_string(),
            _ => return None,
        },
        t if t.is_float() => {
            let f = j.as_f64()?;
            if f.fract() == 0.0 {
                format!("{f:.1}")
            } else {
                format!("{f}")
            }
        }
        _ => j.to_string(),
    })
}

/// Formats protovalidate has a rule for.
fn native_format(f: Format) -> Option<&'static str> {
    Some(match f {
        Format::Email => "email",
        Format::Hostname => "hostname",
        Format::Ip => "ip",
        Format::Ipv4 => "ipv4",
        Format::Ipv6 => "ipv6",
        Format::IpWithPrefixlen => "ip_with_prefixlen",
        Format::Cidr => "ip_prefix",
        Format::Uri | Format::Url => "uri",
        Format::UriRef => "uri_ref",
        Format::Uuid => "uuid",
        Format::Tuuid => "tuuid",
        Format::Ulid => "ulid",
        Format::HostAndPort => "host_and_port",
        Format::Address => "address",
        _ => return None,
    })
}

/// The rule tree for a field of type `ty` with rules `v`.
pub(crate) fn field(
    ty: &Type,
    kind: Option<&TypeKind>,
    v: &Validate,
    warnings: &mut Vec<Diagnostic>,
) -> Tree {
    let mut t = Tree::default();
    fill(&mut t, ty, kind, v, warnings);
    t
}

#[allow(clippy::too_many_lines)]
fn fill(
    t: &mut Tree,
    ty: &Type,
    kind: Option<&TypeKind>,
    v: &Validate,
    warnings: &mut Vec<Diagnostic>,
) {
    let k = kind_name(ty, kind);
    let mut cel: Vec<String> = Vec::new();
    for r in &v.rules {
        let one = |t: &mut Tree, key: &str, val: String| t.sub(k).set(key, val);
        let unsigned = if ty.is_unsigned() { "u" } else { "" };
        match &r.kind {
            RuleKind::Required => t.set("required", "true".into()),
            RuleKind::DefinedOnly => one(t, "defined_only", "true".into()),
            RuleKind::Finite => one(t, "finite", "true".into()),
            RuleKind::Unique => one(t, "unique", "true".into()),
            RuleKind::GtNow => one(t, "gt_now", "true".into()),
            RuleKind::LtNow => one(t, "lt_now", "true".into()),
            RuleKind::Within { duration } => match korek_runtime::time::parse_duration(duration) {
                Some(d) => one(t, "within", seconds(d)),
                None => warnings.push(unsupported("proto", r, "bad duration")),
            },
            RuleKind::Const { value }
            | RuleKind::Gt { value }
            | RuleKind::Gte { value }
            | RuleKind::Lt { value }
            | RuleKind::Lte { value } => match lit(ty, kind, value) {
                Some(l) => one(t, r.kind.name(), l),
                None => warnings.push(unsupported("proto", r, "no literal for this operand")),
            },
            RuleKind::In { values } | RuleKind::NotIn { values } => {
                let ls: Option<Vec<String>> = values.iter().map(|x| lit(ty, kind, x)).collect();
                match ls {
                    Some(ls) => one(t, r.kind.name(), format!("[{}]", ls.join(", "))),
                    None => warnings.push(unsupported("proto", r, "no literal for this operand")),
                }
            }
            RuleKind::Len { n }
            | RuleKind::MinLen { n }
            | RuleKind::MaxLen { n }
            | RuleKind::LenBytes { n }
            | RuleKind::MinBytes { n }
            | RuleKind::MaxBytes { n } => {
                one(t, r.kind.name(), n.to_string());
            }
            RuleKind::MinItems { n }
            | RuleKind::MaxItems { n }
            | RuleKind::MinPairs { n }
            | RuleKind::MaxPairs { n } => one(t, r.kind.name(), n.to_string()),
            RuleKind::Pattern { regex } => one(t, "pattern", quote(regex)),
            RuleKind::Prefix { value }
            | RuleKind::Suffix { value }
            | RuleKind::Contains { value }
            | RuleKind::NotContains { value } => {
                one(t, r.kind.name(), quote(value));
            }
            RuleKind::Format { format } => match (native_format(*format), format_regex(*format)) {
                (Some(name), _)
                    if *ty == Type::String
                        || matches!(format, Format::Ip | Format::Ipv4 | Format::Ipv6) =>
                {
                    one(t, name, "true".into());
                }
                (_, Some(re)) => cel.push(format!(
                    "{{id: {}, message: {}, expression: {}}}",
                    quote(&format!("format.{}", format.name())),
                    quote(
                        &r.message
                            .clone()
                            .unwrap_or_else(|| format!("must be a valid {}", format.name()))
                    ),
                    quote(&format!("this.matches('{}')", re.replace('\\', "\\\\")))
                )),
                _ => warnings.push(unsupported(
                    "proto",
                    r,
                    "protovalidate has no rule for this format",
                )),
            },
            RuleKind::MultipleOf { value } if ty.is_integer() => cel.push(format!(
                "{{id: \"multiple_of\", message: {}, expression: {}}}",
                quote(
                    &r.message
                        .clone()
                        .unwrap_or_else(|| format!("must be a multiple of {value}"))
                ),
                quote(&format!("this % {value}{unsigned} == 0{unsigned}"))
            )),
            RuleKind::Cel { id, expression, .. } => cel.push(format!(
                "{{id: {}, message: {}, expression: {}}}",
                quote(id),
                quote(&r.message.clone().unwrap_or_else(|| format!("failed {id}"))),
                quote(expression)
            )),
            _ => warnings.push(unsupported(
                "proto",
                r,
                "protovalidate has no rule for this",
            )),
        }
    }
    if !cel.is_empty() {
        t.set("cel", format!("[{}]", cel.join(", ")));
    }
    let nested = |x: &Option<Box<Validate>>| x.as_deref().filter(|v| !v.is_empty()).cloned();
    match ty {
        Type::List(inner) => {
            if let Some(items) = nested(&v.items) {
                let mut sub = Tree::default();
                fill(&mut sub, inner, None, &items, warnings);
                if !sub.is_empty() {
                    t.sub("repeated")
                        .entries
                        .push(("items".into(), Entry::Tree(sub)));
                }
            }
        }
        Type::Map(inner) => {
            for (key, block, ty) in [
                ("keys", nested(&v.keys), Type::String),
                ("values", nested(&v.values), (**inner).clone()),
            ] {
                if let Some(b) = block {
                    let mut sub = Tree::default();
                    fill(&mut sub, &ty, None, &b, warnings);
                    if !sub.is_empty() {
                        t.sub("map").entries.push((key.into(), Entry::Tree(sub)));
                    }
                }
            }
        }
        _ => {}
    }
}
