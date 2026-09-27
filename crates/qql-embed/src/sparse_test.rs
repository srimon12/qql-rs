use crate::bm25_text::default_pipeline;
use crate::sparse;

/// Default-pipeline document tokens (test-side view of the shared pipeline).
fn tokens(text: &str) -> Vec<String> {
    default_pipeline()
        .doc_tokens(text)
        .expect("default word/English pipeline is infallible")
}

#[test]
fn test_tokenize_word_boundaries_lowercases_and_stems() {
    // Word tokenizer splits on every non-alphanumeric char (including `_`),
    // lowercases, then applies English snowball stemming.
    let got = tokens("Hello, World! 123 TEST_token");
    assert_eq!(got, vec!["hello", "world", "123", "test", "token"]);
}

#[test]
fn test_tokenize_removes_english_stopwords() {
    // "a", "d", "of", "the" are stopwords; single letters like "b"/"c" are not.
    let got = tokens("a b c d go rs");
    assert_eq!(got, vec!["b", "c", "go", "rs"]);
}

#[test]
fn test_tokenize_handles_hyphenated_medical_terms() {
    // Hyphens are word boundaries (server word-tokenizer behavior).
    let got = tokens("B-cell anti-NMDA CD19-negative");
    assert_eq!(got, vec!["b", "cell", "anti", "nmda", "cd19", "negat"]);
}

#[test]
fn test_tokenize_handles_unicode() {
    let got = tokens("Привет мир hello-world");
    assert_eq!(got, vec!["привет", "мир", "hello", "world"]);
}

#[test]
fn test_tokenize_splits_underscore_like_server() {
    let got = tokens("test_fn main_loop");
    assert_eq!(got, vec!["test", "fn", "main", "loop"]);
}

#[test]
fn test_tokenize_apostrophes_and_stopwords() {
    // Mirrors the server's own tokenizer test: "you'll" splits into the
    // stopwords "you" + "ll"; "be" and "in" are stopwords too.
    let got = tokens("you'll be in town");
    assert_eq!(got, vec!["town"]);
}

#[test]
fn test_tokenize_stems_inflections() {
    // Mirrors the server's snowball stemmer test.
    let got = tokens("interestingly proceeding living");
    assert_eq!(got, vec!["interest", "proceed", "live"]);
}

#[test]
fn test_token_id_deterministic() {
    assert_eq!(sparse::token_id("hello"), sparse::token_id("hello"));
    assert_ne!(sparse::token_id("hello"), sparse::token_id("world"));
    // Case is folded before hashing by the pipeline, but token_id itself is
    // raw murmur3 over the given bytes (like the server's `token_id`).
    assert_ne!(sparse::token_id("hello"), sparse::token_id("Hello"));
}

#[test]
fn test_embed_query_uses_unit_weights() {
    let v = sparse::embed_query("hello hello world");
    assert_eq!(v.indices.len(), 2);
    assert_eq!(v.values.len(), 2);
    // Indices sorted, duplicates deduped, every weight exactly 1.0.
    assert!(v.indices.windows(2).all(|w| w[0] < w[1]));
    assert!(v.values.iter().all(|&v| v == 1.0));
    assert!(v.indices.contains(&sparse::token_id("hello")));
    assert!(v.indices.contains(&sparse::token_id("world")));
}

#[test]
fn test_embed_document_uses_bm25_saturated_tf() {
    // dl=4, avgdl=4 → denom_scale = 1.2*(0.25 + 0.75*1) = 1.2.
    // tf(cat)=2 → 2*2.2/(2+1.2) = 1.375; tf(sat)=tf(mat)=1 → 2.2/2.2 = 1.0.
    let v = sparse::embed_document_with("cat sat mat cat", 1.2, 0.75, 4.0);
    assert_eq!(v.indices.len(), 3);

    let cat_idx = sparse::token_id("cat");
    let mut cat_value = 0.0f32;
    for i in 0..v.indices.len() {
        if v.indices[i] == cat_idx {
            cat_value = v.values[i];
        } else {
            assert!((v.values[i] - 1.0).abs() < 0.0001, "tf=1 must weight 1.0");
        }
    }
    assert!(
        (cat_value - 1.375).abs() < 0.0001,
        "cat value mismatch: {cat_value} != 1.375"
    );
}

#[test]
fn test_embed_document_length_normalization_downweights_long_docs() {
    let short = sparse::embed_document_with("foo bar foo", 1.2, 0.75, 5.0);
    let long = sparse::embed_document_with(
        "foo bar foo lorem ipsum dolor sit amet consectetur",
        1.2,
        0.75,
        5.0,
    );
    let foo = sparse::token_id("foo");
    let value = |v: &sparse::SparseVector| {
        v.indices
            .iter()
            .zip(&v.values)
            .find(|(i, _)| **i == foo)
            .map(|(_, v)| *v)
            .unwrap()
    };
    assert!(value(&long) < value(&short));
}

