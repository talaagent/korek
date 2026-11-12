//! Evaluates `validate` rules on JSON values (defaults and examples) at
//! load time, with the same format checks the generated Rust uses.

use korek_ir::{Format, Json, RuleKind, Type, TypeKind, Types, Validate};
use korek_runtime::{base64, format, time};

/// Checks `j` (already shape-checked against `ty`) against `v` and every
/// rule nested in its type.
///
/// # Errors
/// The first failure, with a path.
pub fn check<S: Types + ?Sized>(
    schema: &S,
    ty: &Type,
    j: &Json,
    v: &Validate,
) -> Result<(), String> {
    for r in &v.rules {
        rule(ty, j, &r.kind).map_err(|e| r.message.clone().unwrap_or(e))?;
    }
    match (ty, j) {
        (Type::List(t), Json::Array(items)) => {
            for (i, x) in items.iter().enumerate() {
                let empty = Validate::default();
                check(schema, t, x, v.items.as_deref().unwrap_or(&empty))
                    .map_err(|e| format!("[{i}]: {e}"))?;
            }
        }
        (Type::Map(t), Json::Object(m)) => {
            for (k, x) in m {
                if let Some(kv) = &v.keys {
                    check(schema, &Type::String, &Json::String(k.clone()), kv)
                        .map_err(|e| format!("key {k:?}: {e}"))?;
                }
                let empty = Validate::default();
                check(schema, t, x, v.values.as_deref().unwrap_or(&empty))
                    .map_err(|e| format!("{k:?}: {e}"))?;
            }
        }
        (Type::Named(n), _) => named(schema, n, j)?,
        _ => {}
    }
    Ok(())
}

/// Compiles `pattern` once per thread: a schema repeats the same patterns
/// across many fields, defaults and examples.
pub(crate) fn compile(pattern: &str) -> Result<regex::Regex, String> {
    thread_local! {
        static CACHE: core::cell::RefCell<std::collections::HashMap<String, regex::Regex>> =
            core::cell::RefCell::default();
    }
    CACHE.with(|c| {
        if let Some(re) = c.borrow().get(pattern) {
            return Ok(re.clone());
        }
        let re = regex::Regex::new(pattern).map_err(|e| e.to_string())?;
        c.borrow_mut().insert(pattern.to_owned(), re.clone());
        Ok(re)
    })
}

fn named<S: Types + ?Sized>(schema: &S, name: &str, j: &Json) -> Result<(), String> {
    let Some(def) = schema.get(name) else {
        return Ok(());
    };
    for r in &def.validate.rules {
        rule(&Type::Named(name.to_owned()), j, &r.kind)
            .map_err(|e| r.message.clone().unwrap_or(e))?;
    }
    match &def.kind {
        TypeKind::Enum(_) => Ok(()),
        TypeKind::Struct(s) => {
            for f in &s.fields {
                if let Some(x) = j.get(&f.name).filter(|x| !x.is_null()) {
                    check(schema, &f.ty, x, &f.validate).map_err(|e| format!("{}: {e}", f.name))?;
                }
            }
            Ok(())
        }
        TypeKind::Union(u) => {
            let variant = match &u.tag {
                Some(tag) => {
                    let t = j.get(tag).and_then(Json::as_str);
                    u.variants.iter().find(|v| Some(v.name.as_str()) == t)
                }
                None => u
                    .variants
                    .iter()
                    .find(|v| korek_ir::value::resolve(schema, &v.ty, j).is_ok()),
            };
            match variant {
                Some(v) => check(schema, &v.ty, j, &Validate::default()),
                None => Ok(()),
            }
        }
    }
}

fn num(j: &Json) -> Option<f64> {
    j.as_f64()
}

/// A comparable number for numeric, timestamp and duration values.
fn ordinal(ty: &Type, j: &Json) -> Option<f64> {
    match ty {
        Type::Timestamp => time::parse_timestamp(j.as_str()?),
        Type::Duration => time::parse_duration(j.as_str()?),
        _ => num(j),
    }
}

fn same(a: &Json, b: &Json) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => (x - y).abs() < f64::EPSILON,
        _ => a == b,
    }
}

