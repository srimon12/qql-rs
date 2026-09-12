use super::*;
use qql::executor::{Executor, OnError};
use qql_core::parser::Parser;
use qql_plan::{
    PlanFacetValue, PlanGroupId, PlanPointId, PlanVectorStruct, PlanVectorValue, PlannedOperation,
    plan,
};
use serde_json::json;

fn plan_one(qql: &str) -> PlannedOperation {
    let statement = Parser::parse(qql).unwrap_or_else(|error| panic!("parse '{qql}': {error}"));
    plan(&statement).unwrap_or_else(|error| panic!("plan '{qql}': {error}"))
}

fn temp_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("qql-edge-{tag}-{}", std::process::id()))
}

async fn seed_docs(backend: &EdgeQdrant) {
    backend
        .execute_planned(&plan_one(
            "CREATE COLLECTION docs (dense VECTOR(3, COSINE))",
        ))
        .await
        .expect("create collection");
    backend
        .execute_planned(&plan_one(
            "CREATE INDEX ON COLLECTION docs FOR city TYPE keyword",
        ))
        .await
        .expect("create facet index");
    for qql in [
        "UPSERT INTO docs VALUES {id: 1, vector: {dense: [1.0, 0.0, 0.0]}, city: 'NYC', price: 10}",
        "UPSERT INTO docs VALUES {id: 2, vector: {dense: [0.0, 1.0, 0.0]}, city: 'NYC', price: 30}",
        "UPSERT INTO docs VALUES {id: 3, vector: {dense: [0.0, 0.0, 1.0]}, city: 'SF', price: 20}",
    ] {
        backend
            .execute_planned(&plan_one(qql))
            .await
            .unwrap_or_else(|error| panic!("seed '{qql}': {error}"));
    }
}

/// `execute_planned` answers every read with typed `ExecData` built
/// straight from qdrant-edge results (no JSON envelope, no telemetry).
#[test]
fn typed_reads_and_facet_return_exec_data() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
            let dir = temp_dir("typed-reads");
            let _ = std::fs::remove_dir_all(&dir);
            let backend = EdgeQdrant::new(&dir, false);
            seed_docs(&backend).await;

            // Query → Hits with score, payload, and vector mapped directly.
            let query = plan_one(
                "QUERY [1.0, 0.0, 0.0] FROM docs USING dense WITH PAYLOAD true WITH VECTOR true LIMIT 5",
            );
            let response = backend.execute_planned(&query).await.expect("typed query");
            assert!(
                response.telemetry.is_none(),
                "in-process edge has no telemetry"
            );
            let ExecData::Hits(hits) = &response.data else {
                panic!("expected typed hits, got {:?}", response.data);
            };
            assert_eq!(hits.len(), 3);
            assert_eq!(hits[0].id, PlanPointId::Number(1));
            assert!((hits[0].score - 1.0).abs() < 1e-6);
            assert_eq!(
                hits[0].payload.as_ref().and_then(|p| p.get("city")),
                Some(&json!("NYC"))
            );
            assert_eq!(
                hits[0].vector,
                Some(PlanVectorStruct::Named(std::collections::BTreeMap::from(
                    [(
                        "dense".to_string(),
                        PlanVectorValue::Dense(vec![1.0, 0.0, 0.0])
                    )]
                ))),
                "WITH VECTOR true must map the vector"
            );
            assert!(hits[0].collection.is_none());

            // GetPoints → Hits with score 0.0 (no similarity search).
            let points = plan_one("QUERY POINTS (1, 2) FROM docs WITH PAYLOAD true");
            let response = backend
                .execute_planned(&points)
                .await
                .expect("typed points");
            let ExecData::Hits(hits) = &response.data else {
                panic!("expected typed hits, got {:?}", response.data);
            };
            assert_eq!(hits.len(), 2);
            assert!(hits.iter().all(|hit| hit.score == 0.0));

            // Scroll → Hits; `next_page_offset` is intentionally not carried.
            let scroll = plan_one("SCROLL FROM docs LIMIT 2");
            let response = backend
                .execute_planned(&scroll)
                .await
                .expect("typed scroll");
            let ExecData::Hits(hits) = &response.data else {
                panic!("expected typed hits, got {:?}", response.data);
            };
            assert_eq!(hits.len(), 2);
            assert!(hits.iter().all(|hit| hit.score == 0.0));

            // Count → Count.
            let count = plan_one("COUNT FROM docs WHERE city = 'NYC'");
            let response = backend
                .execute_planned(&count)
                .await
                .expect("typed count");
            assert_eq!(response.data, ExecData::Count(2));

            // Facet → Facet (previously rejected as unsupported offline).
            let facet = plan_one("FACET city FROM docs");
            let response = backend
                .execute_planned(&facet)
                .await
                .expect("typed facet");
            assert_eq!(
                response.data,
                ExecData::Facet(vec![
                    FacetHit {
                        value: PlanFacetValue::Keyword("NYC".into()),
                        count: 2,
                    },
                    FacetHit {
                        value: PlanFacetValue::Keyword("SF".into()),
                        count: 1,
                    },
                ])
            );

            // Mutations answer typed, status-only data too.
            let upsert = plan_one(
                "UPSERT INTO docs VALUES {id: 4, vector: {dense: [0.5, 0.5, 0.5]}}",
            );
            let response = backend
                .execute_planned(&upsert)
                .await
                .expect("typed upsert");
            assert_eq!(response.data, ExecData::Mutation { affected: None });

            backend.close().await.expect("close edge backend");
            let _ = std::fs::remove_dir_all(dir);
        });
}