#[test]
fn test_embed_document_invalid_avgdl_falls_back_to_default() {
    let bad = sparse::embed_document_with("alpha beta", 1.2, 0.75, 0.0);
    let good = sparse::embed_document_with("alpha beta", 1.2, 0.75, sparse::DEFAULT_AVGDL);
    assert_eq!(bad.indices, good.indices);
    assert_eq!(bad.values, good.values);
}

#[test]
fn test_embed_document_merges_murmur3_collisions_deterministically() {
    // Find two distinct 4-letter words colliding under murmur3-32 (the 26^4
    // space holds ~24 such pairs, so this search always succeeds).
    let mut seen: std::collections::HashMap<u32, [u8; 4]> = std::collections::HashMap::new();
    let mut collision = None;
    'outer: for n in 0..26u32.pow(4) {
        let mut buf = [0u8; 4];
        let mut x = n;
        for slot in buf.iter_mut() {
            *slot = b'a' + (x % 26) as u8;
            x /= 26;
        }
        let id = sparse::token_id(std::str::from_utf8(&buf).unwrap());
        match seen.get(&id) {
            Some(prev) if *prev != buf => {
                collision = Some((*prev, buf));
                break 'outer;
            }
            _ => {
                seen.insert(id, buf);
            }
        }
    }
    let (w1, w2) = collision.expect("expected a murmur3 collision in the 26^4 space");
    let w1 = std::str::from_utf8(&w1).unwrap();
    let w2 = std::str::from_utf8(&w2).unwrap();
    assert_ne!(w1, w2);
    assert_eq!(sparse::token_id(w1), sparse::token_id(w2));

    // Both terms twice: merged into ONE dimension with summed count n=4.
    // dl=4, avgdl=4 → denom_scale=1.2 → tf = 4*2.2/(4+1.2) ≈ 1.6923.
    let text = format!("{w1} {w2} {w1} {w2}");
    let v = sparse::embed_document_with(&text, 1.2, 0.75, 4.0);
    assert_eq!(v.indices, vec![sparse::token_id(w1)]);
    assert!(
        (v.values[0] - 1.692_307_7f32).abs() < 1e-5,
        "got {}",
        v.values[0]
    );

    // Deterministic across repeated runs.
    let again = sparse::embed_document_with(&text, 1.2, 0.75, 4.0);
    assert_eq!(v, again);
}

#[test]
fn test_embed_returns_empty_for_empty_text() {
    let doc = sparse::embed_document("");
    assert!(doc.indices.is_empty());
    assert!(doc.values.is_empty());

    let q = sparse::embed_query("   ");
    assert!(q.indices.is_empty());
    assert!(q.values.is_empty());
}

#[test]
fn test_embed_all_stopword_text_is_empty() {
    let doc = sparse::embed_document("the and of to in");
    assert!(
        doc.indices.is_empty(),
        "stopwords-only doc must embed empty"
    );
}

/// Golden wire-compat test against real Qdrant server output.
///
/// From the Qdrant "Server-side Inference: BM25" docs: upserting
/// `"Recipe for baking chocolate chip cookies"` with `model: "qdrant/bm25"`
/// (default options) stores exactly these indices and values. Our pipeline
/// must reproduce them byte-for-byte: same murmur3-32 token IDs, same
/// stopword removal ("for"), same snowball stems, same tf saturation.
#[test]
fn test_wire_compat_with_qdrant_server_bm25() {
    let doc = sparse::embed_document("Recipe for baking chocolate chip cookies");

    let mut got = doc.indices.clone();
    got.sort_unstable();
    let mut want = vec![112174620u32, 177304315, 662344706, 771857363, 1617337648];
    want.sort_unstable();
    assert_eq!(got, want, "token IDs must match Qdrant server qdrant/bm25");

    // dl=5 (after stopword removal), tf=1, avgdl=256 → 2.2/(1 + 1.2*(0.25 +
    // 0.75*5/256)) ≈ 1.6697302 for every term.
    for &v in &doc.values {
        assert!(
            (v - 1.669_730_2f32).abs() < 1e-6,
            "tf weight {v} != 1.6697302"
        );
    }

    // Query side: "How to bake cookies?" → stopwords "how"/"to" dropped,
    // stems "bake"/"cooki" — both present in the document vector, unit weights.
    let q = sparse::embed_query("How to bake cookies?");
    assert_eq!(q.indices.len(), 2);
    assert!(q.values.iter().all(|&v| v == 1.0));
    for idx in &q.indices {
        assert!(
            doc.indices.contains(idx),
            "query token {idx} missing from document vector"
        );
    }
}

