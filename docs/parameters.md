# Parameter Binding & Prepared Statements

QQL provides type-safe parameter binding and prepared query templates through `qql_core::params` and `qql::executor::Executor`.

---

## 1. Placeholders & Syntax

QQL supports two parameter binding styles:

| Style | Syntax | Example | Description |
|---|---|---|---|
| **Named** | `:name` | `QUERY TEXT :query FROM docs WHERE tenant = :tenant LIMIT :lim;` | Mapped by variable name (without `:`). |
| **Positional** | `?` | `QUERY TEXT ? FROM docs WHERE tenant = ? LIMIT ?;` | Sequential 1-to-1 mapping with parameters list. |

---

## 2. Language Grammar & Security Rationale

### Why not `$name` or `$1`?
In QQL, **`$` is a first-class identifier character** (for example, fields like `$category`, `$id`, or `$tenant_id`).

If parameter binding used `$name` or `$1`, queries containing valid `$`-prefixed fields would be silently rewritten or corrupted (e.g., `WHERE $category = 'medical'` accidentally becoming `WHERE 'cardio' = 'medical'`).

To guarantee zero collisions:
- **`:name`** is reserved for named templates (`:` cannot start a QQL identifier).
- **`?`** is reserved for positional templates (`?` is rejected outside strings by the QQL lexer).
- All `$`-identifiers in queries are preserved verbatim.

### Token Boundary Rule for `:name`
A colon `:` is only treated as a placeholder when it occurs at a valid token boundary (preceded by whitespace, start of query, or operator/punctuation like `=`, `(`, `[`, `,`).

If a colon immediately follows an identifier character (`a:b`) or closing quote (`'a':b`, `"a":b`, `` `a`:b ``), it is recognized as a compact dictionary key-value separator, never as a placeholder `:b`. To bind a dictionary value, write `{key: :val}`. Note that unconventional spacing with whitespace before the colon (`{a :b}`) makes `:b` lexically indistinguishable from a placeholder and will be treated as one.

---

## 3. Injection Prevention & Escaping Guarantees

When substituting parameters into templates, the binder applies strict literal formatting:

