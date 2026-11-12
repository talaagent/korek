//! Names derived from schema names, shared by the core's collision checks
//! and the generators.

use crate::ir::{KeyGroup, Type};

/// The message/interface name of a keys block: `ui.theme` → `UiThemeKeys`.
pub fn keys_type(group: &KeyGroup) -> String {
    format!("{}Keys", pascal(&group.ident()))
}

/// The Protobuf wrapper message for a list or map that cannot appear
/// directly (nested in another collection or in a `oneof`):
/// `list<string>` → `StringList`, `map<string,list<i64>>` → `Int64ListMap`.
pub fn proto_wrapper(ty: &Type) -> String {
    match ty {
        Type::List(t) => format!("{}List", proto_base(t)),
        Type::Map(t) => format!("{}Map", proto_base(t)),
        other => proto_base(other),
    }
}

fn proto_base(ty: &Type) -> String {
    match ty {
        Type::Bool => "Bool".into(),
        Type::String => "String".into(),
        Type::I32 => "Int32".into(),
        Type::I64 => "Int64".into(),
        Type::U32 => "Uint32".into(),
        Type::U64 => "Uint64".into(),
        Type::F32 => "Float".into(),
        Type::F64 => "Double".into(),
        Type::Bytes => "Bytes".into(),
        Type::Timestamp => "Timestamp".into(),
        Type::Duration => "Duration".into(),
        Type::Json => "Json".into(),
        Type::Named(n) => n.clone(),
        Type::List(_) | Type::Map(_) => proto_wrapper(ty),
    }
}

/// Every list/map type that needs a Protobuf wrapper message, innermost
/// first, deduplicated, given the types used by fields, variants and keys.
pub fn proto_wrappers<'a>(fields: impl IntoIterator<Item = (&'a Type, bool)>) -> Vec<Type> {
    fn need(t: &Type, out: &mut Vec<Type>) {
        let (Type::List(inner) | Type::Map(inner)) = t else {
            return;
        };
        need(inner, out);
        if !out.contains(t) {
            out.push(t.clone());
        }
    }
    fn walk(t: &Type, out: &mut Vec<Type>) {
        if let Type::List(inner) | Type::Map(inner) = t {
            if inner.is_collection() {
                need(inner, out);
            }
            walk(inner, out);
        }
    }
    let mut out: Vec<Type> = Vec::new();
    // `in_oneof`: a union variant, where collections must be wrapped.
    for (t, in_oneof) in fields {
        if in_oneof && t.is_collection() {
            need(t, &mut out);
        }
        walk(t, &mut out);
    }
    out
}

/// `lower_snake` → `PascalCase` (`ui_theme` → `UiTheme`, `v2_x` → `V2X`).
pub fn pascal(s: &str) -> String {
    s.split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                .unwrap_or_default()
        })
        .collect()
}

/// `lower_snake` → `SHOUTY_SNAKE`.
pub fn shouty(s: &str) -> String {
    s.to_ascii_uppercase()
}

/// `lower_snake` → `lowerCamel`.
pub fn camel(s: &str) -> String {
    let p = pascal(s);
    let mut c = p.chars();
    c.next()
        .map(|f| f.to_ascii_lowercase().to_string() + c.as_str())
        .unwrap_or_default()
}