#[test]
fn test_bm25_params_default_is_byte_identical_to_legacy_defaults() {
    let text = "Recipe for baking chocolate chip cookies";
    let legacy = sparse::embed_document(text);
    let explicit = sparse::embed_document_with(
        text,
        sparse::DEFAULT_K1,
        sparse::DEFAULT_B,
        sparse::DEFAULT_AVGDL,
    );
    let params = sparse::embed_document_with_params(text, &sparse::Bm25Params::default());

    assert_eq!(legacy, explicit, "unset params must not change output");
    assert_eq!(
        legacy, params,
        "Bm25Params::default must equal legacy defaults"
    );

    let resolved = sparse::Bm25Params::resolve(None, None, None).expect("defaults are valid");
    assert_eq!(resolved, sparse::Bm25Params::default());
    assert_eq!(resolved.k1(), sparse::DEFAULT_K1);
    assert_eq!(resolved.b(), sparse::DEFAULT_B);
    assert_eq!(resolved.avg_len(), sparse::DEFAULT_AVGDL);
}

#[test]
fn test_bm25_params_non_default_changes_document_weights() {
    // dl=4, avg_len=4 → denom_scale = k1*(1 - b + b*1) = k1 = 2.0.
    // tf(cat)=2 → 2*(2+1)/(2+2) = 1.5; tf=1 → 3/3 = 1.0.
    let params = sparse::Bm25Params::new(2.0, 0.5, 4.0).expect("valid");
    let v = sparse::embed_document_with_params("cat sat mat cat", &params);
    assert_eq!(v.indices.len(), 3);

    let cat_idx = sparse::token_id("cat");
    for (idx, &value) in v.indices.iter().zip(&v.values) {
        let want = if *idx == cat_idx { 1.5f32 } else { 1.0f32 };
        assert!(
            (value - want).abs() < 1e-6,
            "index {idx}: got {value}, want {want}"
        );
    }

    // Same text under Qdrant defaults must differ (different k1/b/avg_len).
    let default = sparse::embed_document("cat sat mat cat");
    assert_ne!(v.values, default.values);
}

#[test]
fn test_bm25_params_avg_len_only_shifts_docs_off_the_average() {
    // `b` cancels when doc_len == avg_len: denom_scale = k1*(1 - b + b*1) = k1
    // for any b, so weights are identical across b values on that document.
    let b_zero = sparse::Bm25Params::new(1.2, 0.0, 3.0).expect("valid");
    let b_full = sparse::Bm25Params::new(1.2, 1.0, 3.0).expect("valid");
    let doc = sparse::embed_document_with_params("foo bar baz", &b_zero);
    let same = sparse::embed_document_with_params("foo bar baz", &b_full);
    assert_eq!(doc, same, "doc_len == avg_len must cancel b");

    // Longer docs saturate less when avg_len is small.
    let short_avg = sparse::Bm25Params::new(1.2, 0.75, 3.0).expect("valid");
    let long_avg = sparse::Bm25Params::new(1.2, 0.75, 30.0).expect("valid");
    let long = "foo bar baz qux quux corge grault garply waldo fred plugh xyzzy";
    let short_avg_vec = sparse::embed_document_with_params(long, &short_avg);
    let long_avg_vec = sparse::embed_document_with_params(long, &long_avg);
    assert_ne!(short_avg_vec.values, long_avg_vec.values);
    assert!(
        short_avg_vec.values[0] < long_avg_vec.values[0],
        "small avg_len must downweight a long doc more"
    );
}

#[test]
fn test_bm25_params_validation_fail_closed() {
    let rejected = [
        (-1.0, 0.75, 256.0),
        (f64::NAN, 0.75, 256.0),
        (f64::INFINITY, 0.75, 256.0),
        (f64::NEG_INFINITY, 0.75, 256.0),
        (1.2, -0.1, 256.0),
        (1.2, 1.1, 256.0),
        (1.2, f64::NAN, 256.0),
        (1.2, f64::INFINITY, 256.0),
        (1.2, -f64::INFINITY, 256.0),
        (1.2, 0.75, 0.0),
        (1.2, 0.75, -1.0),
        (1.2, 0.75, f64::NAN),
        (1.2, 0.75, f64::INFINITY),
        (1.2, 0.75, f64::NEG_INFINITY),
    ];
    for (k1, b, avg_len) in rejected {
        let err = sparse::Bm25Params::new(k1, b, avg_len)
            .expect_err(&format!("({k1}, {b}, {avg_len}) must fail closed"));
        assert_eq!(err.code, "QQL-VALIDATION-CONFIG", "({k1}, {b}, {avg_len})");
    }

    // Valid boundaries: b = 0 / 1 are accepted, k1 = 0 is accepted like
    // Qdrant's validator (binary weighting), any positive finite avg_len.
    for (k1, b, avg_len) in [
        (0.0, 0.75, 256.0),
        (0.0001, 0.0, 0.5),
        (100.0, 1.0, 1e9),
        (1.2, 0.75, 256.0),
    ] {
        sparse::Bm25Params::new(k1, b, avg_len)
            .unwrap_or_else(|e| panic!("({k1}, {b}, {avg_len}) must be valid: {e}"));
    }
}

