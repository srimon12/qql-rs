use qql::backend::{
    CollectionInfo, CollectionParamsSpec, CollectionSchema, PayloadIndexSpec, VectorSpec,
};
use qql_plan::semantic::PlanPointId;
use serde_json::json;

use super::checkpoint::{self, Checkpoint, Phase};
use super::options::{
    DEFAULT_BULK_INDEXING_THRESHOLD, DEFAULT_INDEXING_THRESHOLD, MigrateOptions, MissingShardKey,
    QuantizeKind, QuantizeSpec,
};
use super::pipeline::split_by_shard;
use super::schema::{
    apply_overrides, build_plan, parse_where_filter, quantize_json, restore_optimizers_statement,
    suppress_indexing,
};
use crate::dump::generate_create_statement;

fn sample_info() -> CollectionInfo {
    CollectionInfo {
        status: "green".into(),
        points_count: 10,
        segments_count: 1,
        schema: CollectionSchema {
            dense_vectors: vec!["dense".into()],
            sparse_vectors: Vec::new(),
            vectors: vec![VectorSpec {
                name: Some("dense".into()),
                size: 4,
                distance: "Cosine".into(),
                hnsw: None,
                quantization: None,
                multivector: None,
                on_disk: None,
                datatype: None,
                memory: None,
            }],
            payload_indexes: vec![PayloadIndexSpec {
                field: "city".into(),
                data_type: "keyword".into(),
                params: serde_json::Map::new(),
                is_tenant: None,
            }],
            params: CollectionParamsSpec {
                shard_number: Some(2),
                sharding_method: None,
                on_disk_payload: None,
                payload_memory: None,
                replication_factor: Some(1),
            },
            hnsw: None,
            optimizers: None,
            quantization: None,
        },
    }
}

fn base_opts() -> MigrateOptions {
    MigrateOptions {
        source_collection: "docs".into(),
        target_collection: "docs_v2".into(),
        source_url: "http://localhost:6333".into(),
        target_url: "http://localhost:6333".into(),
        batch_size: 128,
        workers: 2,
        shard_number: None,
        replication_factor: None,
        sharding_method: None,
        quantize: None,
        shard_key: None,
        shard_key_field: None,
        missing_shard_key: MissingShardKey::Error,
        bulk_indexing_threshold: DEFAULT_BULK_INDEXING_THRESHOLD,
        cutover_alias: None,
        drop_source_after_cutover: false,
        where_clause: None,
        checkpoint_path: ".qql-migrate/docs__docs_v2.json".into(),
        resume: false,
        restart: false,
        dry_run: false,
        fast_bulk: true,
        verify: true,
        wait: true,
        recreate: false,
    }
}

#[test]
fn overrides_reshard_and_custom_sharding() {
    let mut info = sample_info();
    let mut opts = base_opts();
    opts.shard_number = Some(12);
    opts.replication_factor = Some(2);
    opts.shard_key_field = Some("tenant_id".into());
    apply_overrides(&mut info, &opts);
    assert_eq!(info.schema.params.shard_number, Some(12));
    assert_eq!(info.schema.params.replication_factor, Some(2));
    assert_eq!(
        info.schema.params.sharding_method.as_deref(),
        Some("custom")
    );
}

#[test]
fn fast_bulk_plan_suppresses_then_restores_indexing() {
    let info = sample_info();
    let opts = base_opts();
    let plan = build_plan(&info, &opts);
    qql_core::parser::Parser::parse(&format!("{};", plan.create)).expect("create parses");
    assert!(
        plan.create.contains(&format!(
            "indexing_threshold = {DEFAULT_BULK_INDEXING_THRESHOLD}"
        )),
        "bulk-load CREATE should suppress HNSW: {}",
        plan.create
    );
    assert_eq!(plan.indexes.len(), 1);
    qql_core::parser::Parser::parse(&format!("{};", plan.indexes[0])).expect("index parses");
    let restore = plan.restore_optimizers.expect("restore ALTER");
    assert!(restore.contains(&format!(
        "indexing_threshold = {DEFAULT_INDEXING_THRESHOLD}"
    )));
    qql_core::parser::Parser::parse(&format!("{};", restore)).expect("alter parses");
}

#[test]
fn scalar_quantize_create_parses() {
    let mut info = sample_info();
    let spec = QuantizeSpec::new(QuantizeKind::Scalar);
    info.schema.quantization = Some(quantize_json(&spec));
    let stmt = generate_create_statement("docs", &info);
    assert!(stmt.contains("WITH QUANTIZATION ("));
    assert!(stmt.contains("type = 'scalar'"));
    qql_core::parser::Parser::parse(&format!("{};", stmt)).expect("scalar quantize CREATE");
}

