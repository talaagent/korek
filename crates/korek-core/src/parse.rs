//! Source → IR. Reads the grammar; numbers, defaults and examples are
//! finished by later passes from the side table [`Aux`].

use std::collections::HashMap;

use korek_ir::codes;
use korek_ir::{
    Attrs, Diagnostic, EnumDef, EnumVariant, Field, Format, Idempotency, Json, Key, KeyGroup,
    Level, Meta, Rpc, Rule, RuleKind, Schema, Service, Span, StructDef, Type, TypeDef, TypeKind,
    UnionDef, UnionVariant, Validate,
};

use crate::ext::{Arg, Extension, ItemKind, Literal, Node};
use crate::source::Sources;
use crate::syntax;
use crate::types::parse_type;

/// What later passes need that the IR does not keep, keyed by item span.
#[derive(Default)]
pub(crate) struct Aux {
    pub ty_span: HashMap<Span, Span>,
    /// An rpc's `output=` (its `input=` is in `ty_span`).
    pub out_span: HashMap<Span, Span>,
    pub id: HashMap<Span, (u32, Span)>,
    pub default: HashMap<Span, Arg>,
    pub examples: HashMap<Span, Vec<Arg>>,
    pub name_span: HashMap<Span, Span>,
    /// Rule operands that are values (`const`, `in`, `gt`, …), by rule span.
    pub operands: HashMap<Span, Vec<Arg>>,
    pub package_span: Option<Span>,
}

pub(crate) struct Parser<'e> {
    pub exts: &'e [&'e dyn Extension],
    pub diags: Vec<Diagnostic>,
    pub aux: Aux,
    pub schema: Schema,
}

fn err(code: &str, msg: impl Into<String>, span: Span, label: &str) -> Diagnostic {
    Diagnostic::error(code, msg).at(span, label)
}

/// Properties every item accepts.
const COMMON_PROPS: &[&str] = &["doc", "secret", "deprecated", "since", "unit"];

/// Rule names, for parsing and suggestions.
pub(crate) const RULES: &[&str] = &[
    "const",
    "in",
    "not_in",
    "len",
    "min_len",
    "max_len",
    "len_bytes",
    "min_bytes",
    "max_bytes",
    "pattern",
    "prefix",
    "suffix",
    "contains",
    "not_contains",
    "gt",
    "gte",
    "lt",
    "lte",
    "finite",
    "multiple_of",
    "min_items",
    "max_items",
    "unique",
    "min_pairs",
    "max_pairs",
    "required",
    "defined_only",
    "gt_now",
    "lt_now",
    "within",
    "cel",
    "items",
    "keys",
    "values",
];