#[test]
fn test_bm25_params_resolve_applies_per_field_overrides() {
    let params = sparse::Bm25Params::resolve(Some(3.0), None, Some(8.0)).expect("valid");
    assert_eq!(params.k1(), 3.0);
    assert_eq!(params.b(), sparse::DEFAULT_B);
    assert_eq!(params.avg_len(), 8.0);

    let err =
        sparse::Bm25Params::resolve(Some(-1.0), None, None).expect_err("negative k1 rejected");
    assert_eq!(err.code, "QQL-VALIDATION-CONFIG");
    let err = sparse::Bm25Params::resolve(None, Some(2.0), None).expect_err("b > 1 rejected");
    assert_eq!(err.code, "QQL-VALIDATION-CONFIG");
    let err = sparse::Bm25Params::resolve(None, None, Some(f64::NAN)).expect_err("NaN rejected");
    assert_eq!(err.code, "QQL-VALIDATION-CONFIG");
}

// ── Tokenizer property coverage (deterministic, no external dep) ──────────

/// Deterministic xorshift64* generator so the property cases are reproducible
/// without pulling a fuzz/proptest dependency into the crate.
struct XorShift64(u64);

impl XorShift64 {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

/// Random-ish UTF-8: ASCII words, whitespace, punctuation, CJK, accented and
/// astral code points, combining marks, and `char::REPLACEMENT_CHARACTER`
/// surrogates are impossible by construction (they are not `char`s).
fn generated_utf8(rng: &mut XorShift64, len: usize) -> String {
    const POOLS: [&[char]; 6] = [
        &[
            'a', 'z', 'A', 'Z', '0', '9', '_', '-', '\'', '.', ',', '!', ' ',
        ],
        &['é', 'à', 'ü', 'ñ', 'ß', 'ø', 'å'],
        &['你', '好', '世', '界', '日', '本', '語'],
        &['😀', '🚀', '🧪', '🦀'],
        &['\u{0301}', '\u{0308}', '\u{200d}', '\u{200b}', '\u{00ad}'],
        &['\n', '\t', '\r', '\u{2028}'],
    ];
    let mut out = String::new();
    for _ in 0..len {
        let pick = (rng.next() % POOLS.len() as u64) as usize;
        let pool = POOLS[pick];
        out.push(pool[(rng.next() % pool.len() as u64) as usize]);
    }
    out
}

#[test]
fn tokenizer_never_panics_and_query_ids_are_sorted_unique() {
    let mut rng = XorShift64(0x9E37_79B9_7F4A_7C15);
    for case in 0..2000 {
        let len = (rng.next() % 40) as usize;
        let text = generated_utf8(&mut rng, len);
        let query = sparse::embed_query(&text);
        assert_eq!(
            query.indices.len(),
            query.values.len(),
            "case {case}: index/value length mismatch for {text:?}"
        );
        assert!(
            query.indices.windows(2).all(|w| w[0] < w[1]),
            "case {case}: query ids must be sorted and unique for {text:?}"
        );
        assert!(
            query.values.iter().all(|&v| v == 1.0),
            "case {case}: query weights must be unit"
        );

        let document = sparse::embed_document(&text);
        assert_eq!(document.indices.len(), document.values.len());
        assert!(
            document.indices.windows(2).all(|w| w[0] < w[1]),
            "case {case}: document ids must be sorted and unique for {text:?}"
        );
        assert!(
            document.values.iter().all(|v| v.is_finite() && *v > 0.0),
            "case {case}: document weights must be finite and positive for {text:?}"
        );

        // Query terms are a subset of document terms for identical text.
        for &id in &query.indices {
            assert!(
                document.indices.contains(&id),
                "case {case}: query id {id} missing from document ids for {text:?}"
            );
        }
    }
}

#[test]
fn tokenizer_handles_malformed_whitespace_runs() {
    // Separator-only and mixed-separator inputs must produce empty vectors
    // rather than panicking on byte slicing.
    for text in ["", " ", "\u{00a0}\u{2028}\t\r", "_-_.-", "\u{200b}\u{200d}"] {
        assert!(sparse::embed_query(text).indices.is_empty(), "{text:?}");
        assert!(sparse::embed_document(text).indices.is_empty(), "{text:?}");
        assert!(tokens(text).is_empty(), "{text:?}");
    }
}