/// Every write and DDL op answers `execute_planned` with a status-only
/// typed `ExecData::Mutation` — no JSON envelope, no `Raw`.
#[test]
fn typed_mutations_and_ddl_return_exec_data() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let dir = temp_dir("typed-mutations");
        let _ = std::fs::remove_dir_all(&dir);
        let backend = EdgeQdrant::new(&dir, false);
        backend
            .execute_planned(&plan_one(
                "CREATE COLLECTION docs (dense VECTOR(3, COSINE))",
            ))
            .await
            .expect("create collection");

        let cases = [
            "UPSERT INTO docs VALUES {id: 1, vector: {dense: [1.0, 0.0, 0.0]}, city: 'NYC'}",
            "UPDATE docs SET PAYLOAD = {price: 10} WHERE id = 1",
            "UPDATE docs SET VECTOR dense = [0.0, 1.0, 0.0] WHERE id = 1",
            "DELETE VECTOR dense FROM docs WHERE id = 1",
            "DELETE PAYLOAD city FROM docs WHERE id = 1",
            "CLEAR PAYLOAD FROM docs WHERE id = 1",
            "DELETE FROM docs WHERE id = 1",
            "CREATE INDEX ON COLLECTION docs FOR city TYPE keyword",
            "DROP INDEX ON COLLECTION docs FOR city",
        ];
        for qql in cases {
            let response = backend
                .execute_planned(&plan_one(qql))
                .await
                .unwrap_or_else(|error| panic!("{qql}: {error}"));
            assert_eq!(
                response.data,
                ExecData::Mutation { affected: None },
                "{qql} must answer status-only typed mutation data"
            );
            assert!(response.telemetry.is_none());
        }

        let response = backend
            .execute_planned(&plan_one("DROP COLLECTION docs"))
            .await
            .expect("drop collection");
        assert_eq!(response.data, ExecData::Mutation { affected: None });

        backend.close().await.expect("close edge backend");
        let _ = std::fs::remove_dir_all(dir);
    });
}

/// `execute_update_batch` answers typed status-only mutations directly —
/// no JSON, no envelope parsing — one response per operation, in order.
#[test]
fn typed_update_batch_returns_mutations() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let dir = temp_dir("typed-update-batch");
        let _ = std::fs::remove_dir_all(&dir);
        let backend = EdgeQdrant::new(&dir, false);

        let ops = vec![
            plan_one("CREATE COLLECTION docs (dense VECTOR(3, COSINE))"),
            plan_one("UPSERT INTO docs VALUES {id: 1, vector: {dense: [1.0, 0.0, 0.0]}}"),
        ];
        for op in &ops {
            backend.execute_planned(op).await.expect("setup op");
        }

        let mutations = vec![
            plan_one("UPSERT INTO docs VALUES {id: 2, vector: {dense: [0.0, 1.0, 0.0]}}"),
            plan_one("UPDATE docs SET PAYLOAD = {city: 'NYC'} WHERE id = 2"),
            plan_one("DELETE FROM docs WHERE id = 2"),
        ];
        let (collection, _labels, batch) =
            qql_plan::build_update_batch(&mutations).expect("update batch");
        assert_eq!(collection, "docs");

        let responses = backend
            .execute_update_batch(&collection, &batch, true)
            .await
            .expect("typed update batch");
        assert_eq!(responses.len(), mutations.len());
        for response in &responses {
            assert_eq!(response.data, ExecData::Mutation { affected: None });
            assert!(response.telemetry.is_none());
            // The executor owns upsert counts; the backend never reports one.
            assert!(response.data.count().is_none());
        }

        backend.close().await.expect("close edge backend");
        let _ = std::fs::remove_dir_all(dir);
    });
}

/// `SHOW` metadata answers through the typed variants: collection lists
/// via `Collections`, collection metadata via `Collection`.
#[test]
fn show_metadata_returns_typed_variants() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let dir = temp_dir("typed-metadata");
        let _ = std::fs::remove_dir_all(&dir);
        let backend = EdgeQdrant::new(&dir, false);
        backend
            .execute_planned(&plan_one(
                "CREATE COLLECTION docs (dense VECTOR(3, COSINE))",
            ))
            .await
            .expect("create collection");

        for qql in [
            "UPSERT INTO docs VALUES {id: 1, vector: {dense: [1.0, 0.0, 0.0]}}",
            "QUERY POINTS (1) FROM docs",
            "COUNT FROM docs",
            "DELETE FROM docs WHERE id = 1",
        ] {
            let response = backend
                .execute_planned(&plan_one(qql))
                .await
                .unwrap_or_else(|error| panic!("{qql}: {error}"));
            assert!(
                matches!(
                    response.data,
                    ExecData::Hits(_) | ExecData::Count(_) | ExecData::Mutation { .. }
                ),
                "{qql} must answer a typed variant: {:?}",
                response.data
            );
        }

        let collections = backend
            .execute_planned(&plan_one("SHOW COLLECTIONS"))
            .await
            .expect("show collections");
        assert_eq!(
            collections.data.collections(),
            Some(["docs".to_string()].as_slice())
        );

        let collection = backend
            .execute_planned(&plan_one("SHOW COLLECTION docs"))
            .await
            .expect("show collection");
        // The typed loop above deleted the only point.
        let info = collection.data.collection().expect("typed collection info");
        assert_eq!(info.points_count, 0);
        assert_eq!(info.schema.vectors.len(), 1);

        backend.close().await.expect("close edge backend");
        let _ = std::fs::remove_dir_all(dir);
    });
}

