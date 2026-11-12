//! Checks on the lowered schema: names, references, unions, cycles,
//! constraints and defaults.

use std::collections::{HashMap, HashSet};

use crate::diag::Diagnostic;
use crate::ext::Literal;
use crate::ir::{Constraints, Schema, Span, Type, TypeKind};
use crate::naming;
use crate::parse::{Aux, is_pascal, is_snake};
use crate::value::resolve;

fn err(code: &str, msg: impl Into<String>, span: Span, label: &str) -> Diagnostic {
    Diagnostic::error(code, msg).at(span, label)
}

pub(crate) struct Validator<'a> {
    pub schema: &'a Schema,
    pub aux: &'a Aux,
    pub diags: Vec<Diagnostic>,
    /// Key defaults in canonical JSON, by key span.
    pub resolved: Vec<(Span, serde_json::Value)>,
}

/// Field names Rust cannot spell even as a raw identifier.
const NOT_RAW: &[&str] = &["self", "super", "crate", "_"];

impl Validator<'_> {
    fn name_span(&self, item: Span) -> Span {
        self.aux.name_span.get(&item).copied().unwrap_or(item)
    }

    fn dup(&mut self, seen: &mut HashMap<String, Span>, name: &str, span: Span, what: &str) {
        let at = self.name_span(span);
        if seen.insert(name.to_owned(), at).is_some() {
            self.diags.push(err(
                "korek::duplicate",
                format!("duplicate {what} `{name}`"),
                at,
                "declared again here",
            ));
        }
    }

    fn snake(&mut self, name: &str, span: Span, what: &str) {
        if !is_snake(name) {
            self.diags.push(err(
                "korek::bad_name",
                format!("{what} `{name}` must be lower_snake_case"),
                self.name_span(span),
                "here",
            ));
        }
    }

    fn pascal(&mut self, name: &str, span: Span, what: &str) {
        if !is_pascal(name) {
            self.diags.push(err(
                "korek::bad_name",
                format!("{what} `{name}` must be PascalCase"),
                self.name_span(span),
                "here",
            ));
        }
    }

    fn dotted(&mut self, name: &str, span: Span, what: &str) {
        if !name.split('.').all(is_snake) {
            self.diags.push(err(
                "korek::bad_name",
                format!("{what} `{name}` must be dotted lower_snake_case segments"),
                self.name_span(span),
                "here",
            ));
        }
    }

    pub fn run(&mut self) {
        self.names();
        self.references();
        if !self.diags.is_empty() {
            return;
        }
        self.unions();
        self.cycles();
        self.constraints();
        if self.diags.is_empty() {
            self.defaults();
        }
    }

    fn names(&mut self) {
        let s = self.schema;
        // One namespace: Protobuf messages, enums and services share it.
        let mut top = HashMap::new();
        for t in &s.types {
            self.pascal(&t.name, t.span, "type");
            self.dup(&mut top, &t.name, t.span, "type");
            let mut seen = HashMap::new();
            match &t.kind {
                TypeKind::Enum(e) => {
                    if e.variants.is_empty() {
                        self.diags.push(err("korek::empty", format!("enum `{}` has no variants", t.name), self.name_span(t.span), "add a variant"));
                    }
                    for v in &e.variants {
                        self.snake(&v.name, v.span, "variant");
                        self.dup(&mut seen, &v.name, v.span, "variant");
                        if v.name == "unspecified" {
                            self.diags.push(
                                err("korek::bad_name", "`unspecified` is reserved", self.name_span(v.span), "reserved")
                                    .help("Protobuf's zero value is <ENUM>_UNSPECIFIED; korek emits it for you"),
                            );
                        }
                    }
                }
                TypeKind::Struct(st) => {
                    for f in &st.fields {
                        self.snake(&f.name, f.span, "field");
                        self.dup(&mut seen, &f.name, f.span, "field");
                        if NOT_RAW.contains(&f.name.as_str()) {
                            self.diags.push(err("korek::bad_name", format!("`{}` cannot be a field name", f.name), self.name_span(f.span), "reserved in Rust"));
                        }
                    }
                }
                TypeKind::Union(u) => {
                    if u.variants.is_empty() {
                        self.diags.push(err("korek::empty", format!("union `{}` has no variants", t.name), self.name_span(t.span), "add a variant"));
                    }
                    for v in &u.variants {
                        self.snake(&v.name, v.span, "variant");
                        self.dup(&mut seen, &v.name, v.span, "variant");
                    }
                }
            }
        }
        for svc in &s.services {
            self.pascal(&svc.name, svc.span, "service");
            self.dup(&mut top, &svc.name, svc.span, "type or service");
            let mut seen = HashMap::new();
            for r in &svc.rpcs {
                self.pascal(&r.name, r.span, "rpc");
                self.dup(&mut seen, &r.name, r.span, "rpc");
            }
        }
        let mut groups = HashMap::new();
        for g in &s.keys {
            self.dotted(&g.name, g.span, "keys block");
            self.dup(&mut groups, &g.ident(), g.span, "keys block");
            let generated = naming::keys_type(g);
            if let Some(at) = top.get(&generated) {
                let at = *at;
                self.diags.push(
                    err("korek::name_clash", format!("keys block `{}` generates the type `{generated}`, which is already declared", g.name), self.name_span(g.span), "generates a clashing type")
                        .help(format!("rename the type declared at byte {}", at.offset)),
                );
            }
            let mut seen = HashMap::new();
            for k in &g.keys {
                self.dotted(&k.name, k.span, "key");
                self.dup(&mut seen, &k.ident(), k.span, "key (dots count as underscores)");
            }
        }
        let used = used_types(self.schema).into_iter().map(|(t, o, _)| (t, o));
        for w in naming::proto_wrappers(used) {
            let name = naming::proto_wrapper(&w);
            if let Some(at) = top.get(&name) {
                let at = *at;
                self.diags.push(err("korek::name_clash", format!("`{name}` is the Protobuf wrapper korek generates for `{}`", w.display()), at, "rename this type"));
            }
        }
    }

    fn unknown(&mut self, name: &str, at: Span) {
        let names = self.schema.types.iter().map(|t| t.name.as_str());
        let mut d = err("korek::unknown_type", format!("unknown type `{name}`"), at, "not declared");
        if let Some(best) = suggest(name, names) {
            d = d.help(format!("did you mean `{best}`?"));
        }
        self.diags.push(d);
    }

    fn references(&mut self) {
        let declared: HashSet<&str> = self.schema.types.iter().map(|t| t.name.as_str()).collect();
        for (ty, _, item) in used_types(self.schema) {
            let mut t = ty;
            while let Type::List(inner) | Type::Map(inner) = t {
                t = inner;
            }
            if let Type::Named(n) = t {
                if !declared.contains(n.as_str()) {
                    let at = self.aux.ty_span.get(&item).copied().unwrap_or(item);
                    self.unknown(n, at);
                }
            }
        }
        for svc in &self.schema.services {
            for r in &svc.rpcs {
                for (name, at) in [(&r.input, self.aux.ty_span.get(&r.span)), (&r.output, self.aux.out_span.get(&r.span))] {
                    let at = at.copied().unwrap_or(r.span);
                    match self.schema.get(name).map(|t| &t.kind) {
                        None => self.unknown(name, at),
                        Some(TypeKind::Enum(_)) => self.diags.push(err("korek::bad_rpc", format!("`{name}` is an enum; rpc input and output must be a struct or union"), at, "not a message")),
                        Some(_) => {}
                    }
                }
            }
        }
    }

    fn unions(&mut self) {
        for t in &self.schema.types {
            let TypeKind::Union(u) = &t.kind else { continue };
            let Some(tag) = &u.tag else { continue };
            for v in &u.variants {
                let at = self.aux.ty_span.get(&v.span).copied().unwrap_or(v.span);
                let target = match &v.ty {
                    Type::Named(n) => self.schema.get(n),
                    _ => None,
                };
                match target.map(|t| &t.kind) {
                    Some(TypeKind::Struct(s)) => {
                        if s.fields.iter().any(|f| &f.name == tag) {
                            self.diags.push(err("korek::tag_clash", format!("struct `{}` has a field `{tag}`, the tag of union `{}`", v.ty.display(), t.name), at, "clashes with the tag"));
                        }
                    }
                    _ => self.diags.push(
                        err("korek::bad_union", format!("variant `{}` of tagged union `{}` must be a struct, found `{}`", v.name, t.name, v.ty.display()), at, "not a struct")
                            .help("remove `tag=` for an untagged union of any types"),
                    ),
                }
            }
        }
    }

    fn cycles(&mut self) {
        let s = self.schema;
        let edges = |t: &crate::ir::TypeDef| -> Vec<String> {
            let mut out = Vec::new();
            let mut push = |ty: &Type| {
                let mut ty = ty;
                while let Type::List(i) | Type::Map(i) = ty {
                    ty = i;
                }
                if let Type::Named(n) = ty {
                    out.push(n.clone());
                }
            };
            match &t.kind {
                TypeKind::Enum(_) => {}
                TypeKind::Struct(st) => st.fields.iter().for_each(|f| push(&f.ty)),
                TypeKind::Union(u) => u.variants.iter().for_each(|v| push(&v.ty)),
            }
            out
        };
        // 0 = unvisited, 1 = on the stack, 2 = done.
        let mut state: HashMap<&str, u8> = HashMap::new();
        let mut reported = false;
        fn visit<'s>(s: &'s Schema, name: &'s str, state: &mut HashMap<&'s str, u8>, edges: &dyn Fn(&crate::ir::TypeDef) -> Vec<String>, path: &mut Vec<&'s str>) -> Option<Vec<String>> {
            match state.get(name) {
                Some(2) => return None,
                Some(1) => {
                    let start = path.iter().position(|p| *p == name).unwrap_or(0);
                    let mut cycle: Vec<String> = path[start..].iter().map(|s| (*s).to_owned()).collect();
                    cycle.push(name.to_owned());
                    return Some(cycle);
                }
                _ => {}
            }
            let def = s.get(name)?;
            state.insert(name, 1);
            path.push(&def.name);
            for e in edges(def) {
                let Some(target) = s.get(&e) else { continue };
                if let Some(c) = visit(s, &target.name, state, edges, path) {
                    return Some(c);
                }
            }
            path.pop();
            state.insert(name, 2);
            None
        }
        for t in &s.types {
            if reported {
                break;
            }
            if let Some(cycle) = visit(s, &t.name, &mut state, &edges, &mut Vec::new()) {
                reported = true;
                let at = s.get(&cycle[0]).map_or(t.span, |d| self.name_span(d.span));
                self.diags.push(
                    err("korek::recursive_type", format!("recursive types are not supported yet: {}", cycle.join(" → ")), at, "part of the cycle")
                );
            }
        }
    }

    fn constraint_check(&mut self, ty: &Type, c: &Constraints, item: Span) {
        if c.is_empty() {
            return;
        }
        let at = item;
        let ty_name = ty.display();
        let mut bad = |what: &str, ok: bool| {
            if !ok {
                self.diags.push(err("korek::bad_constraint", format!("`{what}` does not apply to `{ty_name}`"), at, "here"));
            }
        };
        bad("min", c.min.is_none() || ty.is_numeric());
        bad("max", c.max.is_none() || ty.is_numeric());
        bad("min_len", c.min_len.is_none() || ty.has_len());
        bad("max_len", c.max_len.is_none() || ty.has_len());
        bad("pattern", c.pattern.is_none() || *ty == Type::String);
        if let (Some(a), Some(b)) = (c.min, c.max) {
            if a > b {
                self.diags.push(err("korek::bad_constraint", format!("min {a} is above max {b}"), at, "empty range"));
            }
        }
        if let (Some(a), Some(b)) = (c.min_len, c.max_len) {
            if a > b {
                self.diags.push(err("korek::bad_constraint", format!("min_len {a} is above max_len {b}"), at, "empty range"));
            }
        }
        if let Some(Err(e)) = c.pattern.as_deref().map(regex::Regex::new) {
            self.diags.push(err("korek::bad_constraint", format!("invalid pattern: {e}"), at, "here"));
        }
        if *ty == Type::U64 && c.min.is_some_and(|m| m < 0.0) {
            self.diags.push(err("korek::bad_constraint", "a u64 cannot have a negative min", at, "here"));
        }
    }

    fn constraints(&mut self) {
        for t in &self.schema.types {
            if let TypeKind::Struct(s) = &t.kind {
                for f in &s.fields {
                    self.constraint_check(&f.ty, &f.constraints, f.span);
                }
            }
        }
        for g in &self.schema.keys {
            for k in &g.keys {
                self.constraint_check(&k.ty, &k.constraints, k.span);
            }
        }
    }

    /// Resolved defaults, keyed by key span.
    fn defaults(&mut self) {
        let mut resolved = Vec::new();
        for g in &self.schema.keys {
            for k in &g.keys {
                let Some(arg) = self.aux.default.get(&k.span) else { continue };
                match default_json(&k.ty, self.schema, &arg.value) {
                    Ok(j) => match resolve(self.schema, &k.ty, &j, &k.constraints) {
                        Ok(v) => resolved.push((k.span, v.to_json())),
                        Err(e) => self.diags.push(err("korek::bad_default", format!("bad default for `{}` ({}): {e}", k.name, k.ty.display()), arg.span, "this default")),
                    },
                    Err(e) => self.diags.push(err("korek::bad_default", format!("bad default for `{}` ({}): {e}", k.name, k.ty.display()), arg.span, "this default")),
                }
            }
        }
        self.resolved = resolved;
    }
}