impl<'e> Parser<'e> {
    pub fn new(exts: &'e [&'e dyn Extension]) -> Self {
        Self {
            exts,
            diags: Vec::new(),
            aux: Aux::default(),
            schema: Schema {
                package: "korek.v1".into(),
                files: Vec::new(),
                types: Vec::new(),
                keys: Vec::new(),
                services: Vec::new(),
                ext: korek_ir::Ext::new(),
            },
        }
    }

    /// Parses (with kdl-load, one thread per file) and lowers every file,
    /// in file order.
    pub fn files(&mut self, sources: &Sources) {
        let files = sources.files();
        for (file, doc) in files.iter().zip(syntax::parse_all(files)) {
            #[allow(clippy::cast_possible_truncation)]
            let id = self.schema.files.len() as u32;
            self.schema.files.push(file.name().to_owned());
            match doc {
                Ok(doc) => {
                    for n in doc.nodes(id) {
                        self.top(n);
                    }
                }
                Err(problems) => {
                    for p in problems {
                        let mut diag =
                            Diagnostic::error(codes::SYNTAX, p.message).at(p.span, p.label);
                        diag.help = p.help;
                        self.diags.push(diag);
                    }
                }
            }
        }
    }

    fn top(&mut self, node: Node<'_>) {
        match node.name() {
            "package" => self.package(node),
            "enum" | "struct" | "union" => self.type_def(node),
            "keys" => self.keys(node),
            "service" => self.service(node),
            other => {
                let r = match self.exts.iter().find(|x| x.claims_top_level(other)) {
                    Some(x) => x.top_level(node).map(|v| {
                        self.schema.ext.entry(x.name().to_owned()).or_default().push(v);
                    }),
                    None => Err(err(
                        codes::UNKNOWN_NODE,
                        format!("unknown top-level node `{other}`"),
                        node.name_span(),
                        "not a korek node",
                    )
                    .help(suggest_or(
                        other,
                        &["package", "enum", "struct", "union", "keys", "service"],
                        "expected package, enum, struct, union, keys or service (or a node an extension registers)",
                    ))),
                };
                if let Err(d) = r {
                    self.diags.push(d);
                }
            }
        }
    }

    fn package(&mut self, node: Node<'_>) {
        let Some((name, span)) = self.name_arg(node, 1) else {
            return;
        };
        let ok = name.split('.').all(|s| {
            crate::validate::is_snake(s)
                || s.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                    && !s.is_empty()
        });
        if !ok {
            self.diags.push(err(
                codes::INVALID_NAME,
                format!("package `{name}` must be dotted lower_snake_case segments"),
                span,
                "here",
            ));
        }
        if self.aux.package_span.replace(span).is_some() {
            self.diags.push(err(
                codes::DUPLICATE_NAME,
                "the package is declared twice",
                span,
                "second declaration",
            ));
        }
        self.schema.package = name;
    }

    /// The first positional arg as a name; `want` positional args allowed.
    fn name_arg(&mut self, node: Node<'_>, want: usize) -> Option<(String, Span)> {
        let args = node.args();
        for extra in args.iter().skip(want) {
            self.diags.push(err(
                codes::INVALID_VALUE,
                format!("unexpected argument on `{}`", node.name()),
                extra.span,
                "not expected",
            ));
        }
        match args.first() {
            None => {
                self.diags.push(err(
                    codes::MISSING_VALUE,
                    format!("`{}` needs a name", node.name()),
                    node.name_span(),
                    "no name",
                ));
                None
            }
            Some(Arg {
                value: Literal::String(s),
                span,
            }) => Some((s.clone(), *span)),
            Some(a) => {
                self.diags.push(err(
                    codes::INVALID_NAME,
                    format!("expected a name, found {}", a.value.describe()),
                    a.span,
                    "not a string",
                ));
                None
            }
        }
    }

    /// The second positional arg: a type expression.
    fn type_arg(&mut self, node: Node<'_>, item: Span) -> Option<Type> {
        let Some(arg) = node.args().into_iter().nth(1) else {
            self.diags.push(err(
                codes::MISSING_VALUE,
                format!("`{}` needs a type", node.name()),
                node.span(),
                "add a type, e.g. \"string\"",
            ));
            return None;
        };
        let Literal::String(s) = &arg.value else {
            self.diags.push(err(
                codes::INVALID_TYPE,
                format!("expected a type, found {}", arg.value.describe()),
                arg.span,
                "not a type",
            ));
            return None;
        };
        self.aux.ty_span.insert(item, arg.span);
        match parse_type(s) {
            Ok(t) => Some(t),
            Err(msg) => {
                self.diags
                    .push(err(codes::INVALID_TYPE, msg, arg.span, "in this type"));
                None
            }
        }
    }

    fn str_prop(&mut self, node: Node<'_>, name: &str) -> Option<(String, Span)> {
        let a = node.prop(name)?;
        if let Literal::String(s) = a.value {
            Some((s, a.span))
        } else {
            self.diags.push(err(
                codes::INVALID_VALUE,
                format!("`{name}` must be a string"),
                a.span,
                "not a string",
            ));
            None
        }
    }

    fn bool_prop(&mut self, node: Node<'_>, name: &str) -> bool {
        match node.prop(name) {
            None => false,
            Some(Arg {
                value: Literal::Bool(b),
                ..
            }) => b,
            Some(a) => {
                self.diags.push(err(
                    codes::INVALID_VALUE,
                    format!("`{name}` must be #true or #false"),
                    a.span,
                    "not a boolean",
                ));
                false
            }
        }
    }

    fn id_prop(&mut self, node: Node<'_>, item: Span) {
        let Some(arg) = node.prop("id") else { return };
        match arg.value {
            Literal::Int(i) if (1..=536_870_911).contains(&i) => {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                self.aux.id.insert(item, (i as u32, arg.span));
            }
            _ => self.diags.push(err(
                codes::INVALID_ID,
                "`id` must be an integer from 1 to 536870911",
                arg.span,
                "bad id",
            )),
        }
    }

    /// Hands an unknown property to the extension that claims it.
    fn foreign_prop(&mut self, kind: ItemKind, name: &str, arg: &Arg, attrs: &mut Attrs) {
        let r = match self.exts.iter().find(|x| x.claims_property(kind, name)) {
            Some(x) => x
                .property(kind, name, arg)
                .map(|v| attrs.ext.entry(x.name().to_owned()).or_default().push(v)),
            None => Err(err(
                codes::UNKNOWN_PROPERTY,
                format!("unknown property `{name}`"),
                arg.span,
                "not recognized here",
            )),
        };
        if let Err(d) = r {
            self.diags.push(d);
        }
    }

    fn foreign_child(&mut self, kind: ItemKind, node: Node<'_>, attrs: &mut Attrs) {
        let r = match self.exts.iter().find(|x| x.claims_child(kind, node.name())) {
            Some(x) => x
                .child(kind, node)
                .map(|v| attrs.ext.entry(x.name().to_owned()).or_default().push(v)),
            None => Err(err(
                codes::UNKNOWN_NODE,
                format!("unknown node `{}` here", node.name()),
                node.name_span(),
                "not recognized here",
            )
            .help(suggest_or(
                node.name(),
                ATTR_NODES,
                "see the schema reference in the README",
            ))),
        };
        if let Err(d) = r {
            self.diags.push(d);
        }
    }

    /// Common properties (`doc=`, `secret=`, …) and the item's own `known`;
    /// the rest go to extensions.
    fn props(&mut self, node: Node<'_>, kind: ItemKind, known: &[&str], attrs: &mut Attrs) {
        for (name, arg) in node.props() {
            match (name, &arg.value) {
                (_, _) if known.contains(&name) => {}
                ("doc", Literal::String(s)) => attrs.doc = Some(s.clone()),
                ("deprecated", Literal::String(s)) => attrs.deprecated = Some(s.clone()),
                ("deprecated", Literal::Bool(true)) => attrs.deprecated = Some(String::new()),
                ("since", Literal::String(s)) => attrs.since = Some(s.clone()),
                ("unit", Literal::String(s)) => attrs.unit = Some(s.clone()),
                ("secret", Literal::Bool(b)) => attrs.secret = *b,
                (n, _) if COMMON_PROPS.contains(&n) => self.diags.push(err(
                    codes::INVALID_VALUE,
                    format!("bad value for `{n}`"),
                    arg.span,
                    "here",
                )),
                _ => self.foreign_prop(kind, name, &arg, attrs),
            }
        }
    }

    fn strings(&mut self, node: Node<'_>) -> Vec<String> {
        node.args()
            .into_iter()
            .filter_map(|a| match a.value {
                Literal::String(s) => Some(s),
                other => {
                    self.diags.push(err(
                        codes::INVALID_VALUE,
                        format!(
                            "`{}` takes strings, found {}",
                            node.name(),
                            other.describe()
                        ),
                        a.span,
                        "not a string",
                    ));
                    None
                }
            })
            .collect()
    }

    fn one_string(&mut self, node: Node<'_>) -> Option<String> {
        let mut v = self.strings(node);
        if v.len() == 1 {
            v.pop()
        } else {
            self.diags.push(err(
                codes::INVALID_VALUE,
                format!("`{}` takes one string", node.name()),
                node.span(),
                "here",
            ));
            None
        }
    }

    /// A child every item accepts; returns false when it is not one.
    fn attr_child(&mut self, ch: Node<'_>, item: Span, attrs: &mut Attrs) -> bool {
        match ch.name() {
            "doc" => {
                let text = self.strings(ch).join("\n");
                attrs.doc = Some(match attrs.doc.take() {
                    Some(d) => format!("{d}\n{text}"),
                    None => text,
                });
            }
            "description" => attrs.description = self.one_string(ch),
            "example" => match ch.args().into_iter().next() {
                Some(a) => self.aux.examples.entry(item).or_default().push(a),
                None => self.diags.push(err(
                    codes::MISSING_VALUE,
                    "`example` needs a value",
                    ch.span(),
                    "here",
                )),
            },
            "unit" => attrs.unit = self.one_string(ch),
            "deprecated" => attrs.deprecated = Some(self.strings(ch).join(" ")),
            "since" => attrs.since = self.one_string(ch),
            "alias" | "aliases" => {
                let a = self.strings(ch);
                attrs.aliases.extend(a);
            }
            "secret" => attrs.secret = true,
            "label" | "section" | "widget" | "level" => {
                let v = self.one_string(ch);
                let ui = attrs.ui.get_or_insert_with(Default::default);
                match ch.name() {
                    "label" => ui.label = v,
                    "section" => ui.section = v,
                    "widget" => ui.widget = v,
                    _ => {
                        ui.level = match v.as_deref() {
                            Some("basic") => Some(Level::Basic),
                            Some("advanced") => Some(Level::Advanced),
                            _ => {
                                self.diags.push(err(
                                    codes::INVALID_VALUE,
                                    "`level` must be \"basic\" or \"advanced\"",
                                    ch.span(),
                                    "here",
                                ));
                                None
                            }
                        }
                    }
                }
            }
            "meta" => {
                let namespace = self.one_string(ch).unwrap_or_default();
                let mut props: alloc::collections::BTreeMap<String, Json> = ch
                    .props()
                    .into_iter()
                    .map(|(k, a)| (k.to_owned(), a.value.to_json()))
                    .collect();
                for c in ch.children() {
                    props.insert(c.name().to_owned(), c.to_json());
                }
                attrs.meta.push(Meta { namespace, props });
            }
            _ => return false,
        }
        true
    }

    /// Walks an item's children: attributes, `validate` when `v` is given,
    /// `own` for item-specific children, extensions for the rest.
    fn body(
        &mut self,
        node: Node<'_>,
        kind: ItemKind,
        attrs: &mut Attrs,
        mut v: Option<&mut Validate>,
        mut own: impl FnMut(&mut Self, Node<'_>) -> bool,
    ) {
        let item = node.span();
        for ch in node.children() {
            if self.attr_child(ch, item, attrs) || own(self, ch) {
                continue;
            }
            match (ch.name(), v.as_deref_mut()) {
                ("validate", Some(v)) => self.validate_block(ch, v),
                _ => self.foreign_child(kind, ch, attrs),
            }
        }
    }

    fn validate_block(&mut self, node: Node<'_>, v: &mut Validate) {
        for ch in node.children() {
            match ch.name() {
                "items" | "keys" | "values" => {
                    let mut inner = Validate::default();
                    self.validate_block(ch, &mut inner);
                    let slot = match ch.name() {
                        "items" => &mut v.items,
                        "keys" => &mut v.keys,
                        _ => &mut v.values,
                    };
                    *slot = Some(Box::new(inner));
                }
                _ => {
                    if let Some(r) = self.rule(ch) {
                        v.rules.push(r);
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn rule(&mut self, node: Node<'_>) -> Option<Rule> {
        let name = node.name();
        let span = node.span();
        let args = node.args();
        let message = self.str_prop(node, "message").map(|m| m.0);
        for (p, a) in node.props() {
            if !matches!(p, "message" | "id" | "expression" | "ts" | "rust") {
                self.diags.push(err(
                    codes::UNKNOWN_PROPERTY,
                    format!("unknown property `{p}` on rule `{name}`"),
                    a.span,
                    "here",
                ));
            }
        }
        let bad = |what: &str| {
            err(
                codes::INVALID_RULE,
                format!("`{name}` takes {what}"),
                span,
                "here",
            )
        };
        let n = || match args.as_slice() {
            [
                Arg {
                    value: Literal::Int(i),
                    ..
                },
            ] if *i >= 0 => u64::try_from(*i).ok(),
            _ => None,
        };
        let s = || match args.as_slice() {
            [
                Arg {
                    value: Literal::String(s),
                    ..
                },
            ] => Some(s.clone()),
            _ => None,
        };
        let no_args = args.is_empty();
        let value = |p: &mut Self| {
            if args.len() == 1 {
                p.aux.operands.insert(span, args.clone());
                Some(args[0].value.to_json())
            } else {
                None
            }
        };
        macro_rules! need {
            ($e:expr, $what:expr) => {
                match $e {
                    Some(x) => x,
                    None => {
                        self.diags.push(bad($what));
                        return None;
                    }
                }
            };
        }
        let kind = match name {
            "len" => RuleKind::Len {
                n: need!(n(), "a non-negative integer"),
            },
            "min_len" => RuleKind::MinLen {
                n: need!(n(), "a non-negative integer"),
            },
            "max_len" => RuleKind::MaxLen {
                n: need!(n(), "a non-negative integer"),
            },
            "len_bytes" => RuleKind::LenBytes {
                n: need!(n(), "a non-negative integer"),
            },
            "min_bytes" => RuleKind::MinBytes {
                n: need!(n(), "a non-negative integer"),
            },
            "max_bytes" => RuleKind::MaxBytes {
                n: need!(n(), "a non-negative integer"),
            },
            "min_items" => RuleKind::MinItems {
                n: need!(n(), "a non-negative integer"),
            },
            "max_items" => RuleKind::MaxItems {
                n: need!(n(), "a non-negative integer"),
            },
            "min_pairs" => RuleKind::MinPairs {
                n: need!(n(), "a non-negative integer"),
            },
            "max_pairs" => RuleKind::MaxPairs {
                n: need!(n(), "a non-negative integer"),
            },
            "pattern" => RuleKind::Pattern {
                regex: need!(s(), "a regular expression"),
            },
            "prefix" => RuleKind::Prefix {
                value: need!(s(), "a string"),
            },
            "suffix" => RuleKind::Suffix {
                value: need!(s(), "a string"),
            },
            "contains" => RuleKind::Contains {
                value: need!(s(), "a string"),
            },
            "not_contains" => RuleKind::NotContains {
                value: need!(s(), "a string"),
            },
            "within" => RuleKind::Within {
                duration: need!(s(), "a duration like \"3600s\""),
            },
            "const" => RuleKind::Const {
                value: need!(value(self), "one value"),
            },
            "gt" => RuleKind::Gt {
                value: need!(value(self), "one value"),
            },
            "gte" => RuleKind::Gte {
                value: need!(value(self), "one value"),
            },
            "lt" => RuleKind::Lt {
                value: need!(value(self), "one value"),
            },
            "lte" => RuleKind::Lte {
                value: need!(value(self), "one value"),
            },
            "multiple_of" => RuleKind::MultipleOf {
                value: need!(value(self), "one number"),
            },
            "in" | "not_in" => {
                if args.is_empty() {
                    self.diags.push(bad("one or more values"));
                    return None;
                }
                self.aux.operands.insert(span, args.clone());
                let values = args.iter().map(|a| a.value.to_json()).collect();
                if name == "in" {
                    RuleKind::In { values }
                } else {
                    RuleKind::NotIn { values }
                }
            }
            "finite" | "unique" | "required" | "defined_only" | "gt_now" | "lt_now" if !no_args => {
                self.diags.push(bad("no arguments"));
                return None;
            }
            "finite" => RuleKind::Finite,
            "unique" => RuleKind::Unique,
            "required" => RuleKind::Required,
            "defined_only" => RuleKind::DefinedOnly,
            "gt_now" => RuleKind::GtNow,
            "lt_now" => RuleKind::LtNow,
            "cel" => {
                let id = self.str_prop(node, "id").map(|x| x.0);
                let expression = self.str_prop(node, "expression").map(|x| x.0);
                let (Some(id), Some(expression)) = (id, expression) else {
                    self.diags.push(bad("`id=\"…\"` and `expression=\"…\"`"));
                    return None;
                };
                RuleKind::Cel {
                    id,
                    expression,
                    ts: self.str_prop(node, "ts").map(|x| x.0),
                    rust: self.str_prop(node, "rust").map(|x| x.0),
                }
            }
            other => match Format::from_name(other) {
                Some(format) if no_args => RuleKind::Format { format },
                Some(_) => {
                    self.diags.push(bad("no arguments"));
                    return None;
                }
                None => {
                    let all: Vec<&str> = RULES
                        .iter()
                        .copied()
                        .chain(Format::ALL.iter().map(|f| f.name()))
                        .collect();
                    self.diags.push(
                        err(
                            codes::UNKNOWN_RULE,
                            format!("unknown rule `{other}`"),
                            node.name_span(),
                            "not a rule",
                        )
                        .help(suggest_or(
                            other,
                            &all,
                            "see VALIDATION.md for every rule",
                        )),
                    );
                    return None;
                }
            },
        };
        Some(Rule {
            kind,
            message,
            span,
        })
    }

    fn type_def(&mut self, node: Node<'_>) {
        let Some((name, name_span)) = self.name_arg(node, 1) else {
            return;
        };
        let span = node.span();
        self.aux.name_span.insert(span, name_span);
        let mut attrs = Attrs::default();
        let mut validate = Validate::default();
        let (kind, item_kind) = match node.name() {
            "enum" => (
                TypeKind::Enum(EnumDef {
                    variants: Vec::new(),
                    retired: Vec::new(),
                }),
                ItemKind::Enum,
            ),
            "struct" => (
                TypeKind::Struct(StructDef {
                    fields: Vec::new(),
                    retired: Vec::new(),
                }),
                ItemKind::Struct,
            ),
            _ => {
                let tag = self.str_prop(node, "tag");
                if let Some((t, s)) = &tag {
                    if !crate::validate::is_snake(t) {
                        self.diags.push(err(
                            codes::INVALID_NAME,
                            format!("tag `{t}` must be lower_snake_case"),
                            *s,
                            "here",
                        ));
                    }
                }
                (
                    TypeKind::Union(UnionDef {
                        tag: tag.map(|t| t.0),
                        variants: Vec::new(),
                        retired: Vec::new(),
                    }),
                    ItemKind::Union,
                )
            }
        };
        self.props(node, item_kind, &["tag"], &mut attrs);
        let mut kind = kind;
        self.body(node, item_kind, &mut attrs, Some(&mut validate), |p, ch| {
            match (&mut kind, ch.name()) {
                (TypeKind::Enum(e), "variant") => {
                    if let Some(v) = p.enum_variant(ch) {
                        e.variants.push(v);
                    }
                }
                (TypeKind::Struct(s), "field") => {
                    if let Some(f) = p.field(ch) {
                        s.fields.push(f);
                    }
                }
                (TypeKind::Union(u), "variant") => {
                    if let Some(v) = p.union_variant(ch) {
                        u.variants.push(v);
                    }
                }
                _ => return false,
            }
            true
        });
        self.schema.types.push(TypeDef {
            name,
            kind,
            attrs,
            validate,
            span,
        });
    }

    fn enum_variant(&mut self, node: Node<'_>) -> Option<EnumVariant> {
        let (name, name_span) = self.name_arg(node, 1)?;
        let span = node.span();
        self.aux.name_span.insert(span, name_span);
        let mut attrs = Attrs::default();
        self.props(node, ItemKind::EnumVariant, &["id"], &mut attrs);
        self.id_prop(node, span);
        self.body(node, ItemKind::EnumVariant, &mut attrs, None, |_, _| false);
        Some(EnumVariant {
            name,
            number: 0,
            attrs,
            span,
        })
    }

    fn field(&mut self, node: Node<'_>) -> Option<Field> {
        let (name, name_span) = self.name_arg(node, 2)?;
        let span = node.span();
        self.aux.name_span.insert(span, name_span);
        let ty = self.type_arg(node, span);
        let mut attrs = Attrs::default();
        let mut validate = Validate::default();
        self.props(
            node,
            ItemKind::Field,
            &["id", "optional", "default"],
            &mut attrs,
        );
        if let Some(d) = node.prop("default") {
            self.aux.default.insert(span, d);
        }
        self.id_prop(node, span);
        let optional = self.bool_prop(node, "optional");
        self.body(
            node,
            ItemKind::Field,
            &mut attrs,
            Some(&mut validate),
            |_, _| false,
        );
        Some(Field {
            name,
            ty: ty.unwrap_or(Type::String),
            optional,
            default: None,
            validate,
            number: 0,
            attrs,
            span,
        })
    }

    fn union_variant(&mut self, node: Node<'_>) -> Option<UnionVariant> {
        let (name, name_span) = self.name_arg(node, 2)?;
        let span = node.span();
        self.aux.name_span.insert(span, name_span);
        let ty = self.type_arg(node, span)?;
        let mut attrs = Attrs::default();
        self.props(node, ItemKind::UnionVariant, &["id"], &mut attrs);
        self.id_prop(node, span);
        self.body(node, ItemKind::UnionVariant, &mut attrs, None, |_, _| false);
        Some(UnionVariant {
            name,
            ty,
            number: 0,
            attrs,
            span,
        })
    }

    fn keys(&mut self, node: Node<'_>) {
        let Some((name, name_span)) = self.name_arg(node, 1) else {
            return;
        };
        let span = node.span();
        self.aux.name_span.insert(span, name_span);
        let mut attrs = Attrs::default();
        let mut keys = Vec::new();
        self.props(node, ItemKind::KeyGroup, &[], &mut attrs);
        self.body(node, ItemKind::KeyGroup, &mut attrs, None, |p, ch| {
            if ch.name() != "key" {
                return false;
            }
            if let Some(k) = p.key(ch) {
                keys.push(k);
            }
            true
        });
        self.schema.keys.push(KeyGroup {
            name,
            keys,
            retired: Vec::new(),
            attrs,
            span,
        });
    }

    fn key(&mut self, node: Node<'_>) -> Option<Key> {
        let (name, name_span) = self.name_arg(node, 2)?;
        let span = node.span();
        self.aux.name_span.insert(span, name_span);
        let ty = self.type_arg(node, span)?;
        let mut attrs = Attrs::default();
        let mut validate = Validate::default();
        self.props(node, ItemKind::Key, &["id", "default"], &mut attrs);
        if let Some(d) = node.prop("default") {
            self.aux.default.insert(span, d);
        }
        self.id_prop(node, span);
        self.body(
            node,
            ItemKind::Key,
            &mut attrs,
            Some(&mut validate),
            |_, _| false,
        );
        Some(Key {
            name,
            ty,
            default: None,
            validate,
            number: 0,
            attrs,
            span,
        })
    }

    fn service(&mut self, node: Node<'_>) {
        let Some((name, name_span)) = self.name_arg(node, 1) else {
            return;
        };
        let span = node.span();
        self.aux.name_span.insert(span, name_span);
        let mut attrs = Attrs::default();
        let mut rpcs = Vec::new();
        self.props(node, ItemKind::Service, &[], &mut attrs);
        self.body(node, ItemKind::Service, &mut attrs, None, |p, ch| {
            if ch.name() != "rpc" {
                return false;
            }
            if let Some(r) = p.rpc(ch) {
                rpcs.push(r);
            }
            true
        });
        self.schema.services.push(Service {
            name,
            rpcs,
            attrs,
            span,
        });
    }

    fn rpc(&mut self, node: Node<'_>) -> Option<Rpc> {
        let (name, name_span) = self.name_arg(node, 1)?;
        let span = node.span();
        self.aux.name_span.insert(span, name_span);
        let io = |p: &mut Self, prop: &str| {
            let v = p.str_prop(node, prop);
            if v.is_none() && node.prop(prop).is_none() {
                p.diags.push(err(
                    codes::MISSING_VALUE,
                    format!("`rpc` needs `{prop}=\"<message type>\"`"),
                    node.span(),
                    "here",
                ));
            }
            v
        };
        let input = io(self, "input");
        let output = io(self, "output");
        let idempotency = match self.str_prop(node, "idempotency") {
            None => None,
            Some((s, _)) if s == "no_side_effects" => Some(Idempotency::NoSideEffects),
            Some((s, _)) if s == "idempotent" => Some(Idempotency::Idempotent),
            Some((s, sp)) => {
                self.diags.push(err(
                    codes::INVALID_VALUE,
                    format!("unknown idempotency `{s}`"),
                    sp,
                    "expected no_side_effects or idempotent",
                ));
                None
            }
        };
        let mut attrs = Attrs::default();
        self.props(
            node,
            ItemKind::Rpc,
            &["input", "output", "idempotency"],
            &mut attrs,
        );
        self.body(node, ItemKind::Rpc, &mut attrs, None, |_, _| false);
        let (input, in_span) = input?;
        let (output, out_span) = output?;
        self.aux.ty_span.insert(span, in_span);
        self.aux.out_span.insert(span, out_span);
        Some(Rpc {
            name,
            input,
            output,
            idempotency,
            attrs,
            span,
        })
    }
}

/// Attribute child names, for suggestions.
const ATTR_NODES: &[&str] = &[
    "doc",
    "description",
    "example",
    "unit",
    "deprecated",
    "alias",
    "aliases",
    "since",
    "secret",
    "label",
    "section",
    "widget",
    "level",
    "meta",
    "validate",
    "field",
    "variant",
    "key",
    "rpc",
];

/// "did you mean `x`?" for the closest of `names`, else `fallback`.
pub(crate) fn suggest_or(name: &str, names: &[&str], fallback: &str) -> String {
    match crate::validate::suggest(name, names.iter().copied()) {
        Some(best) => format!("did you mean `{best}`?"),
        None => fallback.to_owned(),
    }
}