/// `SHOW COLLECTION` reports collection-level HNSW / optimizer /
/// quantization configs through the typed schema fields (no JSON maps).
#[test]
fn show_collection_reports_typed_config_blocks() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let dir = temp_dir("typed-config-blocks");
        let _ = std::fs::remove_dir_all(&dir);
        let backend = EdgeQdrant::new(&dir, false);
        backend
            .execute_planned(&plan_one(
                "CREATE COLLECTION docs (dense VECTOR(4, COSINE)) \
                       WITH HNSW (m = 16, memory = 'pinned') \
                       WITH OPTIMIZERS (indexing_threshold = 20000) \
                       WITH QUANTIZATION (type = 'scalar', quantile = 0.99, always_ram = true)",
            ))
            .await
            .expect("create collection with typed config blocks");

        let response = backend
            .execute_planned(&plan_one("SHOW COLLECTION docs"))
            .await
            .expect("show collection");
        let info = response.data.collection().expect("typed collection info");

        let hnsw = info.schema.hnsw.as_ref().expect("typed hnsw");
        assert_eq!(hnsw.m, Some(16));
        assert_eq!(
            hnsw.memory,
            Some(qql_core::ast::MemoryPlacement::Pinned),
            "engine memory placement survives the typed projection"
        );

        let optimizers = info.schema.optimizers.as_ref().expect("typed optimizers");
        assert_eq!(optimizers.indexing_threshold, Some(20_000));
        // The engine has no `max_optimization_threads`; optimizations are
        // manual, so the field stays unset.
        assert_eq!(optimizers.max_optimization_threads, None);

        match info
            .schema
            .quantization
            .as_ref()
            .expect("typed quantization")
        {
            qql_plan::QuantizationConfig::Scalar { scalar } => {
                assert_eq!(scalar.qtype, "int8");
                assert_eq!(
                    scalar.quantile,
                    Some(0.99),
                    "f32 quantile widens through its shortest decimal form"
                );
                assert_eq!(scalar.always_ram, Some(true));
            }
            other => panic!("expected scalar quantization, got {other:?}"),
        }

        backend.close().await.expect("close edge backend");
        let _ = std::fs::remove_dir_all(dir);
    });
}

/// `optimize_collection` runs the qdrant-edge optimizers. With 1000 points
/// and 25% deleted the vacuum optimizer fires; a second call is a no-op,
/// proving the returned flag and idempotence.
#[test]
fn optimize_collection_runs_optimizers_and_is_idempotent() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let dir = temp_dir("optimize");
        let _ = std::fs::remove_dir_all(&dir);
        let backend = EdgeQdrant::new(&dir, false);
        backend
            .execute_planned(&plan_one("CREATE COLLECTION docs (dense VECTOR(1, DOT))"))
            .await
            .expect("create collection");

        // qdrant-edge only vacuums shards with at least 1000 vectors.
        let points = (1..=1000)
            .map(|id| format!("{{id: {id}, vector: {{dense: [{id}.0]}}}}"))
            .collect::<Vec<_>>()
            .join(", ");
        backend
            .execute_planned(&plan_one(&format!("UPSERT INTO docs VALUES {points}")))
            .await
            .expect("bulk upsert");

        // Delete 250/1000 = 25%, above the default 20% vacuum threshold.
        let deleted = (1..=250).map(PlanPointId::Number).collect();
        backend
            .execute_planned(&PlannedOperation::Delete {
                collection: "docs".to_string(),
                request: qql_plan::types::DeleteRequest {
                    points: Some(deleted),
                    filter: None,
                    shard_key: None,
                },
                wait: false,
            })
            .await
            .expect("bulk delete");

        let optimized = backend.optimize_collection("docs").await.expect("optimize");
        assert!(
            optimized,
            "25% deletion on 1000 points must trigger the vacuum optimizer"
        );
        let optimized_again = backend
            .optimize_collection("docs")
            .await
            .expect("second optimize");
        assert!(
            !optimized_again,
            "a second optimize call must be a no-op (idempotent)"
        );

        let count = backend
            .execute_planned(&plan_one("COUNT FROM docs"))
            .await
            .expect("count after optimize");
        assert_eq!(count.data, ExecData::Count(750));

        backend.close().await.expect("close edge backend");
        let _ = std::fs::remove_dir_all(dir);
    });
}

/// `indexed_vectors_count` reaches the typed `CollectionInfo` and lags
/// `points_count` until `optimize()` builds the index — the contract the
/// CLI doctor/check readout and optimize command rely on.
#[test]
fn info_reports_indexed_vectors_until_optimized() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let dir = temp_dir("indexed-count");
        let _ = std::fs::remove_dir_all(&dir);
        let backend = EdgeQdrant::new(&dir, false);
        // 1 KB threshold so 300 four-dimensional points force indexing.
        backend
            .execute_planned(&plan_one(
                "CREATE COLLECTION docs (dense VECTOR(4, COSINE)) \
                     WITH OPTIMIZERS (indexing_threshold = 1)",
            ))
            .await
            .expect("create collection");

        let points = (1..=300)
            .map(|id| format!("{{id: {id}, vector: {{dense: [{id}.0, 0.0, 0.0, 0.0]}}}}"))
            .collect::<Vec<_>>()
            .join(", ");
        backend
            .execute_planned(&plan_one(&format!("UPSERT INTO docs VALUES {points}")))
            .await
            .expect("bulk upsert");

        let before = backend
            .get_collection_info("docs")
            .await
            .expect("collection info");
        assert_eq!(before.points_count, 300);
        assert_eq!(
            before.indexed_vectors_count,
            Some(0),
            "an unoptimized appendable segment has no vector index yet"
        );

        assert!(
            backend.optimize_collection("docs").await.expect("optimize"),
            "the indexing optimizer must fire above the threshold"
        );

        let after = backend
            .get_collection_info("docs")
            .await
            .expect("collection info after optimize");
        assert_eq!(after.points_count, 300);
        assert!(
            after.indexed_vectors_count.unwrap_or(0) > 0,
            "optimize must index vectors: {after:?}"
        );
        assert_eq!(
            after.indexed_vectors_count,
            Some(300),
            "a 1 KB threshold must index every vector of the segment: {after:?}"
        );

        backend.close().await.expect("close edge backend");
        let _ = std::fs::remove_dir_all(dir);
    });
}

