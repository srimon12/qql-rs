//! Tests for the full BM25 text pipeline: languages, tokenizers, folding,
//! length limits, estimator, and option resolution. Behavioral expectations
//! mirror Qdrant's own `EdgeBm25` tests where applicable.

use crate::bm25_lang::Language;
use crate::bm25_text::{AvgLenEstimate, Bm25Pipeline, Bm25TextConfig, Stemmer, Tokenizer};
use crate::sparse;

fn plain(language: &str) -> Bm25Pipeline {
    Bm25TextConfig::resolve(
        None,
        None,
        None,
        Some(language),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .expect("valid language")
    .pipeline()
}

#[test]
fn language_parse_accepts_names_and_aliases() {
    assert_eq!(Language::parse("spanish").unwrap(), Language::Spanish);
    assert_eq!(Language::parse("es").unwrap(), Language::Spanish);
    assert_eq!(Language::parse("ES").unwrap(), Language::Spanish);
    assert_eq!(Language::parse("Chinese").unwrap(), Language::Chinese);
    assert_eq!(Language::parse("zh").unwrap(), Language::Chinese);
    assert_eq!(Language::parse("hi-en").unwrap(), Language::Hinglish);
    assert_eq!(Language::parse("english").unwrap(), Language::English);
    let err = Language::parse("klingon").expect_err("unknown language rejects");
    assert_eq!(err.code, "QQL-VALIDATION-CONFIG");
    let err = Language::parse("armenian").expect_err("no such processing language");
    assert_eq!(err.code, "QQL-VALIDATION-CONFIG");
}

#[test]
fn stemmer_coverage_matches_qdrant() {
    // Qdrant's 17 Snowball-via-language set: every other language has
    // no default stemmer (tokens pass through, stopwords still apply).
    // Armenian and Tamil round out Qdrant's 19 Snowball languages as
    // explicit-stemmer-only (no Language variant, no stopword lists —
    // reachable via Stemmer::parse, like Qdrant's explicit config).
    let stemmed = [
        "arabic",
        "danish",
        "dutch",
        "english",
        "finnish",
        "french",
        "german",
        "greek",
        "hungarian",
        "italian",
        "norwegian",
        "portuguese",
        "romanian",
        "russian",
        "spanish",
        "swedish",
        "turkish",
    ];
    for name in stemmed {
        assert!(
            Language::parse(name).unwrap().stem_algorithm().is_some(),
            "{name} must have a stemmer"
        );
    }
    let unstemmed = [
        "azerbaijani",
        "basque",
        "bengali",
        "catalan",
        "chinese",
        "hebrew",
        "hinglish",
        "indonesian",
        "japanese",
        "kazakh",
        "nepali",
        "slovene",
        "tajik",
    ];
    for name in unstemmed {
        assert!(
            Language::parse(name).unwrap().stem_algorithm().is_none(),
            "{name} must have no default stemmer"
        );
    }
    assert_eq!(
        stemmed.len() + unstemmed.len(),
        30,
        "every language must be classified"
    );

    // Explicit-only Snowball languages (Qdrant's SnowballLanguage set minus
    // the 17 language-reachable ones): parseable as stemmers, not languages.
    for (name, stemmer) in [
        ("armenian", Stemmer::Armenian),
        ("hy", Stemmer::Armenian),
        ("tamil", Stemmer::Tamil),
        ("ta", Stemmer::Tamil),
    ] {
        assert_eq!(Stemmer::parse(name).expect("explicit stemmer"), stemmer);
    }
    assert!(Language::parse("armenian").is_err());
    assert!(Language::parse("tamil").is_err());
}

#[test]
fn tokenizer_parse_round_trips() {
    for (name, tokenizer) in [
        ("word", Tokenizer::Word),
        ("whitespace", Tokenizer::Whitespace),
        ("prefix", Tokenizer::Prefix),
        ("multilingual", Tokenizer::Multilingual),
    ] {
        assert_eq!(Tokenizer::parse(name).unwrap(), tokenizer);
        assert_eq!(tokenizer.name(), name);
    }
    let err = Tokenizer::parse("ngram").expect_err("unknown tokenizer rejects");
    assert_eq!(err.code, "QQL-VALIDATION-CONFIG");
}

#[test]
fn spanish_pipeline_stems_and_filters() {
    let pipe = plain("spanish");
    // "la" is a Spanish stopword; "casa" stems to "cas".
    assert_eq!(pipe.doc_tokens("la casa").unwrap(), vec!["cas"]);
    // Same text through English keeps different content ("la" is not English).
    let english = plain("english");
    assert_ne!(
        pipe.embed_document("la casa").unwrap().indices,
        english.embed_document("la casa").unwrap().indices
    );
}

#[test]
fn disabled_stemmer_keeps_inflections_distinct() {
    // Mirrors Qdrant's own `disabled_stemmer_keeps_inflections_distinct`.
    let stemmed = plain("english");
    assert_eq!(
        stemmed.embed_document("running run").unwrap().indices.len(),
        1
    );
    let config = Bm25TextConfig::resolve(
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(Vec::new()),
        Some("none"),
        None,
        None,
        None,
    )
    .expect("valid");
    let unstemmed = config.pipeline();
    assert_eq!(
        unstemmed
            .embed_document("running run")
            .unwrap()
            .indices
            .len(),
        2
    );
}

#[test]
fn custom_stopwords_replace_the_default() {
    // `Some(vec![])` disables filtering: "the" survives.
    let config = Bm25TextConfig::resolve(
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(Vec::new()),
        None,
        None,
        None,
        None,
    )
    .expect("valid");
    let tokens = config.pipeline().doc_tokens("the cat").unwrap();
    assert!(tokens.contains(&"the".to_string()), "got {tokens:?}");
    // A custom list filters exactly its words (post-normalization).
    let config = Bm25TextConfig::resolve(
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(vec!["cat".to_string()]),
        Some("none"),
        None,
        None,
        None,
    )
    .expect("valid");
    assert_eq!(
        config.pipeline().doc_tokens("the cat").unwrap(),
        vec!["the"]
    );
}

#[test]
fn ascii_folding_normalizes_before_lowercase() {
    let folded = Bm25TextConfig::resolve(
        None,
        None,
        None,
        None,
        None,
        None,
        Some(true),
        None,
        Some("none"),
        None,
        None,
        None,
    )
    .expect("valid")
    .pipeline();
    let plain_pipe = Bm25TextConfig::resolve(
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some("none"),
        None,
        None,
        None,
    )
    .expect("valid")
    .pipeline();
    assert_eq!(folded.doc_tokens("café").unwrap(), vec!["cafe"]);
    assert_eq!(
        folded.embed_query("café").unwrap().indices,
        folded.embed_query("cafe").unwrap().indices
    );
    assert_ne!(
        plain_pipe.embed_query("café").unwrap().indices,
        plain_pipe.embed_query("cafe").unwrap().indices
    );
}

#[test]
fn lowercase_off_keeps_case_distinct() {
    let cased = Bm25TextConfig::resolve(
        None,
        None,
        None,
        None,
        None,
        Some(false),
        None,
        None,
        Some("none"),
        None,
        None,
        None,
    )
    .expect("valid")
    .pipeline();
    assert_ne!(
        cased.embed_query("Hello").unwrap().indices,
        cased.embed_query("hello").unwrap().indices
    );
    let lowered = plain("english");
    assert_eq!(
        lowered.embed_query("Hello").unwrap().indices,
        lowered.embed_query("hello").unwrap().indices
    );
}

#[test]
fn token_length_limits_apply_in_chars() {
    let config = Bm25TextConfig::resolve(
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some("none"),
        Some(4),
        Some(4),
        None,
    )
    .expect("valid");
    assert_eq!(
        config.pipeline().doc_tokens("a bb ccc dddd eeeee").unwrap(),
        vec!["dddd"]
    );
}

#[test]
fn whitespace_tokenizer_keeps_hyphenated_forms() {
    let config = Bm25TextConfig::resolve(
        None,
        None,
        None,
        None,
        Some("whitespace"),
        None,
        None,
        None,
        Some("none"),
        None,
        None,
        None,
    )
    .expect("valid");
    assert_eq!(
        config.pipeline().doc_tokens("hello-world").unwrap(),
        vec!["hello-world"]
    );
    // Word splitting breaks it apart (modulo stopwords/stemming on pieces).
    assert!(
        plain("english")
            .doc_tokens("hello-world")
            .unwrap()
            .iter()
            .all(|t| t != "hello-world")
    );
}

#[test]
fn prefix_tokenizer_expands_docs_and_truncates_queries() {
    let config = Bm25TextConfig::resolve(
        None,
        None,
        None,
        None,
        Some("prefix"),
        None,
        None,
        None,
        Some("none"),
        Some(2),
        None,
        None,
    )
    .expect("valid");
    let pipe = config.pipeline();
    assert_eq!(
        pipe.doc_tokens("hello").unwrap(),
        vec!["he", "hel", "hell", "hello"]
    );
    // Short words still emit once (full word + break).
    assert_eq!(pipe.doc_tokens("hi").unwrap(), vec!["hi"]);
    // Query keeps the longest n-gram only.
    let mut query_tokens = Vec::new();
    pipe.for_each_query("hello", |t: &str| query_tokens.push(t.to_string()))
        .expect("query tokens");
    assert_eq!(query_tokens, vec!["hello"]);
    // With a max cap the query truncates.
    let capped = Bm25TextConfig::resolve(
        None,
        None,
        None,
        None,
        Some("prefix"),
        None,
        None,
        None,
        Some("none"),
        Some(2),
        Some(3),
        None,
    )
    .expect("valid")
    .pipeline();
    assert_eq!(capped.doc_tokens("hello").unwrap(), vec!["he", "hel"]);
    let mut capped_query = Vec::new();
    capped
        .for_each_query("hello", |t: &str| capped_query.push(t.to_string()))
        .expect("query tokens");
    assert_eq!(capped_query, vec!["hel"]);
}

#[test]
fn multilingual_fails_closed() {
    let pipe = Bm25TextConfig::resolve(
        None,
        None,
        None,
        None,
        Some("multilingual"),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .expect("multilingual parses")
    .pipeline();
    for result in [
        pipe.embed_query("hello"),
        pipe.embed_document("hello"),
        pipe.token_count("hello")
            .map(|_| crate::sparse::SparseVector::default()),
    ] {
        let err = result.expect_err("multilingual must fail closed");
        assert_eq!(err.code, "QQL-VALIDATION-CONFIG");
    }
}

#[test]
fn k1_zero_gives_binary_weighting() {
    let params = sparse::Bm25Params::new(0.0, 0.75, 256.0).expect("k1 = 0 valid");
    let vector = sparse::embed_document_with_params("cat sat mat cat", &params);
    assert!(!vector.indices.is_empty());
    assert!(
        vector.values.iter().all(|&v| v == 1.0),
        "k1 = 0 must saturate every term to 1.0, got {:?}",
        vector.values
    );
}

#[test]
fn tf_formula_matches_qdrant_reference() {
    // Qdrant `lib/bm25` reference: 5 tokens, "the" 2x; k1=1.2, b=0.75,
    // avg_len=5 → tf = 2*2.2 / (1.2*(0.25+0.75*1) + 2) = 1.375. Whitespace
    // tokens with processing disabled reproduce their exact input.
    let config = Bm25TextConfig {
        params: sparse::Bm25Params::new(1.2, 0.75, 5.0).expect("valid"),
        tokenizer: Tokenizer::Whitespace,
        stopwords: Some(crate::bm25_text::Stopwords::default()),
        stemmer: Some(Stemmer::Disabled),
        ..Bm25TextConfig::default()
    };
    let vector = config
        .pipeline()
        .embed_document("the cat sat on the")
        .expect("embed");
    let id = sparse::token_id("the");
    let value = vector
        .indices
        .iter()
        .zip(&vector.values)
        .find(|(i, _)| **i == id)
        .map(|(_, v)| *v)
        .expect("token 'the' should appear");
    assert!((value - 1.375).abs() < 1e-6, "got {value}, want 1.375");
}

#[test]
fn estimator_measures_post_pipeline_lengths() {
    let pipe = plain("english");
    // "the" is filtered: (2 + 1) / 2 = 1.5.
    let estimate =
        crate::bm25_text::estimate_avg_len(["the cat sat", "dogs"], &pipe).expect("estimate");
    assert_eq!(estimate, Some(AvgLenEstimate { mean: 1.5, docs: 2 }));
    assert_eq!(
        crate::bm25_text::estimate_avg_len(Vec::<&str>::new(), &pipe).expect("estimate"),
        None
    );
    assert_eq!(
        crate::bm25_text::estimate_avg_len(["the a"], &pipe).expect("estimate"),
        None,
        "all-filtered sample has no meaningful average"
    );
}

#[test]
fn resolve_rejects_bad_names() {
    for (language, tokenizer, stemmer) in [
        (Some("klingon"), None, None),
        (None, Some("ngram"), None),
        (None, None, Some("yoda")),
    ] {
        let err = Bm25TextConfig::resolve(
            None, None, None, language, tokenizer, None, None, None, stemmer, None, None, None,
        )
        .expect_err("bad option must fail closed");
        assert_eq!(err.code, "QQL-VALIDATION-CONFIG");
    }
    // Explicit stemmer override wins over the language default.
    let config = Bm25TextConfig::resolve(
        None,
        None,
        None,
        Some("spanish"),
        None,
        None,
        None,
        None,
        Some("none"),
        None,
        None,
        None,
    )
    .expect("valid");
    assert_eq!(config.stemmer, Some(Stemmer::Disabled));
    let config = Bm25TextConfig::resolve(
        None,
        None,
        None,
        Some("spanish"),
        None,
        None,
        None,
        None,
        Some("french"),
        None,
        None,
        None,
    )
    .expect("valid");
    assert_eq!(config.stemmer, Some(Stemmer::Snowball(Language::French)));
}

/// Vendored stopword lists keep Qdrant's exact entries, including the
/// easily "normalized away" ones: hinglish carries both apostrophe forms
/// (`aint` and `ain't`); tajik keeps trailing-space and hyphenated forms
/// verbatim (dead or live exactly as upstream); arabic keeps its diacritics.
/// Guards the `scripts/gen_bm25_stopwords.py` port against well-meaning
/// cleanup. See `bm25_stopwords.rs` header for re-sync.
#[test]
fn vendored_stopwords_keep_upstream_entries_verbatim() {
    use crate::bm25_stopwords::stopwords_for;

    let hinglish = stopwords_for(Language::Hinglish);
    for word in ["aint", "ain't", "well", "we'll", "were", "we're"] {
        assert!(hinglish.contains(word), "hinglish must keep {word:?}");
    }
    let tajik = stopwords_for(Language::Tajik);
    for word in ["агар ", "аз-баски ", "чун-ки", "то даме ки", "то даме ки "]
    {
        assert!(tajik.contains(word), "tajik must keep {word:?} verbatim");
    }
    let arabic = stopwords_for(Language::Arabic);
    for word in ["ّأيّان", "لا سيما"] {
        assert!(arabic.contains(word), "arabic must keep {word:?} verbatim");
    }
    let kazakh = stopwords_for(Language::Kazakh);
    assert!(kazakh.contains("әттеген-ай"));
    let hungarian = stopwords_for(Language::Hungarian);
    assert!(hungarian.contains("ill"));
}

/// Incremental updates keep untouched knobs (the WASM `setBm25Text`
/// contract): setting the tokenizer must not reset the language.
#[test]
fn with_text_options_keeps_untouched_knobs() {
    let spanish = Bm25TextConfig::resolve(
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
    .expect("valid");
    let updated = spanish
        .with_text_options(
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
        .expect("valid update");
    assert_eq!(updated.language, Language::Spanish);
    assert_eq!(updated.tokenizer, Tokenizer::Whitespace);
    assert_eq!(updated.params, spanish.params);
    // Empty names behave like None (JS empty-string convention).
    let same = spanish
        .with_text_options(Some(""), Some(""), None, None, None, None, None, None, None)
        .expect("valid update");
    assert_eq!(same, spanish);
    // Invalid names still fail closed on the update path.
    let err = spanish
        .with_text_options(
            Some("klingon"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect_err("bad language rejects");
    assert_eq!(err.code, "QQL-VALIDATION-CONFIG");
}

/// Additional language lists merge with custom words (Qdrant `Set`
/// semantics): an explicit selection replaces the default.
#[test]
fn stopwords_languages_merge() {
    // French + custom: "le" (french) and "zzz" (custom) filter; english
    // "the" survives because the default list is replaced, not merged.
    let config = Bm25TextConfig::resolve(
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(vec!["french".to_string()]),
    )
    .expect("valid");
    let tokens = config
        .pipeline()
        .doc_tokens("le the zzz abc")
        .expect("tokens");
    assert_eq!(tokens, vec!["the", "zzz", "abc"], "got {tokens:?}");

    // Custom words ride alongside merged languages.
    let config = Bm25TextConfig::resolve(
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(vec!["abc".to_string()]),
        None,
        None,
        None,
        Some(vec!["french".to_string()]),
    )
    .expect("valid");
    let tokens = config.pipeline().doc_tokens("le abc").expect("tokens");
    assert!(tokens.is_empty(), "got {tokens:?}");

    // Unknown language names fail closed.
    let err = Bm25TextConfig::resolve(
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(vec!["klingon".to_string()]),
    )
    .expect_err("bad stopwords language rejects");
    assert_eq!(err.code, "QQL-VALIDATION-CONFIG");
}
