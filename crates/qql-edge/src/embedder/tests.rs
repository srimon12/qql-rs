use super::bm25::edge_bm25_config;
use super::catalog::{is_sparse_alias, resolve_sparse_model};
use super::*;
use fastembed::SparseModel;
use qdrant_edge::TokenizerType;
use qdrant_edge::bm25_embed::{EdgeBm25, EdgeBm25Config};
use qql_embed::Bm25TextConfig;
use std::path::PathBuf;

#[test]
fn cache_dir_key_empty_for_none() {
    assert_eq!(cache_dir_key(None), "");
}

#[test]
fn cache_dir_key_preserves_path() {
    let p = PathBuf::from("/tmp/my_cache");
    assert_eq!(cache_dir_key(Some(&p)), "/tmp/my_cache");
}

#[test]
fn resolve_sparse_model_splade_default() {
    let m = resolve_sparse_model("").unwrap();
    assert_eq!(m, SparseModel::SPLADEPPV1);
}

#[test]
fn resolve_sparse_model_by_alias() {
    let m = resolve_sparse_model("splade").unwrap();
    assert_eq!(m, SparseModel::SPLADEPPV1);
}

#[test]
fn resolve_sparse_model_by_enum_name() {
    let m = resolve_sparse_model("SPLADEPPV1").unwrap();
    assert_eq!(m, SparseModel::SPLADEPPV1);
}

#[test]
fn resolve_sparse_model_by_model_code() {
    let m = resolve_sparse_model("Qdrant/Splade_PP_en_v1").unwrap();
    assert_eq!(m, SparseModel::SPLADEPPV1);
}

#[test]
fn resolve_sparse_model_bgem3() {
    let m = resolve_sparse_model("bge-m3").unwrap();
    assert_eq!(m, SparseModel::BGEM3);
    let m = resolve_sparse_model("BGEM3").unwrap();
    assert_eq!(m, SparseModel::BGEM3);
    let m = resolve_sparse_model("BAAI/bge-m3").unwrap();
    assert_eq!(m, SparseModel::BGEM3);
}

#[test]
fn resolve_sparse_model_unknown_errors() {
    let e = resolve_sparse_model("nonexistent_model").unwrap_err();
    assert!(e.message.contains("nonexistent_model"));
}

#[test]
fn is_sparse_alias_matches() {
    assert!(is_sparse_alias("splade"));
    assert!(is_sparse_alias("SPLADE"));
    assert!(is_sparse_alias("bge-m3"));
    assert!(is_sparse_alias("bgem3"));
    assert!(!is_sparse_alias("unknown"));
}

#[test]
fn options_default_has_no_sparse_model() {
    let opts = FastEmbedderOptions::default();
    assert!(opts.sparse_model.is_none());
    assert!(opts.model.is_none());
}

#[test]
fn options_with_sparse_model() {
    let opts = FastEmbedderOptions {
        sparse_model: Some("splade".into()),
        ..Default::default()
    };
    assert_eq!(opts.sparse_model.as_deref(), Some("splade"));
}

