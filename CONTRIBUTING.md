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
- pull requests targeting `main`.

Standalone pushes to topic branches do not start CI. Opening or updating their
pull request into `dev` does.

## Local checks

Run the same core checks used by CI:

```bash
cargo run --locked -p qql-grammar-gen -- check
cargo run --locked -p qql-conformance -- check language/v1
cargo fmt --all -- --check
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 \
  cargo clippy --locked --all-features \
  -p qql-core -p qql-plan -p qql-embed -p qql -p qql-edge -p qql-cli \
  --all-targets -- -D warnings
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 \
  cargo test --locked --all-features \
  -p qql-core -p qql-plan -p qql-embed -p qql -p qql-cli
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