/// Filter lowering is shared: a filtered COUNT answers through the typed
/// path with the direct (non-serde) lowering, proving the typed filter
/// converter is wired into query execution.
#[test]
fn typed_count_uses_direct_filter_lowering() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let dir = temp_dir("typed-filter");
        let _ = std::fs::remove_dir_all(&dir);
        let backend = EdgeQdrant::new(&dir, false);
        seed_docs(&backend).await;

        let count = plan_one("COUNT FROM docs WHERE price >= 20");
        let response = backend.execute_planned(&count).await.expect("typed count");
        assert_eq!(response.data, ExecData::Count(2));

        backend.close().await.expect("close edge backend");
        let _ = std::fs::remove_dir_all(dir);
    });
}

/// FACET used to be rejected offline. Driving the full executor proves
/// dispatch selects the typed edge path and FACET succeeds end to end.
#[test]
fn executor_facet_succeeds_end_to_end() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let dir = temp_dir("executor-facet");
        let _ = std::fs::remove_dir_all(&dir);
        let executor = Executor::new(Box::new(EdgeQdrant::new(&dir, false)), None);

        let report = executor
            .execute(
                "CREATE COLLECTION docs (dense VECTOR(3, COSINE))",
                OnError::Stop,
            )
            .await
            .expect("create collection");
        assert!(report.ok, "create failed: {report:?}");

        for qql in [
            "UPSERT INTO docs VALUES {id: 1, vector: {dense: [1.0, 0.0, 0.0]}, city: 'NYC'}",
            "UPSERT INTO docs VALUES {id: 2, vector: {dense: [0.0, 1.0, 0.0]}, city: 'NYC'}",
            "UPSERT INTO docs VALUES {id: 3, vector: {dense: [0.0, 0.0, 1.0]}, city: 'SF'}",
        ] {
            let report = executor.execute(qql, OnError::Stop).await.expect("upsert");
            assert!(report.ok, "upsert failed: {report:?}");
        }

        let report = executor
            .execute(
                "CREATE INDEX ON COLLECTION docs FOR city TYPE keyword",
                OnError::Stop,
            )
            .await
            .expect("create index");
        assert!(report.ok, "create index failed: {report:?}");

        let report = executor
            .execute("FACET city FROM docs LIMIT 10", OnError::Stop)
            .await
            .expect("facet run");
        assert!(report.ok, "FACET must succeed offline now: {report:?}");
        let response = report.first().expect("facet response");
        assert!(
            matches!(response.data.as_ref(), Some(ExecData::Facet(_))),
            "expected typed facet data, got {:?}",
            response.data
        );
        let entries = response.facet().expect("facet entries");
        assert_eq!(entries.len(), 2, "facet entries: {entries:?}");
        assert!(entries.contains(&(PlanFacetValue::Keyword("NYC".into()), 2)));
        assert!(entries.contains(&(PlanFacetValue::Keyword("SF".into()), 1)));

        executor.close().await.expect("close edge executor");
        let _ = std::fs::remove_dir_all(dir);
    });
}

/// A read against a nonexistent collection keeps the typed not-found code
/// (no ghost collection, no generic engine error).
#[test]
fn missing_collection_reports_typed_not_found() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let dir = temp_dir("missing-collection");
        let _ = std::fs::remove_dir_all(&dir);
        let backend = EdgeQdrant::new(&dir, false);

        let error = backend
            .execute_planned(&plan_one("SHOW COLLECTION never_created"))
            .await
            .expect_err("missing collection must fail");
        assert_eq!(error.code, "QQL-EDGE-COLLECTION-NOT-FOUND");
        assert_eq!(error.field("collection"), Some("never_created"));

        let _ = std::fs::remove_dir_all(dir);
    });
}

/// A wrong-dimension upsert is rejected by the engine and keeps its precise
/// category instead of collapsing into a generic library error.
#[test]
fn engine_dimension_mismatch_reports_dimension_code() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let dir = temp_dir("engine-dimension");
        let _ = std::fs::remove_dir_all(&dir);
        let backend = EdgeQdrant::new(&dir, false);
        backend
            .execute_planned(&plan_one(
                "CREATE COLLECTION docs (dense VECTOR(3, COSINE))",
            ))
            .await
            .expect("create collection");

        let error = backend
            .execute_planned(&plan_one(
                "UPSERT INTO docs VALUES {id: 1, vector: {dense: [1.0, 2.0]}}",
            ))
            .await
            .expect_err("dimension mismatch must fail");
        assert_eq!(error.code, "QQL-EDGE-DIMENSION");
        assert_eq!(error.field("operation"), Some("upsert"));
        assert_eq!(error.field("collection"), Some("docs"));
        assert!(
            error.message.contains("Vector dimension error"),
            "engine cause lost: {}",
            error.message
        );

        backend.close().await.expect("close edge backend");
        let _ = std::fs::remove_dir_all(dir);
    });
}

/// Non-UUID string point IDs are rejected before the engine call, with the
/// dedicated id-conversion code.
#[test]
fn invalid_string_point_id_reports_code() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let dir = temp_dir("invalid-point-id");
        let _ = std::fs::remove_dir_all(&dir);
        let backend = EdgeQdrant::new(&dir, false);
        backend
            .execute_planned(&plan_one(
                "CREATE COLLECTION docs (dense VECTOR(3, COSINE))",
            ))
            .await
            .expect("create collection");

        let error = backend
            .execute_planned(&plan_one("QUERY POINTS ('doc-1') FROM docs"))
            .await
            .expect_err("non-UUID string id must fail");
        assert_eq!(error.code, "QQL-EDGE-INVALID-POINT-ID");
        assert!(
            error.message.contains("doc-1"),
            "id lost from message: {}",
            error.message
        );

        backend.close().await.expect("close edge backend");
        let _ = std::fs::remove_dir_all(dir);
    });
}

