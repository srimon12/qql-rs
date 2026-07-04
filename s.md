Key findings:

## CRITICAL ISSUES

### 1. 💥 MEMORY LEAK - `Box::leak` used for lifetime extension (helpers.rs:96, helpers.rs:107, query_clauses.rs:524)
The `clone_value` function in `executor/helpers.rs` uses `Box::leak` to convert `&str` to `&'static str`, leaking memory on every call. This is called for every insert payload key/value, every filter operation, etc. Every query that involves payload operations leaks memory permanently.

### 2. 💥 MEMORY LEAK - `intern_string` in parser (query_clauses.rs:521-528)
`intern_string` uses `Box::leak` and never frees the memory. Every query with inline prefetch CTEs leaks memory.

### 3. 🔴 RACE CONDITION - `Arc<dyn Embedder>` shared across queries (executor/mod.rs:250)
The `embedder` field is `Option<Arc<dyn Embedder>>`. The `HttpEmbedder` contains a `Client` which is fine, but if someone implements a non-thread-safe embedder, this would be a problem. Less critical since `Client` is `Send + Sync`.

## MODERATE ISSUES

### 4. 🔴 TOKEN STREAM CORRUPTION - `parse_delete` broken backtracking (update.rs:137-150)
`tokens_save_pos` returns 0 always and `tokens_restore_pos` is a no-op. If `parse_filter_expr` consumes tokens and then fails, the fallback path will continue from the wrong position, likely causing wrong parsing or panics.

### 5. 🟡 DIVISION BY ZERO in `build_document` (sparse.rs:211)
If `avgdl` is 0.0, `denom_scale = k1 * (1.0 - b + b * doc_len / 0.0)` will produce `inf`, and then `tf_count / inf` = 0, so it won't crash. Not a crash but could produce incorrect results.

### 6. 🟡 PANIC in `build_match_condition_expression` (formula_nodes.rs:206-221)
Uses `panic!` macro (`unreachable!` and `panic!`) instead of returning errors, which would crash the program.

### 7. 🟡 `to_float64` coerces integer to f64 (converter.rs:452-453)
All integers become f64, causing loss of precision for large integers beyond 2^53.

### 8. 🟡 PANIC in `FusionNode::execute` with `unreachable!` (query_nodes.rs:218)
Uses `unwrap_or_else(|| unreachable!())` where the `if` condition already checks state, but if the condition is true and the state mutates between check and use - unlikely but fragile.

### 9. 🔴 MISSING VALIDATION - Negative `i64` cast to `u64` through `as` (helpers.rs:75, dml.rs:663, multiple places)
In `to_point_id_static`, `Value::Int(i)` where `*i < 0` is checked. But in do_insert, `point.id = PointId::Num(*id as u64)` has no such check. Negative i64 values would silently wrap to very large u64 values.

### 10. 🟡 UNCHECKED OVERFLOW - `i64` to `u64` cast (multiple places)
In `do_insert`, `dml.rs:663`: `point.id = PointId::Num(*id as u64)` - no negative check. An insert with `id: -1` would become `18446744073709551615`.

### 11. 🟡 UNINITIALIZED MEMORY-like behavior in `embed_batch` (embedder.rs:158)
`vectors.into_iter().map(|v| v.unwrap_or_default())` - if an index is missing, returns empty vector `Vec::new()` silently instead of erroring.

## DESIGN ISSUES

### 12. 🟡 `#[no_std]` in `qql-core` but uses `alloc`
Why `no_std` if you're using `alloc` everywhere? The runtime uses `std` anyway. This limits the crate unnecessarily.

### 13. 🟡 `tokenize` function (sparse.rs:29-36)
Uses raw `unsafe` in `to_lower_ascii` (sparse.rs:62) with `String::from_utf8_unchecked`. The ASCII check ensures safety, but it's fragile.

### 14. 🟡 `QueryState` uses `Default` derive but has `Option<Arc<dyn Embedder>>`
`Default` for `Option<Arc<dyn Embedder>>` is `None`, but other fields use `Default::default()` which may leave things in unintended states.

