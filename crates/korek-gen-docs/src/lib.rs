//! Markdown reference docs: every type (fields with their types linked,
//! defaults, validation rules in words, units humanized, examples,
//! deprecations, secrets marked), enum and union variants, each keys block
//! as a table plus a section per key, services, and an index.
//!
//! Options: `file` (default `reference.md`), `split` (default `false`: one
//! file; `true`: `index.md`, `types.md`, `keys.<block>.md`, `services.md`),
//! `heading_level` (default `1`, the level of the title), `title` (default
//! `<package> reference`), and `columns`: extra table columns read from IR
//! metadata, `;`-separated `Header=ext:<extension>[.<path>]` or
//! `Header=meta:<namespace>.<prop>` (`Layers=ext:setting.layers`). In-process
//! callers can also add [`ColumnSource::With`] columns.

extern crate alloc;

mod md;
mod words;

use alloc::fmt::Write as _;

use korek_ir::codes;
use korek_ir::{
    Attrs, Diagnostic, EnumDef, Field, Idempotency, Json, Key, KeyGroup, Level, Schema, Service,
    StructDef, Type, TypeDef, TypeKind, UnionDef,
};
use korek_plugin::{File, GenerateRequest, GenerateResponse, Hook, Item};

use md::{code, header, heading, row};
pub use words::{rules, seconds, with_unit};

/// Generator options.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Options {
    /// The output file when not split.
    pub file: String,
    /// One file per section plus `index.md`.
    pub split: bool,
    /// The heading level of the title (1–4); sections and items nest below.
    pub heading_level: usize,
    /// The title, `<package> reference` when unset.
    pub title: Option<String>,
    /// Extra columns in field, variant and key tables (and lines in key
    /// sections), shown where an item has a value.
    pub columns: Vec<Column>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            file: "reference.md".into(),
            split: false,
            heading_level: 1,
            title: None,
            columns: Vec::new(),
        }
    }
}

/// An extra column: a header and where its cells come from.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Column {
    pub header: String,
    pub source: ColumnSource,
}

/// Where a [`Column`]'s cells come from: an item's attributes.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum ColumnSource {
    /// The values extension `name` attached to the item, or the member at
    /// `path` (object keys) of each.
    Ext { name: String, path: Vec<String> },
    /// A `meta "<namespace>" <prop>=…` property.
    Meta { namespace: String, prop: String },
    /// Computed in-process (markdown).
    With(fn(&Attrs) -> Option<String>),
}

impl Column {
    pub fn new(header: impl Into<String>, source: ColumnSource) -> Self {
        Self {
            header: header.into(),
            source,
        }
    }

    /// Parses `Header=ext:<name>[.<path>]` or `Header=meta:<ns>.<prop>`.
    ///
    /// # Errors
    /// When `spec` has neither form.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let bad = || {
            format!(
                "column `{spec}`: expected Header=ext:NAME[.PATH] or Header=meta:NAMESPACE.PROP"
            )
        };
        let (header, src) = spec.split_once('=').ok_or_else(bad)?;
        let source = if let Some(p) = src.strip_prefix("ext:") {
            let mut parts = p.split('.').map(str::to_owned);
            let name = parts.next().filter(|n| !n.is_empty()).ok_or_else(bad)?;
            ColumnSource::Ext {
                name,
                path: parts.collect(),
            }
        } else if let Some(p) = src.strip_prefix("meta:") {
            let (ns, prop) = p.rsplit_once('.').ok_or_else(bad)?;
            ColumnSource::Meta {
                namespace: ns.into(),
                prop: prop.into(),
            }
        } else {
            return Err(bad());
        };
        if header.trim().is_empty() {
            return Err(bad());
        }
        Ok(Self::new(header.trim(), source))
    }

    /// The cell for an item, `None` when it has no value.
    pub fn cell(&self, attrs: &Attrs) -> Option<String> {
        let text = match &self.source {
            ColumnSource::Ext { name, path } => {
                let vals: Vec<String> = attrs
                    .ext
                    .get(name)
                    .into_iter()
                    .flatten()
                    .filter_map(|v| path.iter().try_fold(v, |v, k| v.get(k)))
                    .map(plain)
                    .filter(|s| !s.is_empty())
                    .collect();
                vals.join(", ")
            }
            ColumnSource::Meta { namespace, prop } => attrs
                .meta
                .iter()
                .filter(|m| &m.namespace == namespace)
                .filter_map(|m| m.props.get(prop))
                .map(plain)
                .collect::<Vec<_>>()
                .join(", "),
            ColumnSource::With(f) => f(attrs).unwrap_or_default(),
        };
        (!text.is_empty()).then_some(text)
    }
}