/// Every type expression with whether it is a union variant, and the
/// span of the item it belongs to.
fn used_types(schema: &Schema) -> Vec<(&Type, bool, Span)> {
    let mut out = Vec::new();
    for t in &schema.types {
        match &t.kind {
            TypeKind::Enum(_) => {}
            TypeKind::Struct(s) => out.extend(s.fields.iter().map(|f| (&f.ty, false, f.span))),
            TypeKind::Union(u) => out.extend(u.variants.iter().map(|v| (&v.ty, true, v.span))),
        }
    }
    for g in &schema.keys {
        out.extend(g.keys.iter().map(|k| (&k.ty, false, k.span)));
    }
    out
}

/// A key default as JSON: native values for scalars and enums, a JSON
/// string for lists, maps, bytes, structs and unions (an untagged union also
/// takes a native value, or a string that is not JSON as itself).
fn default_json(ty: &Type, schema: &Schema, lit: &Literal) -> Result<serde_json::Value, String> {
    let compound = match ty {
        Type::List(_) | Type::Map(_) | Type::Bytes => Some(false),
        Type::Named(n) => match schema.get(n).map(|t| &t.kind) {
            Some(TypeKind::Struct(_)) => Some(false),
            Some(TypeKind::Union(_)) => Some(true),
            _ => None,
        },
        _ => None,
    };
    match (compound, lit) {
        (Some(lenient), Literal::String(s)) => match serde_json::from_str(s) {
            Ok(j) => Ok(j),
            Err(_) if lenient => Ok(serde_json::Value::String(s.clone())),
            Err(e) => Err(format!("expected a JSON string for a {}: {e}", ty.display())),
        },
        _ => Ok(lit.to_json()),
    }
}

/// The closest name within edit distance 2.
fn suggest<'a>(name: &str, names: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    names
        .map(|n| (lev(&name.to_lowercase(), &n.to_lowercase()), n))
        .filter(|(d, _)| *d <= 2)
        .min_by_key(|(d, _)| *d)
        .map(|(_, n)| n)
}

fn lev(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != *cb);
            cur.push(sub.min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}
