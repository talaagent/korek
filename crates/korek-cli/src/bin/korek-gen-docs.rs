//! The `docs` generator as a korek plugin.

fn main() -> std::process::ExitCode {
    korek_plugin::run(korek_gen_docs::plugin)
}