/// A corrupted shard config makes `EdgeShard::load` fail; the failure must
/// surface as the engine-storage code with the crate's own cause text.
#[test]
fn corrupted_shard_load_reports_storage_code() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let dir = temp_dir("corrupt-shard");
        let _ = std::fs::remove_dir_all(&dir);
        let backend = EdgeQdrant::new(&dir, false);
        backend
            .execute_planned(&plan_one(
                "CREATE COLLECTION docs (dense VECTOR(3, COSINE))",
            ))
            .await
            .expect("create collection");
        backend.close().await.expect("close edge backend");

        // Corrupt the persisted shard config so the next load fails inside
        // qdrant-edge (`ServiceError` from the JSON read).
        std::fs::write(dir.join("docs").join("edge_config.json"), b"{ not json")
            .expect("corrupt config");

        let error = backend
            .execute_planned(&plan_one("SHOW COLLECTION docs"))
            .await
            .expect_err("corrupt shard must fail to load");
        assert_eq!(error.code, "QQL-EDGE-STORAGE");
        assert_eq!(error.field("operation"), Some("load"));
        assert_eq!(error.field("collection"), Some("docs"));
        assert!(
            error.message.contains("qdrant-edge load failed:"),
            "operation context lost: {}",
            error.message
        );

        let _ = std::fs::remove_dir_all(dir);
    });
}

/// An I/O failure creating the collection directory (base path is a file)
/// reports the dedicated storage-path code.
#[test]
fn create_directory_io_failure_reports_code() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let base = temp_dir("create-dir-io");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::write(&base, b"not a directory").expect("seed file");
        let backend = EdgeQdrant::new(&base, false);

        let error = backend
            .execute_planned(&plan_one(
                "CREATE COLLECTION docs (dense VECTOR(1, COSINE))",
            ))
            .await
            .expect_err("create_dir_all over a file must fail");
        assert_eq!(error.code, "QQL-EDGE-CREATE-DIR");

        let _ = std::fs::remove_file(&base);
    });
}

/// `QUERY … GROUP BY` runs through qdrant-edge's grouping driver: typed
/// group ids, hydrated payloads, and the executor's client-side
/// `group_offset` trimming.
#[test]
fn executor_group_by_returns_typed_groups() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
            let dir = temp_dir("group-by");
            let _ = std::fs::remove_dir_all(&dir);
            let executor = Executor::new(Box::new(EdgeQdrant::new(&dir, false)), None);

            let report = executor
                .execute(
                    "CREATE COLLECTION docs (dense VECTOR(3, DOT))",
                    OnError::Stop,
                )
                .await
                .expect("create collection");
            assert!(report.ok, "create failed: {report:?}");
            let report = executor
                .execute(
                    "CREATE INDEX ON COLLECTION docs FOR district TYPE keyword",
                    OnError::Stop,
                )
                .await
                .expect("create index");
            assert!(report.ok, "index failed: {report:?}");
            for qql in [
                "UPSERT INTO docs VALUES {id: 1, vector: {dense: [3.0, 0.0, 0.0]}, district: 'NYC', price: 10}",
                "UPSERT INTO docs VALUES {id: 2, vector: {dense: [2.0, 0.0, 0.0]}, district: 'SF', price: 20}",
                "UPSERT INTO docs VALUES {id: 3, vector: {dense: [1.0, 0.0, 0.0]}, district: 'LA', price: 30}",
                "UPSERT INTO docs VALUES {id: 4, vector: {dense: [0.5, 0.0, 0.0]}, district: 'NYC', price: 40}",
            ] {
                let report = executor.execute(qql, OnError::Stop).await.expect("upsert");
                assert!(report.ok, "{qql} failed: {report:?}");
            }

            let report = executor
                .execute(
                    "QUERY [3.0, 0.0, 0.0] FROM docs USING dense GROUP BY district LIMIT 10",
                    OnError::Stop,
                )
                .await
                .expect("group run");
            assert!(report.ok, "GROUP BY must succeed offline: {report:?}");
            let response = report.first().expect("group response");
            let Some(ExecData::Groups(groups)) = response.data.as_ref() else {
                panic!("expected typed groups, got {:?}", response.data);
            };
            assert_eq!(groups.len(), 3, "groups: {groups:?}");
            assert_eq!(groups[0].group_id, PlanGroupId::Keyword("NYC".into()));
            assert_eq!(groups[1].group_id, PlanGroupId::Keyword("SF".into()));
            assert_eq!(groups[2].group_id, PlanGroupId::Keyword("LA".into()));
            assert_eq!(groups[0].hits.len(), 2, "NYC has two points");
            assert_eq!(groups[1].hits.len(), 1);
            // Hydration: the grouping driver only fetched the `district` field.
            let payload = groups[0].hits[0]
                .payload
                .as_ref()
                .expect("hydrated payload");
            assert_eq!(payload.get("price"), Some(&json!(10)));

            // `group_offset` has no wire field: the backend returns
            // LIMIT+OFFSET groups and the executor trims OFFSET client-side.
            let report = executor
                .execute(
                    "QUERY [3.0, 0.0, 0.0] FROM docs USING dense GROUP BY district LIMIT 1 OFFSET 1",
                    OnError::Stop,
                )
                .await
                .expect("group offset run");
            assert!(report.ok, "group offset failed: {report:?}");
            let response = report.first().expect("group response");
            let Some(ExecData::Groups(groups)) = response.data.as_ref() else {
                panic!("expected typed groups, got {:?}", response.data);
            };
            assert_eq!(groups.len(), 1, "offset must trim one group: {groups:?}");
            assert_eq!(groups[0].group_id, PlanGroupId::Keyword("SF".into()));

            // `LOOKUP FROM` has no edge equivalent; only that sub-feature fails.
            let report = executor
                .execute(
                    "QUERY [3.0, 0.0, 0.0] FROM docs USING dense \
                     GROUP BY district LOOKUP FROM districts LIMIT 5",
                    OnError::Continue,
                )
                .await
                .expect("lookup run");
            assert!(!report.ok, "LOOKUP FROM must fail");
            assert!(
                report.results[0]
                    .message
                    .contains("QQL-EDGE-UNSUPPORTED-GROUP-LOOKUP"),
                "expected GROUP-LOOKUP code, got {:?}",
                report.results[0].message
            );

            executor.close().await.expect("close edge executor");
            let _ = std::fs::remove_dir_all(dir);
        });
}

