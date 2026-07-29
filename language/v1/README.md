# QQL 1.0 language contract

This directory is the source of truth for QQL 1.0. The grammar, semantic
rules, fixtures, and canonical AST snapshots are versioned with the supported
Rust implementation so a language change and its implementation cannot drift
across repositories.

## Authority

| Artifact | Contract |
|---|---|
| [`grammar.pest`](grammar.pest) | Complete lexical and syntactic acceptance |
| [`spec/semantics.md`](spec/semantics.md) | Type, normalization, and validation rules |
| [`spec/versioning.md`](spec/versioning.md) | Compatibility and evolution policy |
| [`fixtures/valid`](fixtures/valid) | Valid feature scripts |
| [`fixtures/invalid`](fixtures/invalid) | Isolated invalid cases and stable error codes |
| [`fixtures/expected`](fixtures/expected) | Versioned `qql.ast/v1` snapshots |

`grammar.pest` is the only handwritten core syntax grammar. The generated copy
under `crates/qql-core/grammar` must never be edited directly.

The grammar owns syntax only. Collection-schema inference (dense vs sparse vs
multivector flags), embedding, and other schema-dependent validation live in
`qql-embed` / `qql-runtime` and are specified in `spec/semantics.md`.

`USING name AS MULTI` / `AS MULTIVECTOR` is part of the grammar (dense multivector
role). Parse still stores untyped `USING name` with `kind: null` until execution
prep fills roles from the collection schema.

`USING HYBRID [DENSE n] [SPARSE n] [FUSION …]` is accepted on query tails and
lowers to the same `QueryExpr::Hybrid` AST as front-form `QUERY HYBRID TEXT …`.

Product / implementation gaps (edge limits, remaining UX) live in the workspace
[`gaps.md`](../../gaps.md) and agent summary
[`skills/qql-skill/references/qql-gaps.md`](../../skills/qql-skill/references/qql-gaps.md).

## Generation

After changing `grammar.pest`, regenerate the parser input:

```bash
cargo run -p qql-grammar-gen -- generate
```

Then update `qql-core` AST lowering when the new syntax produces new or changed
AST structure. Verify that no generated artifact is stale:

```bash
cargo run -p qql-grammar-gen -- check
```

Pest compiles the generated PEG into the qql-core syntax parser. Public
`Parser::parse` and `Parser::parse_all` accept a program only when it matches
that generated parser and lowers successfully into the typed QQL AST.

## Conformance

```bash
cargo run -p qql-conformance -- check language/v1
```

Expected result:

```text
conformant: 29 valid files (217 statements), 34 invalid cases, 29 AST snapshots
```

Regenerate AST snapshots only for an intentional contract change:

```bash
cargo run -p qql-conformance -- generate language/v1
```

Snapshot generation validates all valid and invalid fixtures before writing.
Generated JSON must be reviewed together with the grammar, lowering, and
fixture diff.

## Change workflow

1. Edit [`grammar.pest`](grammar.pest).
2. Add the smallest valid and invalid fixtures proving the change.
3. Run `qql-grammar-gen generate`.
4. Update qql-core lowering if the AST shape changes.
5. Run qql-core tests and conformance.
6. Regenerate canonical AST snapshots only when intended.
7. Run `qql-grammar-gen check` and review the complete diff.

QQL 1.0 has no compatibility parser. Pre-v1 `SELECT`, `INSERT`, `BOOST`, and
other removed aliases are invalid source.
