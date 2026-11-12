//! Source → IR: reads the grammar, leaving numbers and defaults to later
//! passes (which use the side table [`Aux`]).

use std::collections::HashMap;

use kdl::KdlDocument;

use crate::diag::{Diagnostic, SourceFile};
use crate::ext::{Arg, Extension, ItemKind, Literal, Node, span_of};
use crate::ir::{
    Constraints, EnumDef, EnumVariant, Ext, Field, Idempotency, Key, KeyGroup, Rpc, Schema,
    Service, Span, StructDef, Type, TypeDef, TypeKind, UnionDef, UnionVariant,
};
use crate::types::parse_type;

/// What later passes need that the IR does not keep, keyed by item span.
#[derive(Default)]
pub(crate) struct Aux {
    pub ty_span: HashMap<Span, Span>,
    pub id: HashMap<Span, (u32, Span)>,
    pub default: HashMap<Span, Arg>,
    pub name_span: HashMap<Span, Span>,
    /// An rpc's `output=` (its `input=` is in `ty_span`).
    pub out_span: HashMap<Span, Span>,
    pub package_span: Option<Span>,
}

const FIELD_PROPS: &[&str] = &[
    "doc", "id", "optional", "min", "max", "min_len", "max_len", "pattern",
];
const KEY_PROPS: &[&str] = &[
    "doc", "id", "default", "min", "max", "min_len", "max_len", "pattern",
];
const RPC_PROPS: &[&str] = &["doc", "input", "output", "idempotency"];

pub(crate) struct Parser<'e> {
    pub exts: &'e [Box<dyn Extension>],
    pub diags: Vec<Diagnostic>,
    pub aux: Aux,
    pub schema: Schema,
}

fn err(code: &str, msg: impl Into<String>, span: Span, label: &str) -> Diagnostic {
    Diagnostic::error(code, msg).at(span, label)
}