/// `PARAMS (acorn = …)` executes on edge: qdrant-edge 0.8 models ACORN as
/// a first-class `SearchParams` field, so the query must reach the engine.
#[test]
fn acorn_params_execute_on_edge() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let dir = temp_dir("acorn");
        let _ = std::fs::remove_dir_all(&dir);
        let backend = EdgeQdrant::new(&dir, false);
        seed_docs(&backend).await;

        let query = plan_one(
            "QUERY [1.0, 0.0, 0.0] FROM docs USING dense WHERE city = 'NYC' \
                 PARAMS (acorn = true, max_selectivity = 0.4) LIMIT 5",
        );
        let response = backend
            .execute_planned(&query)
            .await
            .expect("ACORN query must execute offline");
        let ExecData::Hits(hits) = &response.data else {
            panic!("expected typed hits, got {:?}", response.data);
        };
        assert_eq!(hits.len(), 2);

        backend.close().await.expect("close edge backend");
        let _ = std::fs::remove_dir_all(dir);
    });
}

/// `ALTER COLLECTION` applies the qdrant-edge config setters and persists
/// them; params / quantization / excluded optimizer keys stay fail-closed
/// per field.
#[test]
fn alter_collection_applies_engine_config() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let dir = temp_dir("alter-collection");
        let _ = std::fs::remove_dir_all(&dir);
        let backend = EdgeQdrant::new(&dir, false);
        backend
            .execute_planned(&plan_one(
                "CREATE COLLECTION docs (dense VECTOR(3, COSINE))",
            ))
            .await
            .expect("create collection");

        let response = backend
            .execute_planned(&plan_one(
                "ALTER COLLECTION docs WITH HNSW (m = 32) \
                     WITH OPTIMIZERS (indexing_threshold = 500)",
            ))
            .await
            .expect("ALTER must apply");
        assert_eq!(response.data, ExecData::Mutation { affected: None });

        {
            let shard = backend.open_shard("docs").await.expect("open shard");
            let config = shard.config();
            assert_eq!(config.hnsw_config().m, 32);
            assert_eq!(config.optimizers().indexing_threshold, Some(500));
        }

        backend
            .execute_planned(&plan_one(
                "ALTER COLLECTION docs WITH HNSW (ef_construct = 200) \
                     WITH OPTIMIZERS (deleted_threshold = 0.3)",
            ))
            .await
            .expect("partial ALTER must merge");
        {
            let shard = backend.open_shard("docs").await.expect("open shard");
            let config = shard.config();
            assert_eq!(config.hnsw_config().m, 32, "partial ALTER must keep m");
            assert_eq!(config.hnsw_config().ef_construct, 200);
            assert_eq!(config.optimizers().indexing_threshold, Some(500));
            assert_eq!(config.optimizers().deleted_threshold, Some(0.3));
        }

        // Persisted: a fresh backend sees the updated engine config.
        backend.close().await.expect("close edge backend");
        let reopened = EdgeQdrant::new(&dir, false);
        {
            let shard = reopened.open_shard("docs").await.expect("reopen shard");
            let config = shard.config();
            assert_eq!(config.hnsw_config().m, 32);
            assert_eq!(config.hnsw_config().ef_construct, 200);
            assert_eq!(config.optimizers().indexing_threshold, Some(500));
            assert_eq!(config.optimizers().deleted_threshold, Some(0.3));
        }

        let error = reopened
            .execute_planned(&plan_one(
                "ALTER COLLECTION docs WITH PARAMS (replication_factor = 3)",
            ))
            .await
            .expect_err("params have no edge setter");
        assert_eq!(error.code, "QQL-EDGE-UNSUPPORTED-ALTER-PARAMS");

        let error = reopened
            .execute_planned(&plan_one(
                "ALTER COLLECTION docs WITH QUANTIZATION (disabled = true)",
            ))
            .await
            .expect_err("quantization has no edge setter");
        assert_eq!(error.code, "QQL-EDGE-UNSUPPORTED-ALTER-QUANTIZATION");

        let error = reopened
            .execute_planned(&plan_one(
                "ALTER COLLECTION docs WITH OPTIMIZERS (flush_interval_sec = 5)",
            ))
            .await
            .expect_err("excluded optimizer key");
        assert_eq!(error.code, "QQL-EDGE-UNSUPPORTED-OPTIMIZER-KEY");

        reopened.close().await.expect("close edge backend");
        let _ = std::fs::remove_dir_all(dir);
    });
}