1. **Strings**: Escaped with QQL single-quote rules (`'` $\to$ `''`, `\` $\to$ `\\`, `\n`, `\t`).
2. **Numbers & Floats**: Rendered canonically. Non-finite floats (`NaN`, `+Infinity`, `-Infinity`) are rejected with `QQL-BIND-INVALID-FLOAT`.
3. **Dictionaries**: Keys with colons, quotes, spaces, or dots are escaped and quoted (`{'a: 1, b': 5}`).
4. **Preserved Literals**: Source comments (`-- ...`), string literals (`'hello :name'`), and backtick strings (`` `C:\path\:dir` ``) in the template are preserved verbatim and never altered.
5. **Mixed-Style Detection**: Passing `?` to a named binder or `:name` to a positional binder returns `QQL-BIND-MIXED-STYLE` with clear guidance.

---

## 4. Usage Examples

### Rust API (`qql` runtime)

```rust
use std::collections::HashMap;
use qql::executor::Executor;
use qql_core::ast::Value;

// Named parameter execution
let mut params = HashMap::new();
params.insert("query".into(), Value::Str("chest pain".into()));
params.insert("tenant".into(), Value::Str("hospital-east".into()));
params.insert("lim".into(), Value::Int(5));

let report = executor.execute_with_params(
    "QUERY TEXT :query FROM medical WHERE department = :tenant LIMIT :lim;",
    &params,
    qql::executor::OnError::Stop,
).await?;

// Positional parameter execution
let pos_params = vec![
    Value::Str("chest pain".into()),
    Value::Str("cardio".into()),
    Value::Int(5),
];

let report = executor.execute_with_positional_params(
    "QUERY TEXT ? FROM medical WHERE department = ? LIMIT ?;",
    &pos_params,
    qql::executor::OnError::Stop,
).await?;
```

### Host SDKs (Python / Node / WASM)

One `bind` plus `execute(..., params=)`. Dict/object → named (`:name`); list/array → positional (`?`). Mixed styles fail with `QQL-BIND-MIXED-STYLE`. Rust keeps the typed twins above.

```python
from pyqql import Client, bind

client = Client("http://localhost:6333")
client.execute(
    "QUERY TEXT :q FROM docs LIMIT :lim",
    params={"q": "chest pain", "lim": 5},
)
bound = bind("QUERY TEXT :q FROM docs LIMIT :lim", {"q": "chest pain", "lim": 5})
```

```javascript
const { Client, bind } = require("@veristamp/nqql");
const client = new Client({ url: "http://localhost:6333" });
await client.execute("QUERY TEXT :q FROM docs LIMIT :lim", {
  params: { q: "chest pain", lim: 5 },
});
bind("QUERY TEXT :q FROM docs LIMIT :lim", { q: "chest pain", lim: 5 });
```

WASM `bind` takes a JS object or array (not a JSON string). The same object/array goes on `client.execute(query, { params })`; `params` is optional — without it the query is returned unchanged.

---

## 5. Prepared Statement Surface (QQL 1.7)

Pre-parsed `Stmt` handles can carry parameters without re-lexing the source:

| Host | Bind | Compile route | Direct hits |
|---|---|---|---|
| Python (`pyqql`, `pyqql-edge`) | `stmt.bind(params)` → new `Stmt` | `stmt.compile_route(params=None)` | `client.execute_hits(...)` / `execute_async_hits` |
| Node (`nqql`, `nqql-edge`) | `stmt.bind(params?)` → new `Stmt` | `stmt.compileRoute(params?)` | `client.executeHits(...)` / module `executeHits` |
| WASM (`qql-wasm`) | `stmt.bind(params?)` → new `Stmt` | `stmt.compileRoute(params?)` | — (no `executeHits`) |
| Rust | `qql_core::params::bind_stmt(&mut Stmt, lookup, &positional)` | `compile_statement(&stmt)` after binding | — |

`str(stmt)` (Python) / `stmt.toString()` (Node, WASM) render canonical
re-parseable QQL; `repr(stmt)` / `stmt.toReadableString()` render a readable
preview with long vectors truncated. The module-level `bind` accepts a string
or a `Stmt`: a `Stmt` returns a bound `Stmt`, or the readable string when
`truncate_vectors` / `truncateVectors` is set.

Additional shapes:

- **Nested expansion**: `{"loc": {"lat": 1.0, "lon": 2.0}}` binds `:loc.lat`
  and `:loc.lon`; flat dotted keys (`{"loc.lat": 1.0}`) are equivalent.
- **Statement-scoped batch params**: pass `params` as a list/array whose
  entries are all dicts/objects or arrays — one container per statement
  (object → named, array → positional for that statement). The length must
  match the statement count exactly (`QQL-BIND-BATCH-LENGTH` otherwise);
  partial binding is rejected rather than silently dropping placeholders.
  Any other params shape (object, scalar list, scalar) applies identically to
  every statement: a scalar list is a *shared* positional list, never
  per-statement.
- **`is_valid`** runs the full parse + plan gate (`qql_plan::parse_and_plan`),
  not just lexing, on `pyqql`, `pyqql-edge`, `nqql`, and `nqql-edge`.

```python
from pyqql import Client, parse

client = Client("http://localhost:6333")
stmt = parse("QUERY TEXT :q FROM docs WHERE category = :cat LIMIT :lim")[0]

bound = stmt.bind({"q": "cardiology", "cat": "medical", "lim": 10})
route = stmt.compile_route(params={"q": "cardiology", "cat": "medical", "lim": 10})
hits = client.execute_hits(stmt, params={"q": "cardiology", "cat": "medical", "lim": 10})
```

---

## 6. Error Codes

All binding failures are validation errors with a stable `QQL-BIND-*` code:

| Code | Meaning |
|---|---|
| `QQL-BIND-MIXED-STYLE` | The template mixes `:name` and `?`, or the binder received the other style |
| `QQL-BIND-MISSING-PARAM` | A named placeholder has no bound value, or a positional index is out of range |
| `QQL-BIND-UNUSED-PARAMS` | More positional values were supplied than `?` placeholders |
| `QQL-BIND-INVALID-FLOAT` | A non-finite float (`NaN`, infinity) cannot be rendered as a literal |
| `QQL-BIND-INVALID-POINT-ID` | A bound point ID is neither an unsigned integer nor a string |
| `QQL-BIND-TYPE-MISMATCH` | A bound value has the wrong type for its position (e.g. a non-string bound to `TEXT`) |
| `QQL-BIND-INVALID-INTEGER` | A `LIMIT` / `OFFSET` parameter is not a non-negative integer |
| `QQL-BIND-FORMULA-TYPE` | A formula parameter cannot be bound to a numeric, datetime, or variable constant |
| `QQL-BIND-BATCH-LENGTH` | A statement-scoped params list length does not match the statement count |
| `QQL-BIND-INVALID-PARAMS` | The `params` argument is neither an object (named) nor an array (positional) |

Codes 1–8 live in `crates/qql-core/src/params.rs`; `QQL-BIND-INVALID-PARAMS`
and `QQL-BIND-BATCH-LENGTH` are enforced by the shared JSON binding layer in
`crates/qql-core/src/params_json.rs`, which every SDK binding routes through.
