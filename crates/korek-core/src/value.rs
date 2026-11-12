//! Typed values: defaults resolved against their type.

use serde_json::Value as Json;

use crate::ir::{Constraints, Schema, Type, TypeKind, format_number};

/// A JSON value matched to a korek type, so generators can write literals
/// (`Backend::Claude`, `McpStdio { .. }`) without re-deriving union variants.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Value {
    Bool(bool),
    String(String),
    I64(i64),
    U64(u64),
    F64(f64),
    Bytes(Vec<u8>),
    List(Vec<Value>),
    Map(Vec<(String, Value)>),
    Enum {
        ty: String,
        variant: String,
    },
    /// Fields in declaration order; `None` for an absent optional field.
    Struct {
        ty: String,
        fields: Vec<(String, Option<Value>)>,
    },
    Union {
        ty: String,
        variant: String,
        value: Box<Value>,
    },
}

impl Value {
    /// The canonical JSON form (what serde writes and Zod reads).
    pub fn to_json(&self) -> Json {
        match self {
            Self::Bool(b) => Json::Bool(*b),
            Self::String(s) => Json::String(s.clone()),
            Self::I64(i) => Json::from(*i),
            Self::U64(u) => Json::from(*u),
            Self::F64(f) => Json::from(*f),
            Self::Bytes(b) => Json::Array(b.iter().map(|b| Json::from(*b)).collect()),
            Self::List(v) => Json::Array(v.iter().map(Self::to_json).collect()),
            Self::Map(m) => Json::Object(m.iter().map(|(k, v)| (k.clone(), v.to_json())).collect()),
            Self::Enum { variant, .. } => Json::String(variant.clone()),
            Self::Struct { fields, .. } => Json::Object(
                fields
                    .iter()
                    .filter_map(|(k, v)| v.as_ref().map(|v| (k.clone(), v.to_json())))
                    .collect(),
            ),
            Self::Union { value, .. } => value.to_json(),
        }
    }
}

/// Matches `json` against `ty`, checking `c` and the constraints of nested
/// struct fields.
///
/// # Errors
/// What does not match, with a JSON-pointer-like path.
pub fn resolve(schema: &Schema, ty: &Type, json: &Json, c: &Constraints) -> Result<Value, String> {
    let v = resolve_ty(schema, ty, json)?;
    check(&v, json, c)?;
    Ok(v)
}

fn kind(j: &Json) -> &'static str {
    match j {
        Json::Null => "null",
        Json::Bool(_) => "a boolean",
        Json::Number(_) => "a number",
        Json::String(_) => "a string",
        Json::Array(_) => "an array",
        Json::Object(_) => "an object",
    }
}

fn mismatch(ty: &Type, j: &Json) -> String {
    format!("expected {}, found {}", ty.display(), kind(j))
}

fn resolve_ty(schema: &Schema, ty: &Type, j: &Json) -> Result<Value, String> {
    let bad = || mismatch(ty, j);
    Ok(match ty {
        Type::Bool => Value::Bool(j.as_bool().ok_or_else(bad)?),
        Type::String => Value::String(j.as_str().ok_or_else(bad)?.to_owned()),
        Type::I64 => Value::I64(j.as_i64().ok_or_else(bad)?),
        Type::U64 => Value::U64(j.as_u64().ok_or_else(bad)?),
        Type::F64 => Value::F64(j.as_f64().ok_or_else(bad)?),
        Type::Bytes => Value::Bytes(
            j.as_array()
                .ok_or_else(bad)?
                .iter()
                .map(|b| {
                    b.as_u64()
                        .and_then(|b| u8::try_from(b).ok())
                        .ok_or_else(bad)
                })
                .collect::<Result<_, _>>()?,
        ),
        Type::List(t) => Value::List(
            j.as_array()
                .ok_or_else(bad)?
                .iter()
                .enumerate()
                .map(|(i, v)| resolve_ty(schema, t, v).map_err(|e| format!("[{i}]: {e}")))
                .collect::<Result<_, _>>()?,
        ),
        Type::Map(t) => Value::Map(
            j.as_object()
                .ok_or_else(bad)?
                .iter()
                .map(|(k, v)| {
                    resolve_ty(schema, t, v)
                        .map(|v| (k.clone(), v))
                        .map_err(|e| format!("{k:?}: {e}"))
                })
                .collect::<Result<_, _>>()?,
        ),
        Type::Named(name) => named(schema, name, j)?,
    })
}