/// Per-vector `ALTER COLLECTION`: HNSW patches field-merge over the
/// vector's effective config and persist across reopen; every other
/// per-vector field and all sparse diffs reject per field, only when
/// present, and a mixed request never half-applies.
#[test]
fn alter_collection_applies_per_vector_hnsw_diff() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let dir = temp_dir("alter-vector-diff");
        let _ = std::fs::remove_dir_all(&dir);
        let backend = EdgeQdrant::new(&dir, false);
        backend
            .execute_planned(&plan_one(
                "CREATE COLLECTION docs (dense VECTOR(3, COSINE), other VECTOR(3, COSINE)) \
                     WITH HNSW (m = 8, ef_construct = 64)",
            ))
            .await
            .expect("create collection");

        backend
            .execute_planned(&plan_one(
                "ALTER COLLECTION docs WITH VECTOR dense (HNSW (m = 32))",
            ))
            .await
            .expect("per-vector HNSW must apply");
        {
            let shard = backend.open_shard("docs").await.expect("open shard");
            let config = shard.config();
            let dense = config.vectors.get("dense").expect("dense vector");
            let hnsw = dense.hnsw_config.expect("per-vector HNSW");
            assert_eq!(hnsw.m, 32);
            // Unset diff fields inherit the collection-wide value.
            assert_eq!(hnsw.ef_construct, 64);
            assert!(
                config
                    .vectors
                    .get("other")
                    .expect("other vector")
                    .hnsw_config
                    .is_none(),
                "untouched vectors keep no per-vector override"
            );
        }

        // A partial diff merges over the vector's own config.
        backend
            .execute_planned(&plan_one(
                "ALTER COLLECTION docs WITH VECTOR dense (HNSW (ef_construct = 200))",
            ))
            .await
            .expect("partial per-vector HNSW must merge");
        {
            let shard = backend.open_shard("docs").await.expect("open shard");
            let hnsw = shard
                .config()
                .vectors
                .get("dense")
                .and_then(|params| params.hnsw_config)
                .expect("per-vector HNSW");
            assert_eq!(hnsw.m, 32, "partial diff keeps m");
            assert_eq!(hnsw.ef_construct, 200);
        }

        backend.close().await.expect("close edge backend");
        let reopened = EdgeQdrant::new(&dir, false);
        {
            let shard = reopened.open_shard("docs").await.expect("reopen shard");
            let hnsw = shard
                .config()
                .vectors
                .get("dense")
                .and_then(|params| params.hnsw_config)
                .expect("persisted per-vector HNSW");
            assert_eq!(hnsw.m, 32);
            assert_eq!(hnsw.ef_construct, 200);
        }

        for (query, key) in [
            (
                "ALTER COLLECTION docs WITH VECTOR dense (VECTOR (memory = 'cold'))",
                "memory",
            ),
            (
                "ALTER COLLECTION docs WITH VECTOR dense (QUANTIZATION (type = 'scalar'))",
                "quantization_config",
            ),
        ] {
            let error = reopened
                .execute_planned(&plan_one(query))
                .await
                .expect_err("per-vector field has no edge setter");
            assert_eq!(error.code, "QQL-EDGE-UNSUPPORTED-VECTOR-DIFF");
            assert_eq!(error.field("vector_name"), Some("dense"));
            assert_eq!(error.field("config_key"), Some(key));
        }

        let error = reopened
            .execute_planned(&plan_one(
                "ALTER COLLECTION docs WITH SPARSE bm25 (SPARSE (modifier = 'idf'))",
            ))
            .await
            .expect_err("sparse diff has no edge setter");
        assert_eq!(error.code, "QQL-EDGE-UNSUPPORTED-SPARSE-DIFF");
        assert_eq!(error.field("vector_name"), Some("bm25"));
        assert_eq!(error.field("config_key"), Some("modifier"));

        // A mixed request is validated before any setter runs.
        let error = reopened
            .execute_planned(&plan_one(
                "ALTER COLLECTION docs WITH VECTOR dense (HNSW (m = 48), VECTOR (on_disk = true))",
            ))
            .await
            .expect_err("mixed request must reject");
        assert_eq!(error.code, "QQL-EDGE-UNSUPPORTED-VECTOR-DIFF");
        {
            let shard = reopened.open_shard("docs").await.expect("open shard");
            let hnsw = shard
                .config()
                .vectors
                .get("dense")
                .and_then(|params| params.hnsw_config)
                .expect("per-vector HNSW");
            assert_eq!(hnsw.m, 32, "rejected request must not half-apply HNSW");
        }

        reopened.close().await.expect("close edge backend");
        let _ = std::fs::remove_dir_all(dir);
    });
}

/// Create-time `WITH PARAMS (on_disk_payload = …)` maps onto the engine
/// config; other param keys still fail closed.
#[test]
fn create_collection_params_honor_on_disk_payload() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let dir = temp_dir("create-params");
        let _ = std::fs::remove_dir_all(&dir);
        // Executor-level default is on-disk; the statement overrides it.
        let backend = EdgeQdrant::new(&dir, true);
        backend
            .execute_planned(&plan_one(
                "CREATE COLLECTION docs (dense VECTOR(1, COSINE)) \
                     WITH PARAMS (on_disk_payload = false)",
            ))
            .await
            .expect("on_disk_payload must be accepted");

        let raw = std::fs::read(dir.join("docs").join("edge_config.json"))
            .expect("read persisted config");
        let config: serde_json::Value = serde_json::from_slice(&raw).expect("parse config");
        assert_eq!(config["on_disk_payload"], json!(false));

        let error = backend
            .execute_planned(&plan_one(
                "CREATE COLLECTION other (dense VECTOR(1, COSINE)) \
                     WITH PARAMS (replication_factor = 3)",
            ))
            .await
            .expect_err("replication params have no edge equivalent");
        assert_eq!(error.code, "QQL-EDGE-UNSUPPORTED-COLLECTION-PARAMS");

        backend.close().await.expect("close edge backend");
        let _ = std::fs::remove_dir_all(dir);
    });
}