impl<'e> Parser<'e> {
    pub fn new(exts: &'e [Box<dyn Extension>]) -> Self {
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
                ext: Ext::new(),
            },
        }
    }

    pub fn file(&mut self, file: &SourceFile) {
        #[allow(clippy::cast_possible_truncation)]
        let id = self.schema.files.len() as u32;
        self.schema.files.push(file.path.clone());
        let doc = match KdlDocument::parse_v2(&file.contents) {
            Ok(d) => d,
            Err(e) => {
                for d in e.diagnostics {
                    let mut diag = Diagnostic::error(
                        "korek::syntax",
                        d.message.unwrap_or_else(|| "invalid KDL".into()),
                    )
                    .at(
                        span_of(id, d.span),
                        d.label.unwrap_or_else(|| "here".into()),
                    );
                    diag.help = d.help;
                    self.diags.push(diag);
                }
                return;
            }
        };
        for n in doc.nodes() {
            self.top(Node { inner: n, file: id });
        }
    }

    fn top(&mut self, node: Node<'_>) {
        let r = match node.name() {
            "package" => self.package(node),
            "enum" => {
                self.enum_def(node);
                Ok(())
            }
            "struct" => {
                self.struct_def(node);
                Ok(())
            }
            "union" => {
                self.union_def(node);
                Ok(())
            }
            "keys" => self.keys(node),
            "service" => self.service(node),
            other => {
                if let Some(x) = self.exts.iter().find(|x| x.claims_top_level(other)) {
                    x.top_level(node).map(|v| {
                        self.schema
                            .ext
                            .entry(x.name().to_owned())
                            .or_default()
                            .push(v);
                    })
                } else {
                    Err(err(
                        "korek::unknown_node",
                        format!("unknown top-level node `{other}`"),
                        node.name_span(),
                        "not a korek node",
                    )
                    .help(
                        "expected package, enum, struct, union, keys or service \
                         (or a node an extension registers)",
                    ))
                }
            }
        };
        if let Err(d) = r {
            self.diags.push(d);
        }
    }

    fn package(&mut self, node: Node<'_>) -> Result<(), Diagnostic> {
        let (name, span) = self.name_arg(node, 1)?;
        self.no_extras(node, ItemKind::Service, &[], &mut Ext::new());
        if name.is_empty()
            || !name
                .split('.')
                .all(|s| !s.is_empty() && is_snake(s) && !s.starts_with('_'))
        {
            return Err(err(
                "korek::bad_name",
                format!("package `{name}` must be dotted lower_snake_case segments"),
                span,
                "here",
            ));
        }
        if let Some(prev) = self.aux.package_span {
            return Err(err(
                "korek::duplicate",
                "the package is declared twice",
                node.span(),
                "second declaration",
            )
            .help(format!("first declared at byte {}", prev.offset)));
        }
        self.aux.package_span = Some(span);
        self.schema.package = name;
        Ok(())
    }

    /// The `i`th positional arg (counting from 0 after none) must be a
    /// string; `want` is the number of positional args allowed.
    fn name_arg(&mut self, node: Node<'_>, want: usize) -> Result<(String, Span), Diagnostic> {
        let args = node.args();
        let Some(first) = args.first() else {
            return Err(err(
                "korek::missing_name",
                format!("`{}` needs a name", node.name()),
                node.name_span(),
                "no name",
            ));
        };
        for extra in args.iter().skip(want) {
            self.diags.push(err(
                "korek::extra_argument",
                format!("unexpected argument on `{}`", node.name()),
                extra.span,
                "not expected",
            ));
        }
        match &first.value {
            Literal::String(s) => Ok((s.clone(), first.span)),
            other => Err(err(
                "korek::bad_name",
                format!("expected a name, found {}", other.describe()),
                first.span,
                "not a string",
            )),
        }
    }

    /// The second positional arg: a type expression.
    fn type_arg(&mut self, node: Node<'_>, item: Span) -> Option<Type> {
        let Some(arg) = node.args().into_iter().nth(1) else {
            self.diags.push(err(
                "korek::missing_type",
                format!("`{}` needs a type", node.name()),
                node.span(),
                "add a type, e.g. \"string\"",
            ));
            return None;
        };
        let Literal::String(s) = &arg.value else {
            self.diags.push(err(
                "korek::bad_type",
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
                    .push(err("korek::bad_type", msg, arg.span, "in this type"));
                None
            }
        }
    }

    /// Dispatches a property or child that the core grammar does not know.
    fn foreign_prop(&mut self, kind: ItemKind, name: &str, arg: &Arg, ext: &mut Ext) {
        let r = match self.exts.iter().find(|x| x.claims_property(kind, name)) {
            Some(x) => x
                .property(kind, name, arg)
                .map(|v| ext.entry(x.name().to_owned()).or_default().push(v)),
            None => Err(err(
                "korek::unknown_property",
                format!("unknown property `{name}`"),
                arg.span,
                "not recognized here",
            )),
        };
        if let Err(d) = r {
            self.diags.push(d);
        }
    }

    fn foreign_child(&mut self, kind: ItemKind, node: Node<'_>, ext: &mut Ext) {
        let r = match self.exts.iter().find(|x| x.claims_child(kind, node.name())) {
            Some(x) => x
                .child(kind, node)
                .map(|v| ext.entry(x.name().to_owned()).or_default().push(v)),
            None => Err(err(
                "korek::unknown_node",
                format!("unknown node `{}` here", node.name()),
                node.name_span(),
                "not recognized here",
            )),
        };
        if let Err(d) = r {
            self.diags.push(d);
        }
    }

    /// Rejects (or hands to extensions) every property not in `known`.
    fn no_extras(&mut self, node: Node<'_>, kind: ItemKind, known: &[&str], ext: &mut Ext) {
        for (name, arg) in node.props() {
            if !known.contains(&name) {
                self.foreign_prop(kind, name, &arg, ext);
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
                "korek::bad_id",
                "`id` must be an integer from 1 to 536870911",
                arg.span,
                "bad id",
            )),
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
                    "korek::bad_property",
                    format!("`{name}` must be #true or #false"),
                    a.span,
                    "not a boolean",
                ));
                false
            }
        }
    }

    fn str_prop(&mut self, node: Node<'_>, name: &str) -> Option<(String, Span)> {
        let a = node.prop(name)?;
        if let Literal::String(s) = a.value {
            Some((s, a.span))
        } else {
            self.diags.push(err(
                "korek::bad_property",
                format!("`{name}` must be a string"),
                a.span,
                "not a string",
            ));
            None
        }
    }

    /// Reads a `doc "…"` child (several are joined by newlines).
    fn doc(&mut self, node: Node<'_>, doc: &mut Option<String>) {
        let mut text = Vec::new();
        for a in node.args() {
            match a.value {
                Literal::String(s) => text.push(s),
                other => self.diags.push(err(
                    "korek::bad_doc",
                    format!("`doc` takes strings, found {}", other.describe()),
                    a.span,
                    "not a string",
                )),
            }
        }
        let text = text.join("\n");
        match doc {
            Some(d) => {
                d.push('\n');
                d.push_str(&text);
            }
            None => *doc = Some(text),
        }
    }

    /// Reads a constraint from a child node or property value.
    fn constraint(&mut self, name: &str, arg: Option<Arg>, at: Span, c: &mut Constraints) {
        let Some(arg) = arg else {
            self.diags.push(err(
                "korek::bad_constraint",
                format!("`{name}` needs a value"),
                at,
                "no value",
            ));
            return;
        };
        let bad = |what: &str| {
            err(
                "korek::bad_constraint",
                format!("`{name}` must be {what}, found {}", arg.value.describe()),
                arg.span,
                "bad value",
            )
        };
        match (name, &arg.value) {
            #[allow(clippy::cast_precision_loss)]
            ("min" | "max", Literal::Int(i)) => {
                let v = Some(*i as f64);
                if name == "min" {
                    c.min = v;
                } else {
                    c.max = v;
                }
            }
            ("min" | "max", Literal::Float(f)) => {
                if name == "min" {
                    c.min = Some(*f);
                } else {
                    c.max = Some(*f);
                }
            }
            ("min_len" | "max_len", Literal::Int(i)) if *i >= 0 => {
                let v = u64::try_from(*i).ok();
                if name == "min_len" {
                    c.min_len = v;
                } else {
                    c.max_len = v;
                }
            }
            ("pattern", Literal::String(s)) => c.pattern = Some(s.clone()),
            ("min" | "max", _) => self.diags.push(bad("a number")),
            ("min_len" | "max_len", _) => self.diags.push(bad("a non-negative integer")),
            _ => self.diags.push(bad("a string")),
        }
    }

    fn is_constraint(name: &str) -> bool {
        matches!(name, "min" | "max" | "min_len" | "max_len" | "pattern")
    }

    /// Walks the children of an item: `doc`, constraints when `c` is given,
    /// everything else to extensions.
    fn body(
        &mut self,
        node: Node<'_>,
        kind: ItemKind,
        doc: &mut Option<String>,
        mut c: Option<&mut Constraints>,
        ext: &mut Ext,
    ) {
        for ch in node.children() {
            match (ch.name(), c.as_deref_mut()) {
                ("doc", _) => self.doc(ch, doc),
                (n, Some(c)) if Self::is_constraint(n) => {
                    let arg = ch.args().into_iter().next();
                    self.constraint(n, arg, ch.span(), c);
                }
                _ => self.foreign_child(kind, ch, ext),
            }
        }
    }

    /// Reads properties and children shared by every item: `doc=`.
    fn doc_prop(&mut self, node: Node<'_>, doc: &mut Option<String>) {
        if let Some((d, _)) = self.str_prop(node, "doc") {
            *doc = Some(d);
        }
    }

    fn type_def(&mut self, node: Node<'_>, kind: TypeKind) -> Option<TypeDef> {
        let (name, name_span) = match self.name_arg(node, 1) {
            Ok(n) => n,
            Err(d) => {
                self.diags.push(d);
                return None;
            }
        };
        let span = node.span();
        self.aux.name_span.insert(span, name_span);
        Some(TypeDef {
            name,
            doc: None,
            kind,
            span,
            ext: Ext::new(),
        })
    }

    fn enum_def(&mut self, node: Node<'_>) {
        let kind = TypeKind::Enum(EnumDef {
            variants: Vec::new(),
            retired: Vec::new(),
        });
        let Some(mut t) = self.type_def(node, kind) else {
            return;
        };
        let mut variants = Vec::new();
        self.no_extras(node, ItemKind::Enum, &["doc"], &mut t.ext);
        self.doc_prop(node, &mut t.doc);
        for ch in node.children() {
            match ch.name() {
                "doc" => self.doc(ch, &mut t.doc),
                "variant" => {
                    if let Some(v) = self.enum_variant(ch) {
                        variants.push(v);
                    }
                }
                _ => self.foreign_child(ItemKind::Enum, ch, &mut t.ext),
            }
        }
        t.kind = TypeKind::Enum(EnumDef {
            variants,
            retired: Vec::new(),
        });
        self.schema.types.push(t);
    }

    fn enum_variant(&mut self, node: Node<'_>) -> Option<EnumVariant> {
        let (name, name_span) = self
            .name_arg(node, 1)
            .map_err(|d| self.diags.push(d))
            .ok()?;
        let span = node.span();
        self.aux.name_span.insert(span, name_span);
        let mut v = EnumVariant {
            name,
            doc: None,
            number: 0,
            span,
            ext: Ext::new(),
        };
        self.no_extras(node, ItemKind::EnumVariant, &["doc", "id"], &mut v.ext);
        self.doc_prop(node, &mut v.doc);
        self.id_prop(node, span);
        self.body(node, ItemKind::EnumVariant, &mut v.doc, None, &mut v.ext);
        Some(v)
    }

    fn struct_def(&mut self, node: Node<'_>) {
        let kind = TypeKind::Struct(StructDef {
            fields: Vec::new(),
            retired: Vec::new(),
        });
        let Some(mut t) = self.type_def(node, kind) else {
            return;
        };
        let mut fields = Vec::new();
        self.no_extras(node, ItemKind::Struct, &["doc"], &mut t.ext);
        self.doc_prop(node, &mut t.doc);
        for ch in node.children() {
            match ch.name() {
                "doc" => self.doc(ch, &mut t.doc),
                "field" => {
                    if let Some(f) = self.field(ch) {
                        fields.push(f);
                    }
                }
                _ => self.foreign_child(ItemKind::Struct, ch, &mut t.ext),
            }
        }
        t.kind = TypeKind::Struct(StructDef {
            fields,
            retired: Vec::new(),
        });
        self.schema.types.push(t);
    }

    /// Constraints given as properties (`min=1`).
    fn constraint_props(&mut self, node: Node<'_>, c: &mut Constraints) {
        for (name, arg) in node.props() {
            if Self::is_constraint(name) {
                let at = arg.span;
                self.constraint(name, Some(arg), at, c);
            }
        }
    }

    fn field(&mut self, node: Node<'_>) -> Option<Field> {
        let (name, name_span) = self
            .name_arg(node, 2)
            .map_err(|d| self.diags.push(d))
            .ok()?;
        let span = node.span();
        self.aux.name_span.insert(span, name_span);
        let ty = self.type_arg(node, span);
        let mut f = Field {
            name,
            ty: ty.unwrap_or(Type::String),
            optional: self.bool_prop(node, "optional"),
            doc: None,
            constraints: Constraints::default(),
            number: 0,
            span,
            ext: Ext::new(),
        };
        self.no_extras(node, ItemKind::Field, FIELD_PROPS, &mut f.ext);
        self.doc_prop(node, &mut f.doc);
        self.id_prop(node, span);
        self.constraint_props(node, &mut f.constraints);
        self.body(
            node,
            ItemKind::Field,
            &mut f.doc,
            Some(&mut f.constraints),
            &mut f.ext,
        );
        Some(f)
    }

    fn union_def(&mut self, node: Node<'_>) {
        let kind = TypeKind::Union(UnionDef {
            tag: None,
            variants: Vec::new(),
            retired: Vec::new(),
        });
        let Some(mut t) = self.type_def(node, kind) else {
            return;
        };
        let tag = self.str_prop(node, "tag");
        if let Some((tag, span)) = &tag {
            self.aux.name_span.insert(t.span, *span);
            if !is_snake(tag) {
                self.diags.push(err(
                    "korek::bad_name",
                    format!("tag `{tag}` must be lower_snake_case"),
                    *span,
                    "here",
                ));
            }
        }
        let mut variants = Vec::new();
        self.no_extras(node, ItemKind::Union, &["doc", "tag"], &mut t.ext);
        self.doc_prop(node, &mut t.doc);
        for ch in node.children() {
            match ch.name() {
                "doc" => self.doc(ch, &mut t.doc),
                "variant" => {
                    if let Some(v) = self.union_variant(ch) {
                        variants.push(v);
                    }
                }
                _ => self.foreign_child(ItemKind::Union, ch, &mut t.ext),
            }
        }
        t.kind = TypeKind::Union(UnionDef {
            tag: tag.map(|t| t.0),
            variants,
            retired: Vec::new(),
        });
        self.schema.types.push(t);
    }

    fn union_variant(&mut self, node: Node<'_>) -> Option<UnionVariant> {
        let (name, name_span) = self
            .name_arg(node, 2)
            .map_err(|d| self.diags.push(d))
            .ok()?;
        let span = node.span();
        self.aux.name_span.insert(span, name_span);
        let ty = self.type_arg(node, span)?;
        let mut v = UnionVariant {
            name,
            ty,
            doc: None,
            number: 0,
            span,
            ext: Ext::new(),
        };
        self.no_extras(node, ItemKind::UnionVariant, &["doc", "id"], &mut v.ext);
        self.doc_prop(node, &mut v.doc);
        self.id_prop(node, span);
        self.body(node, ItemKind::UnionVariant, &mut v.doc, None, &mut v.ext);
        Some(v)
    }

    fn keys(&mut self, node: Node<'_>) -> Result<(), Diagnostic> {
        let (name, name_span) = self.name_arg(node, 1)?;
        let span = node.span();
        self.aux.name_span.insert(span, name_span);
        let mut g = KeyGroup {
            name,
            doc: None,
            keys: Vec::new(),
            retired: Vec::new(),
            span,
            ext: Ext::new(),
        };
        self.no_extras(node, ItemKind::KeyGroup, &["doc"], &mut g.ext);
        self.doc_prop(node, &mut g.doc);
        for ch in node.children() {
            match ch.name() {
                "doc" => self.doc(ch, &mut g.doc),
                "key" => {
                    if let Some(k) = self.key(ch) {
                        g.keys.push(k);
                    }
                }
                _ => self.foreign_child(ItemKind::KeyGroup, ch, &mut g.ext),
            }
        }
        self.schema.keys.push(g);
        Ok(())
    }

    fn key(&mut self, node: Node<'_>) -> Option<Key> {
        let (name, name_span) = self
            .name_arg(node, 2)
            .map_err(|d| self.diags.push(d))
            .ok()?;
        let span = node.span();
        self.aux.name_span.insert(span, name_span);
        let ty = self.type_arg(node, span)?;
        let mut k = Key {
            name,
            ty,
            default: None,
            doc: None,
            constraints: Constraints::default(),
            number: 0,
            span,
            ext: Ext::new(),
        };
        self.no_extras(node, ItemKind::Key, KEY_PROPS, &mut k.ext);
        if let Some(d) = node.prop("default") {
            self.aux.default.insert(span, d);
        }
        self.doc_prop(node, &mut k.doc);
        self.id_prop(node, span);
        self.constraint_props(node, &mut k.constraints);
        self.body(
            node,
            ItemKind::Key,
            &mut k.doc,
            Some(&mut k.constraints),
            &mut k.ext,
        );
        Some(k)
    }

    fn service(&mut self, node: Node<'_>) -> Result<(), Diagnostic> {
        let (name, name_span) = self.name_arg(node, 1)?;
        let span = node.span();
        self.aux.name_span.insert(span, name_span);
        let mut s = Service {
            name,
            doc: None,
            rpcs: Vec::new(),
            span,
            ext: Ext::new(),
        };
        self.no_extras(node, ItemKind::Service, &["doc"], &mut s.ext);
        self.doc_prop(node, &mut s.doc);
        for ch in node.children() {
            match ch.name() {
                "doc" => self.doc(ch, &mut s.doc),
                "rpc" => {
                    if let Some(r) = self.rpc(ch) {
                        s.rpcs.push(r);
                    }
                }
                _ => self.foreign_child(ItemKind::Service, ch, &mut s.ext),
            }
        }
        self.schema.services.push(s);
        Ok(())
    }

    fn rpc(&mut self, node: Node<'_>) -> Option<Rpc> {
        let (name, name_span) = self
            .name_arg(node, 1)
            .map_err(|d| self.diags.push(d))
            .ok()?;
        let span = node.span();
        self.aux.name_span.insert(span, name_span);
        let mut io = |p: &str| {
            let v = self.str_prop(node, p);
            if v.is_none() && node.prop(p).is_none() {
                self.diags.push(err(
                    "korek::missing_property",
                    format!("`rpc` needs `{p}=\"<message type>\"`"),
                    node.span(),
                    "here",
                ));
            }
            v
        };
        let input = io("input");
        let output = io("output");
        let idempotency = match self.str_prop(node, "idempotency") {
            None => None,
            Some((s, _)) if s == "no_side_effects" => Some(Idempotency::NoSideEffects),
            Some((s, _)) if s == "idempotent" => Some(Idempotency::Idempotent),
            Some((s, sp)) => {
                self.diags.push(err(
                    "korek::bad_property",
                    format!("unknown idempotency `{s}`"),
                    sp,
                    "expected no_side_effects or idempotent",
                ));
                None
            }
        };
        let (input, in_span) = input?;
        let (output, out_span) = output?;
        // Remember where the type names are, for unknown-type errors.
        self.aux.ty_span.insert(span, in_span);
        self.aux.out_span.insert(span, out_span);
        let mut r = Rpc {
            name,
            input,
            output,
            idempotency,
            doc: None,
            span,
            ext: Ext::new(),
        };
        self.no_extras(node, ItemKind::Rpc, RPC_PROPS, &mut r.ext);
        self.doc_prop(node, &mut r.doc);
        self.body(node, ItemKind::Rpc, &mut r.doc, None, &mut r.ext);
        Some(r)
    }
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
