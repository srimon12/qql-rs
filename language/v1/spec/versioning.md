# QQL versioning policy

QQL uses `MAJOR.MINOR`. The current language version is `1.2`; the canonical AST
schema identifier is independently fixed as `qql.ast/v1`.

## Compatibility rule

Within one major version, every program valid in an earlier minor release must
remain valid with the same meaning and canonical AST. A minor release may make
previously invalid source valid by adding new syntax; therefore invalidity is
not generally monotonic.

Breaking changes require the next major version.

## Major changes

Any of these requires QQL 2:

- removing or renaming syntax;
- changing clause order or operator precedence;
- changing the meaning of an existing expression, operator, or config key;
- changing how an existing valid fixture normalizes;
- renaming/removing an existing canonical AST tag or field;
- changing tokenization so existing source parses differently;
- changing a stable error code asserted by a v1 invalid fixture;
- making schema inference choose a target where QQL 1 requires ambiguity.

For example, assigning special selection priority to a vector literally named
`dense` would break the v1 named-vector contract.

## Minor changes

QQL 1.x may add:

- a statement or query-expression form;
- a filter or formula function;
- a new optional clause after all existing clauses;
- a recommendation/fusion/distance/index value;
- a config or search parameter;
- a new canonical AST variant for the new syntax;
- a new error code for a previously unspecified case.

An additive change must not alter any existing expected snapshot. It adds new
fixtures and snapshots.

Pure prose corrections that do not change accepted source, semantics, errors,
or canonical AST do not require a version increment.

## Canonical AST stability

`qql.ast/v1` remains stable for all QQL 1.x releases:

- existing tags and fields retain meaning;
- existing optional fields remain present and nullable;
- ordered arrays remain ordered;
- six-decimal float normalization remains unchanged;
- existing fixture snapshots are immutable.

A new language feature may introduce a new enum tag or fields inside that new
tag. A representation change for existing nodes requires `qql.ast/v2` and a
QQL major-version review.

## Deprecation

A v1 feature may be marked deprecated but cannot be removed in v1. Deprecation
must state:

1. the deprecated syntax/behavior;
2. its replacement;
3. the reason;
4. the earliest major release that may remove it.

At least one published minor release must contain the notice before removal.
QQL 1.0 contains no deprecated syntax. `INSERT`, `SELECT`, and `BOOST` are
pre-v1 legacy syntax, not deprecated v1 aliases.

## Change procedure

For changes after the 1.0 baseline:

1. propose the normative grammar and semantic change;
2. classify it as clarification, additive minor, or breaking major;
3. add focused valid/invalid fixtures and define AST impact;
4. regenerate the qql-core parser input;
5. implement any required AST lowering in `qql-core`;
6. regenerate snapshots only when new syntax requires new snapshots;
7. run reference conformance;
8. publish the qql-rs commit/tag.

The written spec is authoritative during this process. Reference code is the
executable verifier, not a second private specification.

## Version history

| Version | Date | Summary |
|---|---|---|
| 1.0 | 2026-07-26 | Unified QUERY/UPSERT grammar, arbitrary named-vector roles, schema-safe inference, 29 valid files, 34 invalid cases, and generated `qql.ast/v1` snapshots |
| 1.1 | 2026-07-28 | Additive minor features: ON FIELD and INTO spec modifiers, multi-spec embedding options, raw strings (r'...', r"..."), triple-quoted multiline strings ('''...''', """..."""), backtick strings (`...`), and \$ string escape sequences |
| 1.2 | 2026-07-29 | Additive minor features: ColBERT multivectors (`AS MULTI` / `AS MULTIVECTOR`), CLIP vision (`QUERY IMAGE`), `CROSS RERANK`, `USING HYBRID`, `acorn` search params, exact point counting (`COUNT WITH (exact = true)`), and specific payload deletion (`DELETE PAYLOAD key1, key2 FROM coll`).
| 1.2.1 | 2026-07-31 | Contract corrections & hardening: fixed `field_name` typo rule in `delete_payload` grammar, declared `MULTI`, `MULTIVECTOR`, `IMAGE` in `single_embedding_spec`, added `sharding_method` rules (`auto`, `custom`), generated `keywords.generated.rs` artifact from `grammar.pest`, added `embedding-multi-image.qql` fixtures, and enforced bi-directional reverse-drift CI checks. |

`qql-rs` is the supported reference implementation. An implementation version
number does not imply QQL conformance; conformance is claimed only against a
specific qql-rs language tag/commit and its unchanged fixtures.