/// A metadata value as text: strings bare, lists joined.
fn plain(v: &Json) -> String {
    match v {
        Json::Null => String::new(),
        Json::String(s) => s.clone(),
        Json::Array(a) => a.iter().map(plain).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

fn invalid(msg: String) -> Diagnostic {
    Diagnostic::error(codes::INVALID_OPTION, msg)
}

/// The plugin entry point (`korek-gen-docs`).
///
/// # Errors
/// On unknown or malformed options.
pub fn plugin(req: &GenerateRequest) -> Result<GenerateResponse, Diagnostic> {
    req.check_options(&["file", "split", "heading_level", "title", "columns"])?;
    let mut opts = Options {
        split: req.bool_option("split", false)?,
        title: req.options.get("title").cloned(),
        ..Options::default()
    };
    if let Some(f) = req.options.get("file") {
        opts.file.clone_from(f);
    }
    if let Some(l) = req.options.get("heading_level") {
        opts.heading_level = l
            .parse()
            .ok()
            .filter(|l| (1..=4).contains(l))
            .ok_or_else(|| {
                invalid(format!(
                    "option `heading_level` must be 1 to 4, found `{l}`"
                ))
            })?;
    }
    if let Some(c) = req.options.get("columns") {
        opts.columns = c
            .split(';')
            .filter(|s| !s.trim().is_empty())
            .map(Column::parse)
            .collect::<Result<_, _>>()
            .map_err(invalid)?;
    }
    Ok(GenerateResponse::new(generate(&req.schema, &opts, &[])))
}

/// Which file an item is in when split.
const TYPES: &str = "types.md";
const SERVICES: &str = "services.md";

fn keys_file(g: &KeyGroup) -> String {
    format!("keys.{}.md", g.name)
}

struct Gen<'a> {
    schema: &'a Schema,
    opts: &'a Options,
    hooks: &'a [&'a dyn Hook],
}

/// A one-line summary: the `doc`, else the first paragraph of the
/// description.
fn summary(a: &Attrs) -> Option<String> {
    a.doc.clone().or_else(|| {
        a.description
            .as_deref()
            .and_then(|d| d.trim().split("\n\n").next())
            .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
    })
}

/// The `doc` and `description` as paragraphs.
fn prose(a: &Attrs) -> Vec<String> {
    let mut v: Vec<String> = a.doc.iter().cloned().collect();
    if let Some(d) = &a.description {
        v.push(d.trim().to_owned());
    }
    v
}

/// Deprecation, `since`, aliases and secrecy, as sentences.
fn notes(a: &Attrs) -> Vec<String> {
    let mut v = Vec::new();
    match a.deprecated.as_deref() {
        Some("") => v.push("**Deprecated.**".to_owned()),
        Some(d) => v.push(format!("**Deprecated**: {d}.")),
        None => {}
    }
    if a.secret {
        v.push("**Secret**: never printed or shown with its value.".into());
    }
    if let Some(s) = &a.since {
        v.push(format!("Since {s}."));
    }
    if !a.aliases.is_empty() {
        let names: Vec<String> = a.aliases.iter().map(|n| code(n)).collect();
        v.push(format!("Also known as {}.", names.join(", ")));
    }
    v
}

fn examples_inline(a: &Attrs) -> Option<String> {
    (!a.examples.is_empty() && !a.secret).then(|| {
        let ex: Vec<String> = a.examples.iter().map(|e| code(&e.to_string())).collect();
        format!(
            "{}: {}.",
            if ex.len() == 1 { "Example" } else { "Examples" },
            ex.join(", ")
        )
    })
}

fn unit_note(a: &Attrs) -> Option<String> {
    a.unit.as_ref().map(|u| format!("Unit: {u}."))
}