### 15. 🟡 Error: no `source` implementation for `QqlError`
`QqlError` does not implement `std::error::Error`, so it can't be used with `?` operator in generic error contexts.

### 16. 🟡 In OR filter building (converter.rs:183-186):
`must_not: Some(Vec::new())` - empty `must_not` in an OR condition is unnecessary and semantically odd.

## SUMMARY TABLE
| # | Severity | Category | Description | Location |
|---|----------|----------|-------------|----------|
| 1 | HIGH | Memory Leak | `Box::leak` in `clone_value` | helpers.rs:96,107 |
| 2 | HIGH | Memory Leak | `Box::leak` in `intern_string` | query_clauses.rs:524 |
| 3 | HIGH | Logic Bug | Broken backtracking in `parse_delete` | update.rs:137-150 |
| 4 | HIGH | Logic Bug | Missing negative check in `do_insert` | dml.rs:663 |
| 5 | MEDIUM | Panic | `panic!` instead of error | formula_nodes.rs:206-221 |
| 6 | MEDIUM | Precision | Integer→f64 coercion | converter.rs:452-453 |
| 7 | MEDIUM | Safety | `unsafe` UTF-8 in sparse | sparse.rs:62 |
| 8 | LOW | Style | `no_std` with `alloc` | lib.rs:1 |

Now I have a thorough understanding of the entire codebase. Here is my red team analysis:

---

## Red Team Report for QQL

### 🔴 CRITICAL: Memory Leaks via `Box::leak`

**1. `clone_value`** — `crates/qql-runtime/src/executor/helpers.rs:93-114`

Every call to `clone_value` leaks memory. Strings are converted to `&'static str` via `Box::leak` and **never freed**. This function is called on every `INSERT` payload key/value, every `QUERY` filter value, every `UPDATE`, every `DELETE`, and every `RECOMMEND`/`DISCOVER`/`CONTEXT` operation. A steady stream of queries will monotonically grow memory usage.

```rust
let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
```

**2. `intern_string`** — `crates/qql-core/src/parser/query_clauses.rs:521-528`

Same pattern for CTE names. Every query with an inline `PREFETCH` clause generates unique names like `__inline_pf0`, leaks them, and never reclaims the memory.

```rust
let leaked: &'static str = Box::leak(s.into_boxed_str());
unsafe { &*(leaked as *const str) }
```

**Fix**: Use `String` fields or `Arc<str>` instead of `&'a str` / `&'static str` to avoid lifetime hacking.

---

### 🔴 CRITICAL: Broken Backtracking in `parse_delete`

**`crates/qql-core/src/parser/update.rs:137-150`**

`tokens_save_pos` always returns `0` and `tokens_restore_pos` is a no-op. When `parse_filter_expr` partially consumes the token stream and then fails (e.g., `DELETE FROM coll WHERE id = 1` is followed by more complex syntax), the fallback path parses from the wrong position, producing garbage AST or spurious errors. This is a **silent correctness bug**.

---

### 🔴 CRITICAL: Missing Integer/Negative Validation on Insert

**`crates/qql-runtime/src/executor/dml.rs:662-663`**

```rust
if let Some((_, Value::Int(id))) = id_val {
    point.id = PointId::Num(*id as u64);  // <-- no negative check
```

`INSERT INTO coll VALUES {id: -1, text: 'foo'}` silently wraps `-1` to `18446744073709551615`. The `to_point_id_static` helper in `helpers.rs:71-72` *does* check for negative, but `do_insert` bypasses it entirely.

---

### 🟡 HIGH: `panic!` / `unreachable!` Instead of Error Returns

**`crates/qql-runtime/src/pipeline/formula_nodes.rs:206, 208, 221`**

`build_match_condition_expression` uses `panic!` and `unreachable!()` when values don't match the expected type, instead of returning `Err(...)`. A crafted `MATCH` expression with mixed types causes **the entire query pipeline to crash** instead of returning a user-facing error.

