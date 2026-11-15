# Contributing

Bug reports, small focused pull requests, new validation rules, generator
fixes and documentation are welcome. Include a minimal schema that shows the
problem, the command you ran, and your korek and Rust versions. Remove
credentials and anything private from schemas and logs first.

[AGENTS.md](AGENTS.md) describes the crate layout and the conventions (new
rules, diagnostics, goldens, IR and protocol changes, self-hosting); read it
before changing a generator or the IR.

## Checks

Rust 1.88 is the minimum supported version. The nix dev shell (`nix develop`)
has the pinned toolchain from `rust-toolchain.toml` plus bun, buf, protoc,
cargo-nextest and cargo-deny, which the tests and checks need. CI runs:

```sh
nix develop -c cargo fmt --all --check
nix develop -c cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
nix develop -c cargo nextest run --workspace --all-targets --locked
nix develop -c cargo test --workspace --doc --locked
nix develop -c cargo test -p korek-core --features miette --locked
nix develop -c sh -c 'bun install --frozen-lockfile && bun test && bun run typecheck'
RUSTDOCFLAGS="-D warnings" nix develop -c cargo doc --workspace --no-deps --all-features --locked
nix develop -c cargo deny check
```

After changing a generator, regenerate the golden outputs with
`KOREK_BLESS=1 cargo test -p korek-cli --test golden` (and
`KOREK_BLESS=1 cargo test -p korek-testkit` after editing the rule vectors),
then review the diff. `cargo bench -p korek-cli` runs the benchmarks in
[BENCHMARKS.md](BENCHMARKS.md).

## Packages

Every crate except `korek-fixture` and `examples/plugin-example` is published
to crates.io. Each crate has its own README (korek-cli's is the workspace
README, which is also its crate docs); keep links in them absolute so they
work on crates.io and docs.rs. `kdl-load`, `kdl-nodes` and `kdl-span` are git
dependencies on [kdl-kit](https://github.com/talaagent/kdl-kit) until it is
published; until then `cargo publish --dry-run` only succeeds for the crates
that do not depend on them (CI's package job lists the others).

The TypeScript in `ts/` only tests the generated Zod and Protobuf; it is not
an npm package.

## Releases

All published crates share one version. `release-plz.toml` keeps their
versions in sync and combines changes in the root `CHANGELOG.md`, with
`korek-cli` owning the `v<version>` tag and GitHub release.
`release_always = false` publishes only after a release PR is merged. Review
version bumps and changelog entries and require passing CI before merging.
CI is dispatched explicitly for token-created release PRs.

Release PRs use the built-in `GITHUB_TOKEN` by default, with job-level
`contents: write` and `pull-requests: write` permissions. Enable **Allow GitHub
Actions to create and approve pull requests** in the organization and
repository Actions settings, then set repository variable `RELEASE_PR=true`.
An optional `RELEASE_PLZ_TOKEN` (GitHub App token or PAT) overrides the
built-in token.

### Trusted publishing

Each crate trusts GitHub owner `talaagent`, repository `korek`, workflow
`release-plz.yml`, no environment. The publishing job has `id-token: write`;
release-plz performs the OIDC exchange itself. Do not set
`CARGO_REGISTRY_TOKEN` in that job: a persistent token overrides OIDC.

A crate's first publication needs a short-lived crates.io token with
`publish-new`; trusted publishing cannot create a crate. For the initial
rollout, dispatch the release workflow with `bootstrap=true`; it publishes
the committed versions. `release_packages` optionally selects a
comma-separated subset so dependencies can go first (korek-ir, korek-runtime,
korek-plugin, korek-gen-common, then the generators and korek-core, then
korek, korek-testkit and korek-cli). Then register the trusted publisher for
each crate and remove the token. Never commit it.

After publication, check each version on crates.io and the docs.rs builds.

## License

Contributions are licensed under Apache-2.0, the project's license.