#[test]
fn edge_bm25_config_maps_validated_params() {
    let text = Bm25TextConfig::resolve(
        Some(2.0),
        Some(0.5),
        Some(12.0),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let cfg = edge_bm25_config(&text).unwrap();
    assert_eq!(cfg.k.into_inner(), 2.0);
    assert_eq!(cfg.b.into_inner(), 0.5);
    assert_eq!(cfg.avg_len.into_inner(), 12.0);
    // Unset text knobs forward as explicit Qdrant defaults.
    assert_eq!(cfg.language.as_deref(), Some("english"));
    assert_eq!(cfg.tokenizer, TokenizerType::Word);
    assert_eq!(cfg.lowercase, Some(true));
    assert_eq!(cfg.ascii_folding, Some(false));
    assert_eq!(cfg.min_token_len, None);
    assert_eq!(cfg.max_token_len, None);
}

#[test]
fn edge_bm25_config_default_matches_engine_default() {
    // Our defaults forward as explicit English config, which is what the
    // engine resolves internally — so an unset configuration changes
    // nothing for existing edge documents (behavioral equality, not
    // struct equality: `None` vs explicit-English differ textually).
    let mapped = edge_bm25_config(&Bm25TextConfig::default()).unwrap();
    let engine_default = EdgeBm25::new(EdgeBm25Config::default()).unwrap();
    let engine_mapped = EdgeBm25::new(mapped).unwrap();
    let text = "Recipe for baking chocolate chip cookies";
    let a = engine_default.embed_document(text);
    let b = engine_mapped.embed_document(text);
    assert_eq!(a.indices, b.indices);
    assert_eq!(a.values, b.values);
}

#[test]
fn edge_bm25_config_applied_params_change_document_vectors() {
    // Non-default k1/b/avg_len must land in the document weights, and the
    // engine encoder must agree with `qql_embed::sparse` to f32 tolerance.
    let text_config = Bm25TextConfig::resolve(
        Some(2.0),
        Some(0.25),
        Some(4.0),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let engine = EdgeBm25::new(edge_bm25_config(&text_config).unwrap()).unwrap();
    let text = "cat sat mat cat";
    let got = engine.embed_document(text);
    let want = qql_embed::sparse::embed_document_with_params(text, &text_config.params);
    assert_eq!(got.indices, want.indices);
    for (g, w) in got.values.iter().zip(&want.values) {
        assert!((g - w).abs() < 1e-6, "edge {g} != qql-embed {w}");
    }

    let default = EdgeBm25::new(EdgeBm25Config::default()).unwrap();
    assert_ne!(
        default.embed_document(text).values,
        got.values,
        "configured params must differ from the engine defaults"
    );
}

/// Our reimplemented pipeline must agree with Qdrant's real engine
/// across languages, tokenizers, folding, and length limits — same
/// config in, same sparse vector out. (No network: both sides are pure.)
#[test]
fn local_pipeline_matches_engine_across_configs() {
    let cases: &[(&str, Bm25TextConfig)] = &[
        ("The Time Machine", Bm25TextConfig::default()),
        (
            "La Máquina del Tiempo",
            Bm25TextConfig::resolve(
                None,
                None,
                None,
                Some("spanish"),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap(),
        ),
        (
            "Le temps des cerises",
            Bm25TextConfig::resolve(
                None,
                None,
                None,
                Some("french"),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap(),
        ),
        (
            "Die Verwandlung",
            Bm25TextConfig::resolve(
                None,
                None,
                Some(5.0),
                Some("german"),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap(),
        ),
        (
            "Mieville café",
            Bm25TextConfig::resolve(
                None,
                None,
                None,
                None,
                None,
                None,
                Some(true),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap(),
        ),
        (
            "hello-world",
            Bm25TextConfig::resolve(
                None,
                None,
                None,
                None,
                Some("whitespace"),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap(),
        ),
        (
            "prefix test",
            Bm25TextConfig::resolve(
                None,
                None,
                None,
                None,
                Some("prefix"),
                None,
                None,
                None,
                None,
                Some(2),
                Some(4),
                None,
            )
            .unwrap(),
        ),
    ];
    for (text, config) in cases {
        let engine = EdgeBm25::new(edge_bm25_config(config).unwrap()).unwrap();
        let pipeline = config.pipeline();
        for (got, want) in [
            (
                engine.embed_query(text),
                pipeline.embed_query(text).expect("query embeds"),
            ),
            (
                engine.embed_document(text),
                pipeline.embed_document(text).expect("doc embeds"),
            ),
        ] {
            let want_qql = qql_embed::SparseVector {
                indices: want.indices.clone(),
                values: want.values.clone(),
            };
            assert_eq!(
                got.indices, want_qql.indices,
                "index mismatch for {text:?} with {config:?}"
            );
            assert_eq!(
                got.values.len(),
                want_qql.values.len(),
                "value count mismatch for {text:?} with {config:?}"
            );
            for (g, w) in got.values.iter().zip(&want_qql.values) {
                assert!(
                    (g - w).abs() < 1e-6,
                    "weight mismatch for {text:?} with {config:?}: edge {g} != local {w}"
                );
            }
        }
    }
}

/// Explicit stemmer/stopword overrides must survive the serde round-trip
/// into the engine and behave identically on both sides.
#[test]
fn edge_bm25_overrides_round_trip() {
    use qql_embed::{Stemmer, Stopwords};

    // Disabled stemmer + no stopwords: inflections stay distinct.
    let config = Bm25TextConfig {
        stemmer: Some(Stemmer::Disabled),
        stopwords: Some(Stopwords::default()),
        ..Bm25TextConfig::default()
    };
    let engine = EdgeBm25::new(edge_bm25_config(&config).unwrap()).unwrap();
    let local = config
        .pipeline()
        .embed_document("running run")
        .expect("doc");
    assert_eq!(engine.embed_document("running run").indices.len(), 2);
    assert_eq!(local.indices.len(), 2);

    // Custom stopwords replace the default list.
    let config = Bm25TextConfig {
        stopwords: Some(Stopwords {
            languages: Vec::new(),
            custom: vec!["cat".to_string()],
        }),
        stemmer: Some(Stemmer::Disabled),
        ..Bm25TextConfig::default()
    };
    let engine = EdgeBm25::new(edge_bm25_config(&config).unwrap()).unwrap();
    let local = config.pipeline().embed_document("the cat").expect("doc");
    assert_eq!(engine.embed_document("the cat").indices.len(), 1);
    assert_eq!(local.indices.len(), 1);

    // Explicit Snowball stemmer (French over French text).
    let config = Bm25TextConfig {
        language: qql_embed::Language::French,
        stemmer: Some(Stemmer::Snowball(qql_embed::Language::French)),
        ..Bm25TextConfig::default()
    };
    let engine = EdgeBm25::new(edge_bm25_config(&config).unwrap()).unwrap();
    let local = config.pipeline().embed_document("chats chat").expect("doc");
    assert_eq!(engine.embed_document("chats chat").indices, local.indices);

    // A directly constructed unreachable stemmer fails closed here, not
    // silently inside the engine.
    let config = Bm25TextConfig {
        stemmer: Some(Stemmer::Snowball(qql_embed::Language::Chinese)),
        ..Bm25TextConfig::default()
    };
    edge_bm25_config(&config).expect_err("chinese snowball must fail closed");
}
