# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/talaagent/korek/compare/v0.1.0...v0.1.1) - 2026-11-18

### Other

- korek-runtime, korek-plugin, korek-gen-*: no_std + alloc (default std feature) ([#15](https://github.com/talaagent/korek/pull/15))
- korek-ir, korek-gen-common: no_std + alloc ([#13](https://github.com/talaagent/korek/pull/13))

### Changed (breaking)

- korek-runtime, korek-plugin and the korek-gen-* crates are `no_std` +
  `alloc`. korek-runtime and korek-plugin get a default `std` feature that
  gates `time::now` (the `past`/`future`/`within` rules) and `korek_plugin::run`;
  callers on `default-features = false` must enable `std` to keep them.
- Pattern rules use `regex-automata` instead of `regex`: the
  `korek_runtime::regex` re-export is removed and `RegexCell` replaces
  `std::sync::OnceLock` in `korek_runtime::matches`. Regenerate generated
  Rust with this korek version.

## [0.1.0](https://github.com/talaagent/korek/releases/tag/v0.1.0) - 2026-11-15

### Added

- First public release: the `korek` CLI and the korek library crates. Schema
  files in KDL v2 generate Rust (serde types, `validate()`, typed keys),
  Protobuf with protovalidate rules and services, Zod v4, TypeScript, JSON
  Schema 2020-12 and markdown docs; third-party generators run as plugins.