#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
fn rule(ty: &Type, j: &Json, kind: &RuleKind) -> Result<(), String> {
    let fail = |m: String| Err(m);
    let s = j.as_str();
    let bytes = || match ty {
        Type::Bytes => s.and_then(base64::decode),
        _ => None,
    };
    let len = || match ty {
        Type::Bytes => bytes().map(|b| b.len() as u64),
        _ => s.map(|s| s.chars().count() as u64),
    };
    let text = || match ty {
        Type::Bytes => bytes().map(|b| String::from_utf8_lossy(&b).into_owned()),
        _ => s.map(str::to_owned),
    };
    match kind {
        RuleKind::Const { value } if !same(j, value) => fail(format!("must equal {value}")),
        RuleKind::In { values } if !values.iter().any(|v| same(j, v)) => {
            fail(format!("must be one of {}", Json::Array(values.clone())))
        }
        RuleKind::NotIn { values } if values.iter().any(|v| same(j, v)) => fail(format!(
            "must not be one of {}",
            Json::Array(values.clone())
        )),
        RuleKind::Len { n } if len() != Some(*n) => fail(format!("length must be {n}")),
        RuleKind::MinLen { n } if len().is_some_and(|l| l < *n) => {
            fail(format!("length must be at least {n}"))
        }
        RuleKind::MaxLen { n } if len().is_some_and(|l| l > *n) => {
            fail(format!("length must be at most {n}"))
        }
        RuleKind::LenBytes { n } if s.map(|s| s.len() as u64) != Some(*n) => {
            fail(format!("must be {n} bytes"))
        }
        RuleKind::MinBytes { n } if s.is_some_and(|s| (s.len() as u64) < *n) => {
            fail(format!("must be at least {n} bytes"))
        }
        RuleKind::MaxBytes { n } if s.is_some_and(|s| s.len() as u64 > *n) => {
            fail(format!("must be at most {n} bytes"))
        }
        RuleKind::Pattern { regex } => {
            let re = compile(regex)?;
            if text().is_some_and(|t| !re.is_match(&t)) {
                return fail(format!("must match {regex:?}"));
            }
            Ok(())
        }
        RuleKind::Prefix { value } if text().is_some_and(|t| !t.starts_with(value.as_str())) => {
            fail(format!("must start with {value:?}"))
        }
        RuleKind::Suffix { value } if text().is_some_and(|t| !t.ends_with(value.as_str())) => {
            fail(format!("must end with {value:?}"))
        }
        RuleKind::Contains { value } if text().is_some_and(|t| !t.contains(value.as_str())) => {
            fail(format!("must contain {value:?}"))
        }
        RuleKind::NotContains { value } if text().is_some_and(|t| t.contains(value.as_str())) => {
            fail(format!("must not contain {value:?}"))
        }
        RuleKind::Format { format: f } => {
            let ok = match (ty, f) {
                (Type::Bytes, Format::Ip) => bytes().is_some_and(|b| b.len() == 4 || b.len() == 16),
                (Type::Bytes, Format::Ipv4) => bytes().is_some_and(|b| b.len() == 4),
                (Type::Bytes, Format::Ipv6) => bytes().is_some_and(|b| b.len() == 16),
                _ => s.is_some_and(|s| format::by_name(f.name()).is_some_and(|check| check(s))),
            };
            if ok {
                Ok(())
            } else {
                fail(format!("must be a valid {}", f.name()))
            }
        }
        RuleKind::Gt { value }
        | RuleKind::Gte { value }
        | RuleKind::Lt { value }
        | RuleKind::Lte { value } => {
            let (Some(x), Some(b)) = (ordinal(ty, j), ordinal(ty, value)) else {
                return Ok(());
            };
            let (ok, op) = match kind {
                RuleKind::Gt { .. } => (x > b, "greater than"),
                RuleKind::Gte { .. } => (x >= b, "at least"),
                RuleKind::Lt { .. } => (x < b, "less than"),
                _ => (x <= b, "at most"),
            };
            if ok {
                Ok(())
            } else {
                fail(format!("must be {op} {value}"))
            }
        }
        RuleKind::Finite if num(j).is_some_and(|x| !x.is_finite()) => fail("must be finite".into()),
        RuleKind::MultipleOf { value } => match (num(j), num(value)) {
            (Some(x), Some(m)) if m != 0.0 && (x / m - (x / m).round()).abs() > 1e-9 => {
                fail(format!("must be a multiple of {value}"))
            }
            _ => Ok(()),
        },
        RuleKind::MinItems { n } if j.as_array().is_some_and(|a| (a.len() as u64) < *n) => {
            fail(format!("must have at least {n} items"))
        }
        RuleKind::MaxItems { n } if j.as_array().is_some_and(|a| a.len() as u64 > *n) => {
            fail(format!("must have at most {n} items"))
        }
        RuleKind::Unique if j.as_array().is_some_and(|a| !korek_runtime::unique(a)) => {
            fail("items must be unique".into())
        }
        RuleKind::MinPairs { n } if j.as_object().is_some_and(|o| (o.len() as u64) < *n) => {
            fail(format!("must have at least {n} entries"))
        }
        RuleKind::MaxPairs { n } if j.as_object().is_some_and(|o| o.len() as u64 > *n) => {
            fail(format!("must have at most {n} entries"))
        }
        RuleKind::GtNow if ordinal(ty, j).is_some_and(|x| x <= time::now()) => {
            fail("must be in the future".into())
        }
        RuleKind::LtNow if ordinal(ty, j).is_some_and(|x| x >= time::now()) => {
            fail("must be in the past".into())
        }
        RuleKind::Within { duration } => match (ordinal(ty, j), time::parse_duration(duration)) {
            (Some(x), Some(d)) if (x - time::now()).abs() > d => {
                fail(format!("must be within {duration} of now"))
            }
            _ => Ok(()),
        },
        _ => Ok(()),
    }
}