/// Per-vector storage / datatype / quantization / HNSW and sparse index
/// options from `CREATE COLLECTION` land in the engine config instead of
/// being dropped silently.
#[test]
fn create_collection_passes_per_vector_engine_config() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let dir = temp_dir("create-per-vector");
        let _ = std::fs::remove_dir_all(&dir);
        let backend = EdgeQdrant::new(&dir, false);
        backend
            .execute_planned(&plan_one(
                "CREATE COLLECTION docs ( \
                       dense VECTOR(4, COSINE) \
                         WITH HNSW (m = 24) \
                         WITH QUANTIZATION (type = 'scalar', quantile = 0.99) \
                         WITH VECTOR (memory = 'cold', datatype = 'float16'), \
                       sparse SPARSE \
                         WITH SPARSE (full_scan_threshold = 5000, memory = 'pinned', \
                                      datatype = 'float16', modifier = 'idf') \
                     )",
            ))
            .await
            .expect("create collection with per-vector engine config");

        {
            let shard = backend.open_shard("docs").await.expect("open shard");
            let config = shard.config();

            let dense = config.vectors.get("dense").expect("dense vector");
            assert_eq!(
                dense.on_disk,
                Some(true),
                "memory = 'cold' maps to the engine's on-disk flag"
            );
            assert_eq!(
                dense.datatype,
                Some(qdrant_edge::VectorStorageDatatype::Float16)
            );
            assert!(
                dense.quantization_config.is_some(),
                "per-vector quantization must reach the engine"
            );
            assert_eq!(
                dense.hnsw_config.expect("per-vector HNSW").m,
                24,
                "per-vector HNSW must reach the engine"
            );

            let sparse = config.sparse_vectors.get("sparse").expect("sparse vector");
            assert_eq!(sparse.full_scan_threshold, Some(5_000));
            assert_eq!(
                sparse.on_disk,
                Some(false),
                "memory = 'pinned' keeps the sparse index in RAM"
            );
            assert_eq!(
                sparse.datatype,
                Some(qdrant_edge::VectorStorageDatatype::Float16)
            );
            assert_eq!(sparse.modifier, Some(qdrant_edge::Modifier::Idf));
        }

        // Config persists and reloads with the same values.
        backend.close().await.expect("close edge backend");
        let reopened = EdgeQdrant::new(&dir, false);
        {
            let shard = reopened.open_shard("docs").await.expect("reopen shard");
            let config = shard.config();
            assert_eq!(
                config
                    .vectors
                    .get("dense")
                    .and_then(|v| v.hnsw_config)
                    .map(|h| h.m),
                Some(24)
            );
        }

        reopened.close().await.expect("close edge backend");
        let _ = std::fs::remove_dir_all(dir);
    });
}

/// The WAL segment capacity is seeded once: a persisted value wins over
/// later env/CLI values, and reopening with the knob still set (or unset)
/// does not rewrite the persisted config when the value is unchanged.
#[test]
fn wal_segment_capacity_seeds_once_then_persisted_wins() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let dir = temp_dir("wal-capacity");
        let _ = std::fs::remove_dir_all(&dir);
        let config_path = dir.join("docs").join("edge_config.json");
        let capacity = 4 * 1024 * 1024;
        let larger = 8 * 1024 * 1024;

        // Seed: the knob applies while the shard has no persisted value.
        let backend = EdgeQdrant::new(&dir, false).with_wal_segment_capacity(Some(capacity));
        backend
            .execute_planned(&plan_one(
                "CREATE COLLECTION docs (dense VECTOR(1, COSINE))",
            ))
            .await
            .expect("create collection");
        {
            let shard = backend.open_shard("docs").await.expect("open shard");
            assert_eq!(
                shard
                    .config()
                    .wal_options
                    .as_ref()
                    .map(|w| w.segment_capacity),
                Some(capacity),
                "create must persist the configured WAL capacity"
            );
        }
        backend.close().await.expect("close edge backend");
        let seeded_bytes = std::fs::read(&config_path).expect("read seeded config");

        // Persisted wins: a different knob value must not ratchet the
        // shard config on load.
        let reopened = EdgeQdrant::new(&dir, false).with_wal_segment_capacity(Some(larger));
        {
            let shard = reopened.open_shard("docs").await.expect("reopen shard");
            assert_eq!(
                shard
                    .config()
                    .wal_options
                    .as_ref()
                    .map(|w| w.segment_capacity),
                Some(capacity),
                "persisted capacity must win over a later env/CLI value"
            );
        }
        reopened.close().await.expect("close edge backend");
        assert_eq!(
            std::fs::read(&config_path).expect("read after override"),
            seeded_bytes,
            "persisted config must not change when a different knob value is supplied"
        );

        // Env still set at the seeded value, then unset: the file must stay
        // byte-identical. (qdrant-edge 0.8 rewrites edge_config.json on
        // every load via `SaveOnDisk::new`, so mtime is not a stable
        // signal; content equality is the observable invariant.)
        for knob in [Some(capacity), None] {
            let backend = EdgeQdrant::new(&dir, false).with_wal_segment_capacity(knob);
            {
                let shard = backend.open_shard("docs").await.expect("reopen shard");
                assert_eq!(
                    shard
                        .config()
                        .wal_options
                        .as_ref()
                        .map(|w| w.segment_capacity),
                    Some(capacity),
                    "unset/equal knob must keep the persisted capacity"
                );
            }
            backend.close().await.expect("close edge backend");
            assert_eq!(
                std::fs::read(&config_path).expect("read after reopen"),
                seeded_bytes,
                "edge_config.json must stay identical across reopens ({knob:?})"
            );
        }

        // qdrant-edge 0.8's load path calls `SaveOnDisk::new`, which
        // rewrites `edge_config.json` unconditionally (verified: mtime
        // bumps while the content is identical). Content byte-equality —
        // asserted above — is the observable no-ratchet invariant without
        // patching the dependency.
        let _ = std::fs::remove_dir_all(dir);
    });
}
