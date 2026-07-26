use crate::sparse;

#[test]
fn test_tokenize_lowercases_and_filters() {
    let got = sparse::tokenize("Hello, World! 123 TEST_token");
    assert_eq!(got, vec!["hello", "world", "123", "test_token"]);
}

#[test]
fn test_tokenize_filters_single_chars_except_special() {
    let got = sparse::tokenize("a b c d go rs");
    assert_eq!(got, vec!["c", "go", "rs"]);
}

#[test]
fn test_tokenize_handles_hyphenated_medical_terms() {
    let got = sparse::tokenize("B-cell anti-NMDA CD19-negative");
    assert_eq!(got.len(), 5);
    for want in &["cell", "anti", "nmda", "cd19", "negative"] {
        assert!(got.contains(&want.to_string()), "missing token: {want}");
    }
}

#[test]
fn test_tokenize_handles_unicode() {
    let got = sparse::tokenize("Привет мир hello-world");
    assert_eq!(got.len(), 4);
    for want in &["привет", "мир", "hello", "world"] {
        assert!(got.contains(&want.to_string()), "missing token: {want}");
    }
}

#[test]
fn test_tokenize_handles_underscore() {
    let got = sparse::tokenize("test_fn main_loop");
    assert_eq!(got, vec!["test_fn", "main_loop"]);
}

#[test]
fn test_hash_token_deterministic() {
    let a = sparse::hash_token("hello");
    let b = sparse::hash_token("hello");
    assert_eq!(a, b);
}

#[test]
fn test_hash_token_different_for_different_inputs() {
    let a = sparse::hash_token("hello");
    let b = sparse::hash_token("world");
    assert_ne!(a, b);
}

#[test]
fn test_hash_token_length_prefix_avoids_collision() {
    let a = sparse::hash_token("ab");
    let b = sparse::hash_token("abc");
    assert_ne!(a, b);
}

#[test]
fn test_build_query_uses_log_tf() {
    let v = sparse::build_query("hello hello world");
    assert_eq!(v.indices.len(), 2);
    assert_eq!(v.values.len(), 2);

    let hello_idx = sparse::hash_token("hello");
    let world_idx = sparse::hash_token("world");

    let mut hello_value = 0.0f32;
    let mut world_value = 0.0f32;
    for i in 0..v.indices.len() {
        if v.indices[i] == hello_idx {
            hello_value = v.values[i];
        }
        if v.indices[i] == world_idx {
            world_value = v.values[i];
        }
    }

    let expected_hello = 1.0 + (2.0f64).ln() as f32;
    assert!(
        (hello_value - expected_hello).abs() < 0.0001,
        "hello value mismatch: {hello_value} != {expected_hello}"
    );
    assert!(
        (world_value - 1.0).abs() < 0.0001,
        "world value mismatch: {world_value} != 1.0"
    );
}

#[test]
fn test_build_document_uses_bm25_saturated_tf() {
    let v = sparse::build_document("hello hello world", 1.2, 0.75, 256.0);
    assert_eq!(v.indices.len(), 2);

    let hello_idx = sparse::hash_token("hello");
    let world_idx = sparse::hash_token("world");

    let mut hello_value = 0.0f32;
    let mut world_value = 0.0f32;
    for i in 0..v.indices.len() {
        if v.indices[i] == hello_idx {
            hello_value = v.values[i];
        }
        if v.indices[i] == world_idx {
            world_value = v.values[i];
        }
    }

    let denom = 1.2 * (0.25 + 0.75 * 3.0 / 256.0);
    let expected_hello = (2.0 * 2.2) / (2.0 + denom);
    let expected_world = (1.0 * 2.2) / (1.0 + denom);

    assert!(
        (hello_value - expected_hello as f32).abs() < 0.0001,
        "hello value mismatch: {hello_value} != {expected_hello}"
    );
    assert!(
        (world_value - expected_world as f32).abs() < 0.0001,
        "world value mismatch: {world_value} != {expected_world}"
    );
}

#[test]
fn test_build_returns_empty_for_empty_text() {
    let doc = sparse::build_document_default("");
    assert!(
        doc.indices.is_empty(),
        "expected empty indices for empty document"
    );
    assert!(
        doc.values.is_empty(),
        "expected empty values for empty document"
    );

    let q = sparse::build_query("");
    assert!(
        q.indices.is_empty(),
        "expected empty indices for empty query"
    );
    assert!(q.values.is_empty(), "expected empty values for empty query");
}