---

### 🟡 HIGH: `to_float64` Coerces Integers to `f64`

**`crates/qql-runtime/src/filter_conv/converter.rs:452-453`**

```rust
Value::Int(i) => Ok(Some(*i as f64)),
```

Integers above `2^53` (9 quadrillion) lose precision silently in `WHERE`, `BETWEEN`, `>`, `>=`, `<`, `<=` conditions. This is a correctness bug for large integer payload values.

---

### 🟡 HIGH: Unsafe UTF-8 in `to_lower_ascii`

**`crates/qql-runtime/src/sparse.rs:62`**

```rust
return unsafe { String::from_utf8_unchecked(buf) };
```

The function checks for ASCII bytes only, but a future refactoring that changes the caller to pass non-ASCII text would produce corrupted strings without detection.

---

### 🟡 MEDIUM: Empty `must_not` in OR Expressions

**`crates/qql-runtime/src/filter_conv/converter.rs:183`**

```rust
Ok(QdrantCondition::Boolean(Box::new(QdrantFilter {
    must: None,
    must_not: Some(Vec::new()),  // empty must_not is sent to Qdrant
    should: Some(should),
})))
```

An empty `must_not: []` is sent to the Qdrant API in every `OR` expression. This is semantically unnecessary and could interact poorly with some Qdrant versions.

---

### 🟡 MEDIUM: Unchecked `embed_batch` Missing Index

**`crates/qql-runtime/src/embedder.rs:158`**

```rust
let result: Vec<Vec<f32>> = vectors.into_iter().map(|v| v.unwrap_or_default()).collect();
```

If the embedding API returns fewer vectors than requested, missing indices silently become empty `Vec<f32>` (a zero-dimension vector), which would later fail at Qdrant with a confusing error or panics.

---

### 🟡 MEDIUM: `#[no_std]` with Full `alloc` — Unnecessary Constraint

**`crates/qql-core/src/lib.rs:1`**

The crate is `#![no_std]` but uses `extern crate alloc` and `alloc::vec::Vec` everywhere. The runtime crate already pulls in `std`. This adds zero benefit but prevents using `std::error::Error`, `std::collections`, and other standard types, forcing the lifetime-hacking patterns above.

---

### Summary

| Severity | Issue | Location |
|----------|-------|----------|
| 🔴 **Critical** | Memory leak: `Box::leak` in `clone_value` | `helpers.rs:96` |
| 🔴 **Critical** | Memory leak: `Box::leak` in `intern_string` | `query_clauses.rs:524` |
| 🔴 **Critical** | Broken backtracking in `parse_delete` | `update.rs:137-150` |
| 🔴 **Critical** | Missing negative check in `do_insert` | `dml.rs:663` |
| 🟡 **High** | `panic!` instead of error return | `formula_nodes.rs:206-221` |
| 🟡 **High** | Integer→f64 precision loss in filters | `converter.rs:452-453` |
| 🟡 **High** | `unsafe` UTF-8 unchecked in tokenizer | `sparse.rs:62` |
| 🟡 **Medium** | Empty `must_not` in OR filters | `converter.rs:183` |
| 🟡 **Medium** | Silent missing vector in `embed_batch` | `embedder.rs:158` |
| 🟡 **Medium** | `#[no_std]` causes lifetime complexity | `lib.rs:1` |

The most impactful findings are the **memory leaks** (every query leaks) and the **broken backtracking in `parse_delete`** (silent correctness bug). The `Box::leak` pattern is used throughout to work around the `&'a str`-based AST lifetime, which the `#[no_std]` constraint indirectly forces. Dropping `no_std` and switching AST-owned fields to `String` would eliminate most of these issues in one sweep.

---

I have successfully addressed all of the critical, moderate, and design issues identified in the red team analysis. Here is a summary of the fixes applied:

