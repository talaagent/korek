//! Typed values: JSON matched to a korek type.

use crate::Types;
use crate::ir::{Field, Json, Type, TypeKind};

/// A JSON value matched to its type, so generators can write literals
/// (`Backend::Local`, `McpStdio { .. }`) without re-deriving union
/// variants.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Value {
    Bool(bool),
    String(String),
    /// Base64 text, as in JSON.
    Bytes(String),
    Int(i64),
    Uint(u64),
    Float(f64),
    /// RFC 3339 text.
    Timestamp(String),
    /// `"1.5s"` text.
    Duration(String),
    /// Any JSON, as is.
    Json(Json),
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

/// Matches `json` against `ty` (shape only; rules are checked by the
/// loader).
///
/// # Errors
/// What does not match, with a path.
pub fn resolve<S: Types + ?Sized>(schema: &S, ty: &Type, j: &Json) -> Result<Value, String> {
    let bad = || format!("expected {}, found {}", ty.display(), kind(j));
    let str_of = || j.as_str().map(str::to_owned).ok_or_else(bad);
    Ok(match ty {
        Type::Bool => Value::Bool(j.as_bool().ok_or_else(bad)?),
        Type::String => Value::String(str_of()?),
        Type::Bytes => Value::Bytes(str_of()?),
        Type::Timestamp => Value::Timestamp(str_of()?),
        Type::Duration => Value::Duration(str_of()?),
        Type::Json => Value::Json(j.clone()),
        Type::I32 => Value::Int(
            j.as_i64()
                .filter(|i| i32::try_from(*i).is_ok())
                .ok_or_else(bad)?,
        ),
        Type::I64 => Value::Int(j.as_i64().ok_or_else(bad)?),
        Type::U32 => Value::Uint(
            j.as_u64()
                .filter(|i| u32::try_from(*i).is_ok())
                .ok_or_else(bad)?,
        ),
        Type::U64 => Value::Uint(j.as_u64().ok_or_else(bad)?),
        Type::F32 | Type::F64 => Value::Float(j.as_f64().ok_or_else(bad)?),
        Type::List(t) => Value::List(
            j.as_array()
                .ok_or_else(bad)?
                .iter()
                .enumerate()
                .map(|(i, v)| resolve(schema, t, v).map_err(|e| format!("[{i}]: {e}")))
                .collect::<Result<_, _>>()?,
        ),
        Type::Map(t) => Value::Map(
            j.as_object()
                .ok_or_else(bad)?
                .iter()
                .map(|(k, v)| {
                    resolve(schema, t, v)
                        .map(|v| (k.clone(), v))
                        .map_err(|e| format!("{k:?}: {e}"))
                })
                .collect::<Result<_, _>>()?,
        ),
        Type::Named(name) => named(schema, name, j)?,
    })
}

fn named<S: Types + ?Sized>(schema: &S, name: &str, j: &Json) -> Result<Value, String> {
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
                let value = resolve(schema, &v.ty, j).map_err(|e| format!("{name}::{t}: {e}"))?;
                Ok(Value::Union {
                    ty: name.to_owned(),
                    variant: t.to_owned(),
                    value: Box::new(value),
                })
            } else {
                u.variants
                    .iter()
                    .find_map(|v| {
                        resolve(schema, &v.ty, j).ok().map(|value| Value::Union {
                            ty: name.to_owned(),
                            variant: v.name.clone(),
                            value: Box::new(value),
                        })
                    })
                    .ok_or_else(|| format!("{} matches no {name} variant", kind(j)))
            }
        }
    }
}

fn struct_value<S: Types + ?Sized>(
    schema: &S,
    name: &str,
    fields: &[Field],
    o: &serde_json::Map<String, Json>,
) -> Result<Value, String> {
    let mut out = Vec::with_capacity(fields.len());
    for f in fields {
        match o.get(&f.name) {
            None | Some(Json::Null) if f.optional || f.default.is_some() => {
                out.push((f.name.clone(), None));
            }
            None => return Err(format!("{name} is missing `{}`", f.name)),
            Some(j) => {
                let v = resolve(schema, &f.ty, j).map_err(|e| format!("{name}.{}: {e}", f.name))?;
                out.push((f.name.clone(), Some(v)));
            }
        }
    }
    Ok(Value::Struct {
        ty: name.to_owned(),
        fields: out,
    })
}
