//! Names derived from schema names, shared by the core's collision checks
//! and the generators.

use heck::ToUpperCamelCase as _;

use crate::ir::{KeyGroup, Type};

/// The message/interface name of a keys block: `ui.theme` → `UiThemeKeys`.
pub fn keys_type(group: &KeyGroup) -> String {
    format!("{}Keys", group.ident().to_upper_camel_case())
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
        Type::I64 => "Int64".into(),
        Type::U64 => "Uint64".into(),
        Type::F64 => "Double".into(),
        Type::Bytes => "Bytes".into(),
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