fn sentence(parts: &[String]) -> Option<String> {
    (!parts.is_empty()).then(|| {
        let mut s = parts.join("; ");
        if let Some(f) = s.get(..1) {
            s.replace_range(..1, &f.to_uppercase());
        }
        s.push('.');
        s
    })
}

fn optional<T>(v: Option<T>) -> Vec<T> {
    v.into_iter().collect()
}

impl Gen<'_> {
    fn level(&self, depth: usize) -> usize {
        self.opts.heading_level + depth
    }

    /// How deep items sit below the title.
    fn item_depth(&self) -> usize {
        if self.opts.split { 1 } else { 2 }
    }

    fn link(&self, file: &str, anchor: &str) -> String {
        if self.opts.split {
            format!("{file}#{anchor}")
        } else {
            format!("#{anchor}")
        }
    }

    fn type_link(&self, name: &str) -> String {
        self.link(TYPES, &format!("type.{name}"))
    }

    /// A type as markdown, named types linked.
    fn ty(&self, ty: &Type) -> String {
        fn has_named(t: &Type) -> bool {
            match t {
                Type::Named(_) => true,
                Type::List(t) | Type::Map(t) => has_named(t),
                _ => false,
            }
        }
        if !has_named(ty) {
            return code(&ty.display());
        }
        match ty {
            Type::List(t) => format!("{}{}{}", code("list<"), self.ty(t), code(">")),
            Type::Map(t) => format!("{}{}{}", code("map<string,"), self.ty(t), code(">")),
            Type::Named(n) if self.schema.get(n).is_some() => {
                format!("[{}]({})", code(n), self.type_link(n))
            }
            other => code(&other.display()),
        }
    }

    /// The columns with a value for any of `items`, and each item's cells.
    fn extra(&self, items: &[&Attrs]) -> (Vec<String>, Vec<Vec<String>>) {
        let cols: Vec<&Column> = self
            .opts
            .columns
            .iter()
            .filter(|c| items.iter().any(|a| c.cell(a).is_some()))
            .collect();
        let headers = cols.iter().map(|c| c.header.clone()).collect();
        let cells = items
            .iter()
            .map(|a| cols.iter().map(|c| c.cell(a).unwrap_or_default()).collect())
            .collect();
        (headers, cells)
    }

    fn paragraphs(out: &mut String, parts: &[String]) {
        for p in parts {
            out.push_str(p);
            out.push_str("\n\n");
        }
    }

    fn type_section(&self, out: &mut String, t: &TypeDef) {
        let kind = match &t.kind {
            TypeKind::Enum(_) => "enum",
            TypeKind::Struct(_) => "struct",
            TypeKind::Union(_) => "union",
        };
        heading(
            out,
            self.level(self.item_depth()),
            Some(&format!("type.{}", t.name)),
            &format!("{} ({kind})", t.name),
        );
        Self::paragraphs(out, &prose(&t.attrs));
        let mut meta = notes(&t.attrs);
        if let Some(r) = sentence(&rules(&t.validate, &Type::Named(t.name.clone()), None)) {
            meta.push(format!("Rules: {r}"));
        }
        if !meta.is_empty() {
            out.push_str(&meta.join(" "));
            out.push_str("\n\n");
        }
        match &t.kind {
            TypeKind::Enum(e) => self.enum_table(out, e),
            TypeKind::Struct(s) => self.struct_table(out, s),
            TypeKind::Union(u) => self.union_table(out, u),
        }
        if !t.attrs.secret {
            for e in &t.attrs.examples {
                out.push_str("Example:\n\n");
                md::block(
                    out,
                    "json",
                    &serde_json::to_string_pretty(e).unwrap_or_default(),
                );
            }
        }
        for h in self.hooks {
            h.after(Item::Type(t), out);
        }
    }

    /// The description cell of a field, variant or key: prose, unit, rules,
    /// examples and notes.
    fn describe(attrs: &Attrs, rules: Option<String>) -> String {
        let mut parts = prose(attrs);
        parts.extend(unit_note(attrs));
        parts.extend(rules.map(|r| format!("Rules: {r}")));
        parts.extend(examples_inline(attrs));
        parts.extend(notes(attrs));
        parts.join("\n\n")
    }

    fn table(
        out: &mut String,
        mut head: Vec<String>,
        extra: (Vec<String>, Vec<Vec<String>>),
        rows: Vec<Vec<String>>,
    ) {
        head.extend(extra.0);
        header(out, &head);
        for (mut r, x) in rows.into_iter().zip(extra.1) {
            r.extend(x);
            row(out, &r);
        }
        out.push('\n');
    }

    fn enum_table(&self, out: &mut String, e: &EnumDef) {
        let attrs: Vec<&Attrs> = e.variants.iter().map(|v| &v.attrs).collect();
        let rows = e
            .variants
            .iter()
            .map(|v| vec![code(&v.name), Self::describe(&v.attrs, None)])
            .collect();
        Self::table(
            out,
            vec!["Variant".into(), "Description".into()],
            self.extra(&attrs),
            rows,
        );
    }

    fn default_cell(default: Option<&Json>, ty: &Type, attrs: &Attrs) -> String {
        match default {
            Some(_) if attrs.secret => "*hidden*".into(),
            Some(d) => words::value(d, ty, attrs.unit.as_deref()),
            None => String::new(),
        }
    }

    fn field_rules(f: &Field) -> Option<String> {
        sentence(&rules(&f.validate, &f.ty, f.attrs.unit.as_deref()))
    }

    fn struct_table(&self, out: &mut String, s: &StructDef) {
        if s.fields.is_empty() {
            out.push_str("No fields.\n\n");
            return;
        }
        let attrs: Vec<&Attrs> = s.fields.iter().map(|f| &f.attrs).collect();
        let rows = s
            .fields
            .iter()
            .map(|f| {
                let mut ty = self.ty(&f.ty);
                if f.optional {
                    ty.push_str(", optional");
                }
                vec![
                    code(&f.name),
                    ty,
                    Self::default_cell(f.default.as_ref(), &f.ty, &f.attrs),
                    Self::describe(&f.attrs, Self::field_rules(f)),
                ]
            })
            .collect();
        let head = ["Field", "Type", "Default", "Description"]
            .map(String::from)
            .to_vec();
        Self::table(out, head, self.extra(&attrs), rows);
    }

    fn union_table(&self, out: &mut String, u: &UnionDef) {
        match &u.tag {
            Some(tag) => {
                let _ = writeln!(
                    out,
                    "Tagged: the {} field names the variant; the variant's own fields sit beside it.\n",
                    code(tag)
                );
            }
            None => out.push_str("Untagged: a value is the first variant it matches.\n\n"),
        }
        let attrs: Vec<&Attrs> = u.variants.iter().map(|v| &v.attrs).collect();
        let rows = u
            .variants
            .iter()
            .map(|v| {
                vec![
                    code(&v.name),
                    self.ty(&v.ty),
                    Self::describe(&v.attrs, None),
                ]
            })
            .collect();
        let head = ["Variant", "Type", "Description"]
            .map(String::from)
            .to_vec();
        Self::table(out, head, self.extra(&attrs), rows);
    }

    fn key_anchor(g: &KeyGroup, k: &Key) -> String {
        format!("key.{}.{}", g.name, k.name)
    }

    /// A keys block: under a heading at `depth`, or (`None`, split) as the
    /// whole file.
    fn keys_section(&self, out: &mut String, g: &KeyGroup, depth: Option<usize>) {
        if let Some(d) = depth {
            heading(
                out,
                self.level(d),
                Some(&format!("keys.{}", g.name)),
                &format!("{} keys", code(&g.name)),
            );
        }
        Self::paragraphs(out, &prose(&g.attrs));
        let n = notes(&g.attrs);
        if !n.is_empty() {
            Self::paragraphs(out, &[n.join(" ")]);
        }
        let attrs: Vec<&Attrs> = g.keys.iter().map(|k| &k.attrs).collect();
        let file = keys_file(g);
        let rows = g
            .keys
            .iter()
            .map(|k| {
                let mut desc: Vec<String> = optional(summary(&k.attrs));
                if k.attrs.deprecated.is_some() {
                    desc.push("**Deprecated.**".into());
                }
                if k.attrs.secret {
                    desc.push("**Secret.**".into());
                }
                vec![
                    format!(
                        "[{}]({})",
                        code(&format!("{}.{}", g.name, k.name)),
                        self.link(&file, &Self::key_anchor(g, k))
                    ),
                    self.ty(&k.ty),
                    Self::default_cell(k.default.as_ref(), &k.ty, &k.attrs),
                    desc.join(" "),
                ]
            })
            .collect();
        let head = ["Key", "Type", "Default", "Description"]
            .map(String::from)
            .to_vec();
        Self::table(out, head, self.extra(&attrs), rows);
        for h in self.hooks {
            h.after(Item::KeyGroup(g), out);
        }
        for k in &g.keys {
            self.key_section(out, g, k, depth.map_or(1, |d| d + 1));
        }
    }

    fn key_section(&self, out: &mut String, g: &KeyGroup, k: &Key, depth: usize) {
        heading(
            out,
            self.level(depth),
            Some(&Self::key_anchor(g, k)),
            &code(&format!("{}.{}", g.name, k.name)),
        );
        Self::paragraphs(out, &prose(&k.attrs));
        let a = &k.attrs;
        let mut items = vec![format!("Type: {}", self.ty(&k.ty))];
        if k.default.is_some() {
            items.push(format!(
                "Default: {}",
                Self::default_cell(k.default.as_ref(), &k.ty, a)
            ));
        }
        if let Some(ui) = &a.ui {
            items.extend(ui.label.as_ref().map(|l| format!("Label: {l}")));
            items.extend(ui.level.map(|l| {
                match l {
                    Level::Basic => "Level: basic",
                    Level::Advanced => "Level: advanced",
                }
                .to_owned()
            }));
        }
        items.extend(a.unit.as_ref().map(|u| format!("Unit: {u}")));
        items.extend(
            sentence(&rules(&k.validate, &k.ty, a.unit.as_deref())).map(|r| format!("Rules: {r}")),
        );
        items.extend(notes(a));
        items.extend(
            self.opts
                .columns
                .iter()
                .filter_map(|c| c.cell(a).map(|v| format!("{}: {v}", c.header))),
        );
        for i in items {
            let _ = writeln!(out, "- {i}");
        }
        out.push('\n');
        if !a.secret {
            for e in &a.examples {
                out.push_str("Example:\n\n");
                md::block(
                    out,
                    "json",
                    &serde_json::to_string_pretty(e).unwrap_or_default(),
                );
            }
        }
        for h in self.hooks {
            h.after(Item::Key(g, k), out);
        }
    }

    fn service_section(&self, out: &mut String, s: &Service) {
        heading(
            out,
            self.level(self.item_depth()),
            Some(&format!("service.{}", s.name)),
            &s.name,
        );
        Self::paragraphs(out, &prose(&s.attrs));
        let rows = s
            .rpcs
            .iter()
            .map(|r| {
                let mut desc = prose(&r.attrs);
                desc.extend(r.idempotency.map(|i| {
                    match i {
                        Idempotency::NoSideEffects => "No side effects.",
                        Idempotency::Idempotent => "Idempotent.",
                    }
                    .to_owned()
                }));
                desc.extend(notes(&r.attrs));
                vec![
                    code(&r.name),
                    self.ty(&Type::Named(r.input.clone())),
                    self.ty(&Type::Named(r.output.clone())),
                    desc.join("\n\n"),
                ]
            })
            .collect::<Vec<_>>();
        let n = rows.len();
        let head = ["RPC", "Input", "Output", "Description"]
            .map(String::from)
            .to_vec();
        Self::table(out, head, (Vec::new(), vec![Vec::new(); n]), rows);
        for h in self.hooks {
            h.after(Item::Service(s), out);
        }
    }

    fn title(&self) -> String {
        self.opts
            .title
            .clone()
            .unwrap_or_else(|| format!("{} reference", self.schema.package))
    }

    fn start(&self, out: &mut String, anchor: Option<&str>, title: &str) {
        let _ = writeln!(out, "<!-- {} -->\n", korek_gen_common::GENERATED);
        heading(out, self.level(0), anchor, title);
    }

    /// The contents: every type, keys block and service, linked.
    fn index(&self, out: &mut String) {
        let schema = self.schema;
        let entry = |out: &mut String, link: String, what: Option<String>| {
            let _ = match what {
                Some(w) => writeln!(out, "  - {link}: {w}"),
                None => writeln!(out, "  - {link}"),
            };
        };
        if !schema.types.is_empty() {
            let _ = writeln!(out, "- [Types]({})", self.section_link(TYPES, "types"));
            for t in &schema.types {
                entry(
                    out,
                    format!("[{}]({})", code(&t.name), self.type_link(&t.name)),
                    summary(&t.attrs),
                );
            }
        }
        if !schema.keys.is_empty() {
            let _ = writeln!(out, "- [Keys]({})", self.section_link("", "keys"));
            for g in &schema.keys {
                let link = self.link(&keys_file(g), &format!("keys.{}", g.name));
                entry(
                    out,
                    format!("[{}]({link})", code(&g.name)),
                    summary(&g.attrs),
                );
            }
        }
        if !schema.services.is_empty() {
            let _ = writeln!(
                out,
                "- [Services]({})",
                self.section_link(SERVICES, "services")
            );
            for v in &schema.services {
                let link = self.link(SERVICES, &format!("service.{}", v.name));
                entry(
                    out,
                    format!("[{}]({link})", code(&v.name)),
                    summary(&v.attrs),
                );
            }
        }
        out.push('\n');
    }

    /// A section: its file when split (or the first keys file), its anchor
    /// otherwise.
    fn section_link(&self, file: &str, anchor: &str) -> String {
        if !self.opts.split {
            return format!("#{anchor}");
        }
        if file.is_empty() {
            return self.schema.keys.first().map(keys_file).unwrap_or_default();
        }
        file.to_owned()
    }

    fn section(&self, out: &mut String, anchor: &str, title: &str) {
        heading(out, self.level(1), Some(anchor), title);
    }

    fn single(&self) -> File {
        let mut out = String::new();
        self.start(&mut out, None, &self.title());
        self.index(&mut out);
        let s = self.schema;
        if !s.types.is_empty() {
            self.section(&mut out, "types", "Types");
            for t in &s.types {
                self.type_section(&mut out, t);
            }
        }
        if !s.keys.is_empty() {
            self.section(&mut out, "keys", "Keys");
            for g in &s.keys {
                self.keys_section(&mut out, g, Some(self.item_depth()));
            }
        }
        if !s.services.is_empty() {
            self.section(&mut out, "services", "Services");
            for v in &s.services {
                self.service_section(&mut out, v);
            }
        }
        for h in self.hooks {
            h.footer(s, &mut out);
        }
        File::new(&self.opts.file, finish(out))
    }

    fn split(&self) -> Vec<File> {
        let s = self.schema;
        let mut files = Vec::new();
        let mut index = String::new();
        self.start(&mut index, None, &self.title());
        self.index(&mut index);
        for h in self.hooks {
            h.footer(s, &mut index);
        }
        files.push(File::new("index.md", finish(index)));
        if !s.types.is_empty() {
            let mut out = String::new();
            self.start(&mut out, Some("types"), "Types");
            for t in &s.types {
                self.type_section(&mut out, t);
            }
            files.push(File::new(TYPES, finish(out)));
        }
        for g in &s.keys {
            let mut out = String::new();
            self.start(
                &mut out,
                Some(&format!("keys.{}", g.name)),
                &format!("{} keys", code(&g.name)),
            );
            self.keys_section(&mut out, g, None);
            files.push(File::new(keys_file(g), finish(out)));
        }
        if !s.services.is_empty() {
            let mut out = String::new();
            self.start(&mut out, Some("services"), "Services");
            for v in &s.services {
                self.service_section(&mut out, v);
            }
            files.push(File::new(SERVICES, finish(out)));
        }
        files
    }
}

/// One trailing newline.
fn finish(mut s: String) -> String {
    let n = s.trim_end().len();
    s.truncate(n);
    s.push('\n');
    s
}

/// Generates the markdown file(s).
pub fn generate(schema: &Schema, opts: &Options, hooks: &[&dyn Hook]) -> Vec<File> {
    let g = Gen {
        schema,
        opts,
        hooks,
    };
    let mut files = if opts.split {
        g.split()
    } else {
        vec![g.single()]
    };
    for h in hooks {
        files.extend(h.files(schema));
    }
    files
}