#[test]
fn binary_and_turbo_quantize_create_parse() {
    for kind in [
        QuantizeKind::Binary,
        QuantizeKind::Turbo,
        QuantizeKind::Product,
    ] {
        let mut info = sample_info();
        info.schema.quantization = Some(quantize_json(&QuantizeSpec::new(kind)));
        let stmt = generate_create_statement("docs", &info);
        assert!(
            stmt.contains(&format!("type = '{}'", kind.as_str())),
            "{stmt}"
        );
        qql_core::parser::Parser::parse(&format!("{};", stmt))
            .unwrap_or_else(|e| panic!("{kind:?} CREATE should parse: {e}; {stmt}"));
    }
}

#[test]
fn suppress_indexing_preserves_original() {
    let mut info = sample_info();
    let mut map = serde_json::Map::new();
    map.insert("indexing_threshold".into(), json!(20000));
    info.schema.optimizers = Some(map);
    let original = suppress_indexing(&mut info, DEFAULT_BULK_INDEXING_THRESHOLD);
    assert_eq!(original, Some(20000));
    assert_eq!(
        info.schema.optimizers.as_ref().unwrap()["indexing_threshold"],
        json!(DEFAULT_BULK_INDEXING_THRESHOLD)
    );
    let restore = restore_optimizers_statement("docs", original);
    assert!(restore.contains("indexing_threshold = 20000"));
}

#[test]
fn parse_where_accepts_bare_predicate() {
    let filter = parse_where_filter("docs", "city = 'berlin'").expect("parse");
    let encoded = serde_json::to_value(&filter).expect("json");
    let text = encoded.to_string();
    assert!(text.contains("berlin"), "{text}");
}

#[test]
fn parse_where_strips_where_keyword() {
    parse_where_filter("docs", "WHERE city = 'berlin' AND price < 100").expect("parse");
}

