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
