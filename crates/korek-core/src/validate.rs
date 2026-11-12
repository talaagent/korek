//! Checks on the lowered schema: names, references, unions, cycles,
//! constraints and defaults.

use std::collections::{HashMap, HashSet};

use korek_ir::codes;
use korek_ir::naming;
use korek_ir::{Diagnostic, Format, Json, RuleKind, Schema, Span, Type, TypeKind, Validate};

use crate::ext::{Arg, Literal};
use crate::parse::Aux;

fn err(code: &str, msg: impl Into<String>, span: Span, label: &str) -> Diagnostic {
    Diagnostic::error(code, msg).at(span, label)
}

pub(crate) struct Validator<'a> {
    pub schema: &'a Schema,
    pub aux: &'a Aux,
    pub diags: Vec<Diagnostic>,
    /// Defaults in canonical JSON, by field or key span.
    pub resolved: Vec<(Span, Json)>,
    /// Examples, by item span.
    pub examples: Vec<(Span, Vec<Json>)>,
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
                codes::DUPLICATE_NAME,
                format!("duplicate {what} `{name}`"),
                at,
                "declared again here",
            ));
        }
    }

    fn snake(&mut self, name: &str, span: Span, what: &str) {
        if !is_snake(name) {
            self.diags.push(err(
                codes::INVALID_NAME,
                format!("{what} `{name}` must be lower_snake_case"),
                self.name_span(span),
                "here",
            ));
        }
    }

    fn pascal(&mut self, name: &str, span: Span, what: &str) {
        if !is_pascal(name) {
            self.diags.push(err(
                codes::INVALID_NAME,
                format!("{what} `{name}` must be PascalCase"),
                self.name_span(span),
                "here",
            ));
        }
    }

    fn dotted(&mut self, name: &str, span: Span, what: &str) {
        if !name.split('.').all(is_snake) {
            self.diags.push(err(
                codes::INVALID_NAME,
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
        self.rules();
        if self.diags.is_empty() {
            self.values();
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
                        self.diags.push(err(
                            codes::EMPTY,
                            format!("enum `{}` has no variants", t.name),
                            self.name_span(t.span),
                            "add a variant",
                        ));
                    }
                    for v in &e.variants {
                        self.snake(&v.name, v.span, "variant");
                        self.dup(&mut seen, &v.name, v.span, "variant");
                        if v.name == "unspecified" {
                            self.diags.push(
                                err(codes::INVALID_NAME, "`unspecified` is reserved", self.name_span(v.span), "reserved")
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
                            self.diags.push(err(
                                codes::INVALID_NAME,
                                format!("`{}` cannot be a field name", f.name),
                                self.name_span(f.span),
                                "reserved in Rust",
                            ));
                        }
                    }
                }
                TypeKind::Union(u) => {
                    if u.variants.is_empty() {
                        self.diags.push(err(
                            codes::EMPTY,
                            format!("union `{}` has no variants", t.name),
                            self.name_span(t.span),
                            "add a variant",
                        ));
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
        self.names_keys(&top);
        let used = used_types(self.schema).into_iter().map(|(t, o, _)| (t, o));
        for w in naming::proto_wrappers(used) {
            let name = naming::proto_wrapper(&w);
            if let Some(at) = top.get(&name) {
                let at = *at;
                self.diags.push(err(
                    codes::NAME_CLASH,
                    format!(
                        "`{name}` is the Protobuf wrapper korek generates for `{}`",
                        w.display()
                    ),
                    at,
                    "rename this type",
                ));
            }
        }
    }

    fn names_keys(&mut self, top: &HashMap<String, Span>) {
        let mut groups = HashMap::new();
        for g in &self.schema.keys {
            self.dotted(&g.name, g.span, "keys block");
            self.dup(&mut groups, &g.ident(), g.span, "keys block");
            let generated = naming::keys_type(g);
            if let Some(at) = top.get(&generated) {
                let at = *at;
                self.diags.push(
                    err(codes::NAME_CLASH, format!("keys block `{}` generates the type `{generated}`, which is already declared", g.name), self.name_span(g.span), "generates a clashing type")
                        .help(format!("rename the type declared at byte {}", at.offset)),
                );
            }
            let mut seen = HashMap::new();
            for k in &g.keys {
                self.dotted(&k.name, k.span, "key");
                self.dup(
                    &mut seen,
                    &k.ident(),
                    k.span,
                    "key (dots count as underscores)",
                );
            }
        }
    }

    fn unknown(&mut self, name: &str, at: Span) {
        let names = self.schema.types.iter().map(|t| t.name.as_str());
        let mut d = err(
            codes::UNKNOWN_TYPE,
            format!("unknown type `{name}`"),
            at,
            "not declared",
        );
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
                for (name, at) in [
                    (&r.input, self.aux.ty_span.get(&r.span)),
                    (&r.output, self.aux.out_span.get(&r.span)),
                ] {
                    let at = at.copied().unwrap_or(r.span);
                    match self.schema.get(name).map(|t| &t.kind) {
                        None => self.unknown(name, at),
                        Some(TypeKind::Enum(_)) => self.diags.push(err(codes::INVALID_RPC, format!("`{name}` is an enum; rpc input and output must be a struct or union"), at, "not a message")),
                        Some(_) => {}
                    }
                }
            }
        }
    }

    fn unions(&mut self) {
        for t in &self.schema.types {
            let TypeKind::Union(u) = &t.kind else {
                continue;
            };
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
                            self.diags.push(err(
                                codes::INVALID_UNION,
                                format!(
                                    "struct `{}` has a field `{tag}`, the tag of union `{}`",
                                    v.ty.display(),
                                    t.name
                                ),
                                at,
                                "clashes with the tag",
                            ));
                        }
                    }
                    _ => self.diags.push(
                        err(
                            codes::INVALID_UNION,
                            format!(
                                "variant `{}` of tagged union `{}` must be a struct, found `{}`",
                                v.name,
                                t.name,
                                v.ty.display()
                            ),
                            at,
                            "not a struct",
                        )
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
        for t in &s.types {
            if reported {
                break;
            }
            if let Some(cycle) = visit(s, &t.name, &mut state, &edges, &mut Vec::new()) {
                reported = true;
                let at = s.get(&cycle[0]).map_or(t.span, |d| self.name_span(d.span));
                self.diags.push(err(
                    codes::CYCLE,
                    format!(
                        "recursive types are not supported yet: {}",
                        cycle.join(" → ")
                    ),
                    at,
                    "part of the cycle",
                ));
            }
        }
    }

    fn kind_of(&self, ty: &Type) -> Option<&TypeKind> {
        match ty {
            Type::Named(n) => self.schema.get(n).map(|t| &t.kind),
            _ => None,
        }
    }

    /// Why `kind` does not apply to `ty`, if it does not.
    fn inapplicable(&self, kind: &RuleKind, ty: &Type) -> Option<String> {
        use RuleKind as R;
        let named = self.kind_of(ty);
        let is_enum = matches!(named, Some(TypeKind::Enum(_)));
        let is_msg = matches!(named, Some(TypeKind::Struct(_) | TypeKind::Union(_)));
        let ok = match kind {
            R::Cel { .. } => true,
            R::Const { .. } | R::In { .. } | R::NotIn { .. } => {
                !is_msg
                    && !ty.is_collection()
                    && (*ty != Type::Timestamp || matches!(kind, R::Const { .. }))
            }
            R::Len { .. }
            | R::MinLen { .. }
            | R::MaxLen { .. }
            | R::Pattern { .. }
            | R::Prefix { .. }
            | R::Suffix { .. }
            | R::Contains { .. } => matches!(ty, Type::String | Type::Bytes),
            R::LenBytes { .. }
            | R::MinBytes { .. }
            | R::MaxBytes { .. }
            | R::NotContains { .. } => *ty == Type::String,
            R::Format { format } => {
                *ty == Type::String
                    || (*ty == Type::Bytes
                        && matches!(format, Format::Ip | Format::Ipv4 | Format::Ipv6))
            }
            R::Gt { .. } | R::Gte { .. } | R::Lt { .. } | R::Lte { .. } => {
                ty.is_numeric() || matches!(ty, Type::Timestamp | Type::Duration)
            }
            R::Finite => ty.is_float(),
            R::MultipleOf { .. } => ty.is_numeric(),
            R::MinItems { .. } | R::MaxItems { .. } | R::Unique => matches!(ty, Type::List(_)),
            R::MinPairs { .. } | R::MaxPairs { .. } => matches!(ty, Type::Map(_)),
            R::Required => is_msg || is_enum,
            R::DefinedOnly => is_enum,
            R::GtNow | R::LtNow | R::Within { .. } => *ty == Type::Timestamp,
            _ => false,
        };
        (!ok).then(|| format!("`{}` does not apply to `{}`", kind.name(), ty.display()))
    }

    /// A rule operand matched to the value type it constrains.
    fn operand(&self, kind: &RuleKind, ty: &Type, v: &Json) -> Result<(), String> {
        let ordinal = matches!(
            kind,
            RuleKind::Gt { .. }
                | RuleKind::Gte { .. }
                | RuleKind::Lt { .. }
                | RuleKind::Lte { .. }
                | RuleKind::MultipleOf { .. }
        );
        if ordinal && ty.is_numeric() {
            return v
                .as_f64()
                .map(|_| ())
                .ok_or_else(|| "expected a number".into());
        }
        korek_ir::value::resolve(self.schema, ty, v).map(|_| ())?;
        crate::check::check(self.schema, ty, v, &Validate::default())
    }

    #[allow(clippy::too_many_lines)]
    fn rules_for(&mut self, ty: &Type, v: &Validate, item: Span) {
        for r in &v.rules {
            if let Some(why) = self.inapplicable(&r.kind, ty) {
                self.diags
                    .push(err(codes::INVALID_RULE, why, r.span, "this rule"));
                continue;
            }
            let operands: Vec<Json> = match &r.kind {
                RuleKind::Const { value }
                | RuleKind::Gt { value }
                | RuleKind::Gte { value }
                | RuleKind::Lt { value }
                | RuleKind::Lte { value }
                | RuleKind::MultipleOf { value } => vec![value.clone()],
                RuleKind::In { values } | RuleKind::NotIn { values } => values.clone(),
                _ => Vec::new(),
            };
            for o in &operands {
                if let Err(e) = self.operand(&r.kind, ty, o) {
                    self.diags.push(err(
                        codes::INVALID_RULE,
                        format!("bad operand for `{}`: {e}", r.kind.name()),
                        r.span,
                        "this rule",
                    ));
                }
            }
            let problem = match &r.kind {
                RuleKind::Pattern { regex } => regex::Regex::new(regex)
                    .err()
                    .map(|e| format!("invalid pattern: {e}")),
                RuleKind::Within { duration } => korek_runtime::time::parse_duration(duration)
                    .is_none()
                    .then(|| format!("`{duration}` is not a duration like \"3600s\"")),
                RuleKind::MultipleOf { value } => {
                    (value.as_f64() == Some(0.0)).then(|| "multiple_of 0".to_owned())
                }
                _ => None,
            };
            if let Some(p) = problem {
                self.diags
                    .push(err(codes::INVALID_RULE, p, r.span, "this rule"));
            }
        }
        let range = |a: Option<u64>, b: Option<u64>| matches!((a, b), (Some(a), Some(b)) if a > b);
        let n = |f: fn(&RuleKind) -> Option<u64>| v.rules.iter().find_map(|r| f(&r.kind));
        if range(
            n(|k| {
                if let RuleKind::MinLen { n } = k {
                    Some(*n)
                } else {
                    None
                }
            }),
            n(|k| {
                if let RuleKind::MaxLen { n } = k {
                    Some(*n)
                } else {
                    None
                }
            }),
        ) || range(
            n(|k| {
                if let RuleKind::MinItems { n } = k {
                    Some(*n)
                } else {
                    None
                }
            }),
            n(|k| {
                if let RuleKind::MaxItems { n } = k {
                    Some(*n)
                } else {
                    None
                }
            }),
        ) {
            self.diags.push(err(
                codes::INVALID_RULE,
                "the minimum length is above the maximum",
                item,
                "empty range",
            ));
        }
        let at = |k: &RuleKind| match k {
            RuleKind::Gte { value } | RuleKind::Gt { value } => value.as_f64().map(|x| (true, x)),
            RuleKind::Lte { value } | RuleKind::Lt { value } => value.as_f64().map(|x| (false, x)),
            _ => None,
        };
        let lo = v
            .rules
            .iter()
            .filter_map(|r| at(&r.kind))
            .filter(|x| x.0)
            .map(|x| x.1)
            .fold(f64::NEG_INFINITY, f64::max);
        let hi = v
            .rules
            .iter()
            .filter_map(|r| at(&r.kind))
            .filter(|x| !x.0)
            .map(|x| x.1)
            .fold(f64::INFINITY, f64::min);
        if ty.is_numeric() && lo > hi {
            self.diags.push(err(
                codes::INVALID_RULE,
                format!("the lower bound {lo} is above the upper bound {hi}"),
                item,
                "empty range",
            ));
        }
        let nested = [
            (
                "items",
                &v.items,
                if let Type::List(t) = ty {
                    Some((**t).clone())
                } else {
                    None
                },
            ),
            (
                "keys",
                &v.keys,
                if let Type::Map(_) = ty {
                    Some(Type::String)
                } else {
                    None
                },
            ),
            (
                "values",
                &v.values,
                if let Type::Map(t) = ty {
                    Some((**t).clone())
                } else {
                    None
                },
            ),
        ];
        for (what, block, inner) in nested {
            match (block, inner) {
                (Some(b), Some(t)) => self.rules_for(&t, b, item),
                (Some(_), None) => self.diags.push(err(
                    codes::INVALID_RULE,
                    format!("`{what}` does not apply to `{}`", ty.display()),
                    item,
                    "here",
                )),
                _ => {}
            }
        }
    }

    fn rules(&mut self) {
        let s = self.schema;
        for t in &s.types {
            for r in &t.validate.rules {
                let ok = matches!(
                    (&r.kind, &t.kind),
                    (RuleKind::Cel { .. }, _) | (RuleKind::Required, TypeKind::Union(_))
                );
                if !ok {
                    self.diags.push(err(
                        codes::INVALID_RULE,
                        format!(
                            "`{}` does not apply to the type `{}` itself",
                            r.kind.name(),
                            t.name
                        ),
                        r.span,
                        "put it on a field",
                    ));
                }
            }
            if let TypeKind::Struct(st) = &t.kind {
                for f in &st.fields {
                    self.rules_for(&f.ty, &f.validate, f.span);
                }
            }
        }
        for g in &s.keys {
            for k in &g.keys {
                self.rules_for(&k.ty, &k.validate, k.span);
            }
        }
    }

    /// Resolves one default or example against its type and rules.
    fn value(&self, ty: &Type, v: &Validate, arg: &Arg) -> Result<Json, String> {
        let j = literal_json(ty, self.schema, &arg.value)?;
        korek_ir::value::resolve(self.schema, ty, &j)?;
        crate::check::check(self.schema, ty, &j, v)?;
        Ok(j)
    }

    fn values(&mut self) {
        let s = self.schema;
        let empty = Validate::default();
        let mut items: Vec<(Span, &Type, &Validate)> = Vec::new();
        let named: Vec<(Span, Type)> = s
            .types
            .iter()
            .map(|t| (t.span, Type::Named(t.name.clone())))
            .collect();
        for (span, ty) in &named {
            items.push((*span, ty, &empty));
        }
        for t in &s.types {
            if let TypeKind::Struct(st) = &t.kind {
                items.extend(st.fields.iter().map(|f| (f.span, &f.ty, &f.validate)));
            }
        }
        for g in &s.keys {
            items.extend(g.keys.iter().map(|k| (k.span, &k.ty, &k.validate)));
        }
        for (span, ty, v) in items {
            if let Some(arg) = self.aux.default.get(&span) {
                match self.value(ty, v, arg) {
                    Ok(j) => self.resolved.push((span, j)),
                    Err(e) => self.diags.push(err(
                        codes::INVALID_DEFAULT,
                        format!("invalid default ({}): {e}", ty.display()),
                        arg.span,
                        "this default",
                    )),
                }
            }
            if let Some(args) = self.aux.examples.get(&span) {
                let mut ok = Vec::new();
                for arg in args {
                    match self.value(ty, v, arg) {
                        Ok(j) => ok.push(j),
                        Err(e) => self.diags.push(err(
                            codes::INVALID_EXAMPLE,
                            format!("invalid example ({}): {e}", ty.display()),
                            arg.span,
                            "this example",
                        )),
                    }
                }
                self.examples.push((span, ok));
            }
        }
        let placed: std::collections::HashSet<Span> = self.examples.iter().map(|e| e.0).collect();
        for (span, args) in &self.aux.examples {
            if !placed.contains(span) {
                if let Some(a) = args.first() {
                    self.diags.push(err(
                        codes::INVALID_EXAMPLE,
                        "examples go on types, fields and keys",
                        a.span,
                        "not here",
                    ));
                }
            }
        }
    }
}

/// A default or example as JSON: native values for scalars and enums, a
/// JSON string for lists, maps, structs and tagged unions (an untagged
/// union also takes a native value, or a string that is not JSON as itself).
fn literal_json(ty: &Type, schema: &Schema, lit: &Literal) -> Result<Json, String> {
    let compound = match ty {
        Type::List(_) | Type::Map(_) => Some(false),
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
            Err(_) if lenient => Ok(Json::String(s.clone())),
            Err(e) => Err(format!(
                "expected a JSON string for a {}: {e}",
                ty.display()
            )),
        },
        (_, Literal::Float(f)) if ty.is_integer() => Err(format!("{f} is not an integer")),
        _ => Ok(lit.to_json()),
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

#[allow(clippy::type_complexity)]
fn visit<'s>(
    s: &'s Schema,
    name: &'s str,
    state: &mut HashMap<&'s str, u8>,
    edges: &dyn Fn(&crate::ir::TypeDef) -> Vec<String>,
    path: &mut Vec<&'s str>,
) -> Option<Vec<String>> {
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

/// The closest name within edit distance 2.
pub(crate) fn suggest<'a>(name: &str, names: impl Iterator<Item = &'a str>) -> Option<&'a str> {
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

pub(crate) fn is_snake(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !s.ends_with('_')
        && !s.contains("__")
}

pub(crate) fn is_pascal(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_uppercase()) && chars.all(|c| c.is_ascii_alphanumeric())
}