#[test]
fn checkpoint_roundtrip_and_fingerprint() {
    let dir = std::env::temp_dir().join(format!("qql-migrate-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cp.json");
    let opts = base_opts();
    let mut cp = Checkpoint::new(&opts, 42);
    cp.phase = Phase::Ingest;
    cp.cursor = Some(PlanPointId::Number(99));
    cp.written = 128;
    checkpoint::save(path.to_str().unwrap(), &cp).unwrap();
    let loaded = checkpoint::load(path.to_str().unwrap())
        .unwrap()
        .expect("loaded");
    assert_eq!(loaded, cp);
    checkpoint::compatible(&loaded, &opts).unwrap();

    let mut other = opts.clone();
    other.shard_number = Some(12);
    let err = checkpoint::compatible(&loaded, &other).unwrap_err();
    assert!(err.to_string().contains("do not match"));

    checkpoint::remove(path.to_str().unwrap()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn default_checkpoint_path_includes_cluster_identity() {
    let path = checkpoint::default_path("http://old:6333", "docs", "http://new:6334", "docs");
    assert_eq!(
        path,
        ".qql-migrate/http___old_6333__docs__http___new_6334__docs.json"
    );
    let other = checkpoint::default_path("http://staging:6333", "docs", "http://prod:6334", "docs");
    assert_ne!(path, other);
}

#[test]
fn split_by_fixed_shard_key() {
    let mut opts = base_opts();
    opts.shard_key = Some("acme".into());
    let records = vec![
        json!({"id": 1, "tenant_id": "x"}),
        json!({"id": 2, "tenant_id": "y"}),
    ];
    let (groups, skipped) = split_by_shard(records, &opts).unwrap();
    assert_eq!(skipped, 0);
    assert_eq!(groups.len(), 1);
    assert_eq!(
        groups[0].shard_key.as_ref().and_then(|k| k.as_keyword()),
        Some("acme")
    );
    assert_eq!(groups[0].rows.len(), 2);
}

#[test]
fn split_by_payload_field() {
    let mut opts = base_opts();
    opts.shard_key_field = Some("tenant_id".into());
    let records = vec![
        json!({"id": 1, "tenant_id": "acme"}),
        json!({"id": 2, "tenant_id": "globex"}),
        json!({"id": 3, "tenant_id": "acme"}),
    ];
    let (mut groups, skipped) = split_by_shard(records, &opts).unwrap();
    assert_eq!(skipped, 0);
    groups.sort_by(|a, b| a.shard_key.cmp(&b.shard_key));
    assert_eq!(groups.len(), 2);
    assert_eq!(
        groups[0].shard_key.as_ref().and_then(|k| k.as_keyword()),
        Some("acme")
    );
    assert_eq!(groups[0].rows.len(), 2);
    assert_eq!(
        groups[1].shard_key.as_ref().and_then(|k| k.as_keyword()),
        Some("globex")
    );
}

#[test]
fn split_by_payload_field_rejects_missing() {
    let mut opts = base_opts();
    opts.shard_key_field = Some("tenant_id".into());
    let err = split_by_shard(vec![json!({"id": 1})], &opts).unwrap_err();
    assert!(err.to_string().contains("missing"));
}

#[test]
fn fingerprint_ignores_workers_and_batch() {
    let a = base_opts();
    let mut b = base_opts();
    b.workers = 8;
    b.batch_size = 64;
    assert_eq!(a.fingerprint(), b.fingerprint());
    b.quantize = Some(QuantizeSpec::new(QuantizeKind::Scalar));
    assert_ne!(a.fingerprint(), b.fingerprint());
}

#[test]
fn numeric_payload_stays_numeric_shard_key() {
    let mut opts = base_opts();
    opts.shard_key_field = Some("tenant_id".into());
    let records = vec![json!({"id": 1, "tenant_id": 101})];
    let (groups, _) = split_by_shard(records, &opts).unwrap();
    assert!(matches!(
        groups[0].shard_key,
        Some(qql_core::ast::ShardKey::Number(101))
    ));
}

#[test]
fn missing_shard_key_skip_does_not_abort() {
    let mut opts = base_opts();
    opts.shard_key_field = Some("tenant_id".into());
    opts.missing_shard_key = MissingShardKey::Skip;
    let (groups, skipped) = split_by_shard(vec![json!({"id": 1})], &opts).unwrap();
    assert_eq!(skipped, 1);
    assert!(groups.is_empty());
}

#[test]
fn window_totals_sum_every_page() {
    let (written, skipped, batches) =
        super::pipeline::window_totals_for_test(&[128, 128, 64], &[0, 1, 0]);
    assert_eq!(written, 320);
    assert_eq!(skipped, 1);
    assert_eq!(batches, 3);
}

#[test]
fn tenant_index_is_promoted_in_plan() {
    let info = sample_info();
    let mut opts = base_opts();
    opts.shard_key_field = Some("tenant_id".into());
    let plan = build_plan(&info, &opts);
    assert!(
        plan.indexes
            .iter()
            .any(|s| s.contains("tenant_id") && s.contains("is_tenant = true")),
        "{:?}",
        plan.indexes
    );
}

#[test]
fn numeric_shard_upsert_parses() {
    qql_core::parser::Parser::parse("UPSERT INTO docs VALUES :rows SHARD 101 WAIT true;")
        .expect("numeric SHARD on UPSERT");
    qql_core::parser::Parser::parse("CREATE SHARD KEY 101 ON COLLECTION docs;")
        .expect("numeric CREATE SHARD KEY");
}

#[test]
fn missing_policy_parse() {
    assert_eq!(
        MissingShardKey::parse("skip").unwrap(),
        MissingShardKey::Skip
    );
    assert!(matches!(
        MissingShardKey::parse("default=101").unwrap(),
        MissingShardKey::Default(qql_core::ast::ShardKey::Number(101))
    ));
}

#[test]
fn standalone_create_shard_key_is_unsupported_not_exists() {
    assert!(super::schema::shard_key_backend_unsupported(
        "Operation is not implemented or not supported: Qdrant is running in standalone mode"
    ));
    assert!(super::schema::shard_key_backend_unsupported(
        "Qdrant is running in standalone mode"
    ));
    assert!(!super::schema::shard_key_backend_unsupported(
        "shard key 'acme' already exists"
    ));
    let mapped = super::schema::map_shard_key_error(
        "gRPC create_shard_key: code: 'Operation is not implemented or not supported'",
    );
    assert!(mapped.contains("distributed mode"), "{mapped}");
}

#[test]
fn custom_sharding_probe_sql_parses() {
    qql_core::parser::Parser::parse("CREATE SHARD KEY '__qql_migrate_probe__' ON COLLECTION docs;")
        .expect("probe CREATE SHARD KEY");
    qql_core::parser::Parser::parse("DROP SHARD KEY '__qql_migrate_probe__' ON COLLECTION docs;")
        .expect("probe DROP SHARD KEY");
}