#[allow(clippy::many_single_char_names)]
fn named(schema: &Schema, name: &str, j: &Json) -> Result<Value, String> {
    let def = schema
        .get(name)
        .ok_or_else(|| format!("unknown type `{name}`"))?;
    match &def.kind {
        TypeKind::Enum(e) => {
            let s = j
                .as_str()
                .ok_or_else(|| format!("expected a {name} variant name, found {}", kind(j)))?;
            if e.variants.iter().any(|v| v.name == s) {
                Ok(Value::Enum {
                    ty: name.to_owned(),
                    variant: s.to_owned(),
                })
            } else {
                let all: Vec<_> = e.variants.iter().map(|v| v.name.as_str()).collect();
                Err(format!(
                    "`{s}` is not a {name} variant (expected one of {})",
                    all.join(", ")
                ))
            }
        }
        TypeKind::Struct(s) => {
            let o = j
                .as_object()
                .ok_or_else(|| format!("expected a {name} object, found {}", kind(j)))?;
            struct_value(schema, name, &s.fields, o)
        }
        TypeKind::Union(u) => {
            if let Some(tag) = &u.tag {
                let o = j
                    .as_object()
                    .ok_or_else(|| format!("expected a {name} object, found {}", kind(j)))?;
                let t = o
                    .get(tag)
                    .and_then(Json::as_str)
                    .ok_or_else(|| format!("{name} needs a string `{tag}` field"))?;
                let v = u
                    .variants
                    .iter()
                    .find(|v| v.name == t)
                    .ok_or_else(|| format!("`{t}` is not a {name} variant"))?;
                let value =
                    resolve_ty(schema, &v.ty, j).map_err(|e| format!("{name}::{t}: {e}"))?;
                Ok(Value::Union {
                    ty: name.to_owned(),
                    variant: t.to_owned(),
                    value: Box::new(value),
                })
            } else {
                for v in &u.variants {
                    if let Ok(value) = resolve_ty(schema, &v.ty, j) {
                        return Ok(Value::Union {
                            ty: name.to_owned(),
                            variant: v.name.clone(),
                            value: Box::new(value),
                        });
                    }
                }
                Err(format!("{} matches no {name} variant", kind(j)))
            }
        }
    }
}

fn struct_value(
    schema: &Schema,
    name: &str,
    fields: &[crate::ir::Field],
    o: &serde_json::Map<String, Json>,
) -> Result<Value, String> {
    let mut out = Vec::new();
    for f in fields {
        match o.get(&f.name) {
            None | Some(Json::Null) if f.optional => out.push((f.name.clone(), None)),
            None => return Err(format!("{name} is missing `{}`", f.name)),
            Some(j) => {
                let v = resolve_ty(schema, &f.ty, j)
                    .and_then(|v| check(&v, j, &f.constraints).map(|()| v))
                    .map_err(|e| format!("{name}.{}: {e}", f.name))?;
                out.push((f.name.clone(), Some(v)));
            }
        }
    }
    Ok(Value::Struct {
        ty: name.to_owned(),
        fields: out,
    })
}

/// Checks constraints on a resolved value.
fn check(v: &Value, j: &Json, c: &Constraints) -> Result<(), String> {
    let num = match v {
        #[allow(clippy::cast_precision_loss)]
        Value::I64(i) => Some(*i as f64),
        #[allow(clippy::cast_precision_loss)]
        Value::U64(u) => Some(*u as f64),
        Value::F64(f) => Some(*f),
        _ => None,
    };
    if let Some(n) = num {
        if let Some(min) = c.min.filter(|m| n < *m) {
            return Err(format!("{} is below the minimum {}", j, format_number(min)));
        }
        if let Some(max) = c.max.filter(|m| n > *m) {
            return Err(format!("{} is above the maximum {}", j, format_number(max)));
        }
    }
    let len = match v {
        Value::String(s) => Some(s.chars().count()),
        Value::Bytes(b) => Some(b.len()),
        Value::List(l) => Some(l.len()),
        Value::Map(m) => Some(m.len()),
        _ => None,
    };
    if let Some(len) = len.map(|l| l as u64) {
        if let Some(min) = c.min_len.filter(|m| len < *m) {
            return Err(format!("length {len} is below min_len {min}"));
        }
        if let Some(max) = c.max_len.filter(|m| len > *m) {
            return Err(format!("length {len} is above max_len {max}"));
        }
    }
    if let (Some(p), Value::String(s)) = (&c.pattern, v) {
        let re = regex::Regex::new(p).map_err(|e| format!("bad pattern: {e}"))?;
        if !re.is_match(s) {
            return Err(format!("{s:?} does not match the pattern {p:?}"));
        }
    }
    Ok(())
}
