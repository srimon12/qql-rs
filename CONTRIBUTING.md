# Contributing to QQL

QQL uses `dev` as its integration branch and `main` as its release branch.

## Branch workflow

1. Create a focused branch from the latest `dev`.
2. Open a pull request from that branch into `dev`.
3. Merge only after CI passes.
4. Accumulate release-ready work on `dev`.
5. Open one pull request from `dev` into `main`.

Pull requests into `main` from any branch other than `dev` fail the branch
policy check. Do not push directly to `main`, force-push protected branches, or
create release tags from commits that are not already on `main`.

CI runs for:

- every push to `dev`;
- pull requests targeting `dev`;
- pull requests targeting `main`;
- manual `workflow_dispatch`.

Standalone pushes to topic branches do not start CI. Opening or updating their
pull request into `dev` does.

### Required checks

Branch protection can require the single aggregate job **CI success**, which
waits on every gate below:

| Job | What it enforces |
|-----|------------------|
| Branch policy | PRs into `main` must come from `dev` |
| Release metadata | `scripts/check_release.py` + `VERSION` sync |
| Format | `cargo fmt --all -- --check` |
| Clippy & test | Published Rust crates, `--all-features`, clippy `-D warnings` |
| Feature matrix | `qql` / `qql-cli` transport features; `qql-core` feature combos |
| MSRV | `cargo check` on workspace `rust-version` |
| Docs | `cargo doc --no-deps` with `RUSTDOCFLAGS=-D warnings` for all published crates |
| Package dry-run | `cargo package` for publishable crates |
| Conformance | Grammar generation sync + `language/v1` suite |
| Python / Node / WASM bindings | Build + package smoke tests |

## Local checks

Run the same core checks used by CI:

```bash
python3 scripts/check_release.py
cargo run --locked -p qql-grammar-gen -- check
cargo run --locked -p qql-conformance -- check language/v1
cargo fmt --all -- --check
cargo clippy --locked --all-features \
  -p qql-core -p qql-plan -p qql-embed -p qql -p qql-edge -p qql-cli \
  --all-targets -- -D warnings
cargo test --locked --all-features \
  -p qql-core -p qql-plan -p qql-embed -p qql -p qql-edge -p qql-cli
```

Optional gates that CI also runs (slower):

```bash
# Feature matrix
cargo check --locked -p qql --no-default-features --features rest
cargo check --locked -p qql --no-default-features --features grpc
cargo check --locked -p qql --no-default-features
cargo check --locked -p qql-cli --no-default-features --features rest
cargo check --locked -p qql-cli --no-default-features --features grpc
cargo check --locked -p qql-core --no-default-features
cargo check --locked -p qql-core --all-features

# Docs (deny rustdoc warnings on all published crates)
RUSTDOCFLAGS='-D warnings' \
  cargo doc --locked --no-deps --all-features \
  -p qql-core -p qql-plan -p qql-embed -p qql -p qql-edge
# Separate target-dir: CLI binary is also named `qql`
RUSTDOCFLAGS='-D warnings' \
  cargo doc --locked --no-deps --all-features -p qql-cli --target-dir target/doc-cli

# Packaging
cargo package --locked -p qql-core
```

The edge bindings additionally exercise local qdrant-edge and FastEmbed. Their
first test run may download the configured ONNX model.

## Language changes

`language/v1/grammar.pest` is the handwritten source grammar. After changing
it, regenerate the parser grammar and include the generated result in the same
pull request:

```bash
cargo run -p qql-grammar-gen -- generate
cargo run -p qql-grammar-gen -- check
cargo run -p qql-conformance -- check language/v1
```

QQL 1.x accepts additive language changes only. See
`language/v1/spec/versioning.md` for the language evolution policy.

## Releases

Package releases are synchronized across the repository. Read
[`RELEASING.md`](RELEASING.md) before changing versions or creating a tag.
