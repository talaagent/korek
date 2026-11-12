//! The `korek` CLI.

extern crate alloc;

use alloc::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use bpaf::{Parser, construct, long, positional};
use korek::{Driver, Error, Output};
use korek_core::{Loader, Passthrough};

#[derive(Debug, Clone)]
struct Common {
    exts: Vec<String>,
    lock: Option<PathBuf>,
    dir: PathBuf,
}

#[derive(Debug, Clone)]
struct GenArgs {
    builtin: Vec<(&'static str, Option<PathBuf>, Option<String>)>,
    outs: Vec<String>,
    plugins: Vec<String>,
    plugin_opts: Vec<String>,
    check: bool,
    common: Common,
}

#[derive(Debug, Clone)]
enum Cmd {
    Gen(GenArgs),
    Check(Common),
    Ir(Common),
}

fn common_args() -> impl Parser<Common> {
    let exts = long("ext")
        .help("Capture nodes and properties called NAME as extension metadata")
        .argument::<String>("NAME")
        .many();
    let lock = long("lock")
        .help("The lockfile (default: <SCHEMA_DIR>/korek.lock)")
        .argument::<PathBuf>("FILE")
        .optional();
    let dir = positional::<PathBuf>("SCHEMA_DIR").help("The directory of .kdl schema files");
    construct!(Common { exts, lock, dir })
}

fn builtin(
    name: &'static str,
    what: &'static str,
) -> impl Parser<(&'static str, Option<PathBuf>, Option<String>)> {
    let dir = long(name).help(what).argument::<PathBuf>("DIR").optional();
    let opt_name: &'static str = Box::leak(format!("{name}_opt").into_boxed_str());
    let opt = long(opt_name)
        .help("Options for this generator, k=v[,k=v]")
        .argument::<String>("OPTS")
        .optional();
    construct!(dir, opt).map(move |(d, o)| (name, d, o))
}

fn parser() -> impl Parser<Cmd> {
    let rust = builtin("rust", "Write Rust (serde types, key consts) to DIR");
    let proto = builtin("proto", "Write Protobuf (protovalidate, services) to DIR");
    let zod = builtin("zod", "Write TypeScript Zod schemas to DIR");
    let ts = builtin("ts", "Write bare TypeScript types to DIR");
    let builtin = construct!(rust, proto, zod, ts).map(|(a, b, c, d)| vec![a, b, c, d]);
    let outs = long("out")
        .help("Run plugin korek-gen-NAME into DIR")
        .argument::<String>("NAME=DIR")
        .many();
    let plugins = long("plugin")
        .help("Use PATH as the binary for plugin NAME")
        .argument::<String>("NAME=PATH")
        .many();
    let plugin_opts = long("opt")
        .help("Options for plugin NAME")
        .argument::<String>("NAME:k=v[,k=v]")
        .many();
    let check = long("check")
        .help("Fail if any generated file is out of date; write nothing")
        .switch();
    let common = common_args();
    let gen_cmd = construct!(GenArgs {
        builtin,
        outs,
        plugins,
        plugin_opts,
        check,
        common
    })
    .map(Cmd::Gen)
    .to_options()
    .descr("Generate code from a schema directory")
    .command("gen");
    let check_cmd = common_args()
        .map(Cmd::Check)
        .to_options()
        .descr("Validate a schema directory")
        .command("check");
    let ir_cmd = common_args()
        .map(Cmd::Ir)
        .to_options()
        .descr("Print the schema's IR as JSON (what plugins receive)")
        .command("ir");
    construct!([gen_cmd, check_cmd, ir_cmd])
}

fn split_pair(s: &str, sep: char) -> Result<(String, String), String> {
    s.split_once(sep)
        .map(|(a, b)| (a.to_owned(), b.to_owned()))
        .ok_or_else(|| format!("`{s}`: expected NAME{sep}VALUE"))
}

fn parse_opts(s: &str) -> Result<BTreeMap<String, String>, String> {
    s.split(',')
        .filter(|p| !p.is_empty())
        .map(|p| split_pair(p, '='))
        .collect()
}

fn loader(c: &Common) -> Loader {
    let mut l = Loader::new();
    for e in &c.exts {
        l = l.extension(Passthrough::new(e));
    }
    if let Some(p) = &c.lock {
        l = l.lock_path(p);
    }
    l
}

fn print_failure(f: &korek_core::Failure) {
    for r in f.rendered() {
        eprintln!("{:?}", miette::Report::new(r));
    }
}

fn gen_driver(a: &GenArgs) -> Result<Driver, String> {
    let mut d = Driver::new().check(a.check);
    for e in &a.common.exts {
        d = d.extension(Passthrough::new(e));
    }
    if let Some(p) = &a.common.lock {
        d = d.lock_path(p);
    }
    for p in &a.plugins {
        let (n, path) = split_pair(p, '=')?;
        d = d.plugin(n, path);
    }
    let mut extra: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for o in &a.plugin_opts {
        let (n, kv) = split_pair(o, ':')?;
        extra.entry(n).or_default().extend(parse_opts(&kv)?);
    }
    let mut outputs = Vec::new();
    for (name, dir, opts) in &a.builtin {
        if let Some(dir) = dir {
            let mut o = Output::new(*name, dir);
            if let Some(opts) = opts {
                o.options = parse_opts(opts)?;
            }
            outputs.push(o);
        } else if opts.is_some() {
            return Err(format!("--{name}_opt without --{name}"));
        }
    }
    for o in &a.outs {
        let (n, dir) = split_pair(o, '=')?;
        outputs.push(Output::new(n, dir));
    }
    if outputs.is_empty() {
        return Err("nothing to generate: pass --rust, --proto, --zod, --ts or --out".into());
    }
    for mut o in outputs {
        if let Some(x) = extra.remove(&o.plugin) {
            o.options.extend(x);
        }
        d = d.output(o);
    }
    if let Some(n) = extra.keys().next() {
        return Err(format!("--opt for `{n}`, which is not an output"));
    }
    Ok(d)
}

fn run_gen(a: &GenArgs) -> ExitCode {
    let d = match gen_driver(a) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    match d.run(&a.common.dir) {
        Ok(report) => {
            for w in &report.warnings {
                eprintln!("{:?}", miette::Report::new(w.render(&report.sources)));
            }
            for f in report.stale() {
                println!("wrote {}", f.path.display());
            }
            ExitCode::SUCCESS
        }
        Err(Error::Schema(f)) => {
            print_failure(&f);
            ExitCode::FAILURE
        }
        Err(Error::Generate {
            plugin,
            diagnostics,
            sources,
        }) => {
            eprintln!("error: plugin `{plugin}` failed");
            for d in diagnostics {
                eprintln!("{:?}", miette::Report::new(d.render(&sources)));
            }
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let cmd = parser()
        .to_options()
        .descr("korek: define types once, generate Rust, Protobuf, Zod and TypeScript")
        .version(env!("CARGO_PKG_VERSION"))
        .run();
    match cmd {
        Cmd::Gen(a) => run_gen(&a),
        Cmd::Check(c) => run_load(&c, false),
        Cmd::Ir(c) => run_load(&c, true),
    }
}

fn run_load(c: &Common, ir: bool) -> ExitCode {
    let l = match loader(c).load(&c.dir) {
        Ok(l) => l,
        Err(f) => {
            print_failure(&f);
            return ExitCode::FAILURE;
        }
    };
    if ir {
        match serde_json::to_string_pretty(&l.schema) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        let s = &l.schema;
        println!(
            "ok: {} types, {} keys blocks, {} services",
            s.types.len(),
            s.keys.len(),
            s.services.len()
        );
    }
    ExitCode::SUCCESS
}