### 1. Memory Leaks (`Box::leak`)
- **`clone_value` (`crates/qql-runtime/src/executor/helpers.rs`)**: Changed `ast::Value<'a>` to use `std::borrow::Cow<'a, str>` instead of raw `&'a str`. Added a `to_static()` method on `Value` that recursively creates a leak-free `Value<'static>` copy of the enum by transforming `Cow::Borrowed` to `Cow::Owned`. Replaced the leaking manual clone loop with `val.to_static()`.
- **`intern_string` / CTEs (`crates/qql-core/src/parser/query_clauses.rs`)**: Changed the AST structures `CTE<'a>` and `PrefetchRef<'a>` to hold `Cow<'a, str>` names. This allows dynamically generated prefetch names (e.g. `__inline_pf0`) to be stored directly as owned strings (`Cow::Owned`) rather than being leaked permanently to get a `'static` reference. Removed the leaking `intern_string` method completely.
- **`point_id_to_value` (`crates/qql-runtime/src/pipeline/helpers.rs`)**: Removed `Box::leak` when converting a point ID UUID back to a static value, using `std::borrow::Cow::Owned` instead.

### 2. Backtracking in `parse_delete` (`crates/qql-core/src/parser/update.rs`)
- Implemented proper backtracking by adding `save_pos()` and `restore_pos()` methods to the `Parser` which clone and restore the underlying `TokenIter` iterator state. Replaced the dummy sentinel methods in `parse_delete` with these new safe methods.

### 3. Missing Negative validation / Cast overflow on Insert (`crates/qql-runtime/src/executor/dml.rs`)
- Added validation check to `do_insert` when extracting the payload `id`. If a negative integer is passed as the ID, a runtime error is returned instead of silently casting and wrapping it to a large `u64`.

### 4. Panics in Match Condition Builder (`crates/qql-runtime/src/pipeline/formula_nodes.rs`)
- Replaced the `panic!` / `unreachable!` calls in `build_match_condition_expression` when type mismatch occurs with returning a descriptive `QqlError::runtime` error.

### 5. Division by Zero in BM25 Document Builder (`crates/qql-runtime/src/sparse.rs`)
- Validated `avgdl` in `build_document`. If it is less than or equal to `0.0`, it falls back safely to the default average document length (`256.0`), preventing division by zero.

### 6. Integer to Float Coercion Precision Loss (`crates/qql-runtime/src/filter_conv/converter.rs`)
- Added checks in `to_float64` for integers used in range comparisons. If the integer lies outside the range `[-2^53, 2^53]`, it returns a runtime error to prevent silent loss of precision when converting to `f64`.

### 7. Unsafe UTF-8 in sparse Tokenizer (`crates/qql-runtime/src/sparse.rs`)
- Replaced the raw `unsafe { String::from_utf8_unchecked(buf) }` call in `to_lower_ascii` with a safe `String::from_utf8` call falling back to `to_ascii_lowercase`.

### 8. Empty `must_not` in OR Filters (`crates/qql-runtime/src/filter_conv/converter.rs`)
- Changed `build_or_expr` to set `must_not: None` instead of `Some(Vec::new())` to avoid sending empty filter lists to Qdrant.

### 9. Missing Vector Check in `embed_batch` (`crates/qql-runtime/src/embedder.rs`)
- Added checks in `embed_batch` to verify that all batch indices are successfully populated, returning a runtime error if any expected index is missing rather than silently returning empty vectors.

### 10. Removal of unnecessary `#[no_std]` (`crates/qql-core/src/lib.rs` & `error.rs`)
- Removed `#![no_std]` constraint from the core package, which was causing complex lifetime constraints and forcing the leaking workarounds.
- Implemented `std::error::Error` for `QqlError`, allowing it to be used with the `?` operator in standard error contexts.

### 11. Explicit `Default` implementation (`crates/qql-runtime/src/pipeline/mod.rs` & `query_nodes.rs`)
- Replaced `#[derive(Default)]` on `QueryState` with a manual, fully explicit `Default` implementation.
- Cleaned up `FusionNode::execute` by replacing `unwrap_or_else(|| unreachable!())` with a clean `if let` pattern.