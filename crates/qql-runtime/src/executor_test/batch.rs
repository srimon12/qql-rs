use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::backend::CollectionInfo;
use crate::executor::response::{GroupedSearchResult, OnError, SearchHit};
use crate::executor::{ExecData, Executor};
use qql_plan::{PlanGroupId, PlanPointId};

use super::mock::{MockEmbedder, MockQdrantClient, collection_with_vectors, test_config};

#[tokio::test]
async fn test_batch_query_groups_same_collection() {
    let mut client = MockQdrantClient::default();
    client.info = Some(CollectionInfo::default()); // unnamed vector → passes check
    let batch_count = client.batch_call_count.clone();
    let searches_count = client.last_batch_searches_count.clone();

    let executor = Executor::new(Box::new(client), Some(test_config()));

    let resp = qql_core::parser::Parser::parse_all(
        "QUERY TEXT 'a' MODEL 'test-model' FROM docs USING dense AS DENSE LIMIT 1;\
         QUERY TEXT 'b' MODEL 'test-model' FROM docs USING dense AS DENSE LIMIT 1;\
         QUERY TEXT 'c' MODEL 'test-model' FROM docs USING dense AS DENSE LIMIT 1;",
    )
    .unwrap();
    let results = executor.execute_batch_nodes(resp, false).await.unwrap();

    // 3 queries, 3 results, 1 batch call
    assert_eq!(results.len(), 3, "expected 3 results");
    for r in &results {
        assert!(r.ok, "result should be ok: {:?}", r);
    }

    let calls = *batch_count.lock().unwrap();
    assert_eq!(calls, 1, "expected 1 batch call, got {calls}");

    let count = *searches_count.lock().unwrap();
    assert_eq!(count, 3, "expected 3 searches in batch, got {count}");
}

#[tokio::test]
async fn test_batch_mutations_same_collection() {
    let client = MockQdrantClient::default();
    let update_count = client.update_batch_call_count.clone();
    let ops_count = client.last_update_batch_ops_count.clone();
    let route_count = client.last_planned.clone();

    let executor = Executor::new(Box::new(client), Some(test_config()));

    let stmts = qql_core::parser::Parser::parse_all(
        "UPSERT INTO docs VALUES {id: 1, title: 'a'};\
         UPSERT INTO docs VALUES {id: 2, title: 'b'};\
         DELETE FROM docs WHERE id = 3;",
    )
    .unwrap();
    let results = executor.execute_batch_nodes(stmts, false).await.unwrap();

    assert_eq!(
        results.len(),
        3,
        "expected 3 results, got {}",
        results.len()
    );
    for r in &results {
        assert!(r.ok, "result should be ok: {:?}", r);
    }

    let calls = *update_count.lock().unwrap();
    assert_eq!(calls, 1, "expected 1 update-batch call, got {calls}");

    let count = *ops_count.lock().unwrap();
    assert_eq!(count, 3, "expected 3 ops in batch, got {count}");

    // Individual routes should not have been used for these mutations
    assert!(
        route_count.lock().unwrap().is_none(),
        "mutations should go through update batch, not execute_route"
    );
}

#[tokio::test]
async fn test_delete_payload_batches_with_mutations() {
    // P1 full batch support: `DELETE PAYLOAD` has an `UpdateOperation` batch
    // form (`delete_payload`), so same-collection runs batch instead of
    // isolating. REST (`DeletePayloadOperation`) and gRPC
    // (`PointsUpdateOperation.delete_payload = 5`) both carry it.
    let client = MockQdrantClient::default();
    let update_count = client.update_batch_call_count.clone();
    let individual_calls = client.execute_planned_call_count.clone();

    let executor = Executor::new(Box::new(client), Some(test_config()));

    // Two DELETE PAYLOAD statements batch into one update-batch RPC.
    let stmts = qql_core::parser::Parser::parse_all(
        "DELETE PAYLOAD draft FROM docs WHERE id = 1;\
         DELETE PAYLOAD final FROM docs WHERE id = 2;",
    )
    .unwrap();
    let results = executor.execute_batch_nodes(stmts, false).await.unwrap();
    assert_eq!(
        results.len(),
        2,
        "expected 2 results, got {}",
        results.len()
    );
    for r in &results {
        assert!(r.ok, "DELETE PAYLOAD should succeed: {:?}", r);
        assert_eq!(r.operation, "DELETE_PAYLOAD");
    }
    assert_eq!(
        *update_count.lock().unwrap(),
        1,
        "DELETE PAYLOAD run must use update batch"
    );
    assert_eq!(
        *individual_calls.lock().unwrap(),
        0,
        "batched DELETE PAYLOAD must not dispatch singly"
    );

    // UPSERT + DELETE PAYLOAD in one same-collection group batch together.
    let client = MockQdrantClient::default();
    let update_count = client.update_batch_call_count.clone();
    let ops_count = client.last_update_batch_ops_count.clone();
    let individual_calls = client.execute_planned_call_count.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));
    let stmts = qql_core::parser::Parser::parse_all(
        "UPSERT INTO docs VALUES {id: 1, title: 'a'};\
         DELETE PAYLOAD draft FROM docs WHERE id = 1;",
    )
    .unwrap();
    let results = executor.execute_batch_nodes(stmts, false).await.unwrap();
    assert_eq!(results.len(), 2);
    assert!(results[0].ok, "UPSERT should succeed: {:?}", results[0]);
    assert_eq!(results[0].operation, "UPSERT");
    assert!(
        results[1].ok,
        "DELETE PAYLOAD should succeed: {:?}",
        results[1]
    );
    assert_eq!(results[1].operation, "DELETE_PAYLOAD");
    assert_eq!(
        *update_count.lock().unwrap(),
        1,
        "UPSERT + DELETE PAYLOAD must batch"
    );
    assert_eq!(*ops_count.lock().unwrap(), 2);
    assert_eq!(
        *individual_calls.lock().unwrap(),
        0,
        "batched run must not dispatch singly"
    );

    // DELETE PAYLOAD sandwiched between batchable mutations: the whole
    // same-collection run batches as one update batch, order preserved.
    let client = MockQdrantClient::default();
    let update_count = client.update_batch_call_count.clone();
    let ops_count = client.last_update_batch_ops_count.clone();
    let individual_calls = client.execute_planned_call_count.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));
    let stmts = qql_core::parser::Parser::parse_all(
        "UPSERT INTO docs VALUES {id: 1, title: 'a'};\
         DELETE PAYLOAD draft FROM docs WHERE id = 1;\
         DELETE FROM docs WHERE id = 2;\
         UPSERT INTO docs VALUES {id: 3, title: 'c'};",
    )
    .unwrap();
    let results = executor.execute_batch_nodes(stmts, false).await.unwrap();
    assert_eq!(results.len(), 4);
    assert_eq!(results[0].operation, "UPSERT");
    assert_eq!(results[1].operation, "DELETE_PAYLOAD");
    assert_eq!(results[2].operation, "DELETE");
    assert_eq!(results[3].operation, "UPSERT");
    for r in &results {
        assert!(r.ok, "all statements should succeed: {:?}", r);
    }
    assert_eq!(
        *update_count.lock().unwrap(),
        1,
        "full same-collection run must batch once"
    );
    assert_eq!(*ops_count.lock().unwrap(), 4);
    assert_eq!(
        *individual_calls.lock().unwrap(),
        0,
        "batched run must not dispatch singly"
    );
}

#[tokio::test]
async fn test_grouped_offset_applied_exactly_once() {
    // Regression (task 5): GROUP BY with OFFSET must apply the offset exactly
    // once. The planner sends `limit = user_limit + offset` and the executor
    // trims the returned groups client-side; `group_offset` has no wire
    // representation (serde-skipped, not in the gRPC QueryPointGroups proto),
    // so the backend never applies it. Simulate a server returning `limit`
    // groups and assert the result is exactly `user_limit` groups starting at
    // the offset — a double application would drop or duplicate groups.
    let group = |id: &str, point: u64, score: f32| GroupedSearchResult {
        group_id: PlanGroupId::Keyword(id.into()),
        hits: vec![SearchHit {
            id: PlanPointId::Number(point),
            score,
            payload: None,
            collection: None,
            vector: None,
        }],
    };
    let client = MockQdrantClient {
        info: Some(collection_with_vectors(&["dense"], &["sparse"])),
        point_map: Arc::new(Mutex::new(HashMap::from([(
            "docs".to_string(),
            ExecData::Groups(vec![
                group("a", 1, 1.0),
                group("b", 2, 0.9),
                group("c", 3, 0.8),
            ]),
        )]))),
        ..Default::default()
    };
    let executor = Executor::new(Box::new(client), Some(test_config()));
    let stmts = qql_core::parser::Parser::parse_all(
        "QUERY TEXT 'x' MODEL 'test-model' FROM docs USING dense AS DENSE GROUP BY category LIMIT 2 OFFSET 1;",
    )
    .unwrap();
    let results = executor.execute_batch_nodes(stmts, false).await.unwrap();
    assert_eq!(results.len(), 1);
    let r = &results[0];
    assert!(r.ok, "grouped query should succeed: {:?}", r);
    assert_eq!(r.operation, "QUERY_GROUPS");
    assert_eq!(r.message, "Found 2 group(s)");
    let groups = r
        .data
        .as_ref()
        .and_then(ExecData::groups)
        .expect("grouped data is typed");
    assert_eq!(
        groups.len(),
        2,
        "must return exactly user_limit groups, got {}",
        groups.len()
    );
    assert_eq!(
        groups[0].group_id,
        PlanGroupId::Keyword("b".into()),
        "groups must start at the offset"
    );
    assert_eq!(groups[1].group_id, PlanGroupId::Keyword("c".into()));
}

#[tokio::test]
async fn test_batch_preserves_order_mixed_query_and_mutation() {
    let client = MockQdrantClient {
        info: Some(CollectionInfo::default()),
        ..Default::default()
    };
    let query_batch = client.batch_call_count.clone();
    let update_batch = client.update_batch_call_count.clone();

    let executor = Executor::new(Box::new(client), Some(test_config()));

    // Two mutations, then two queries — should batch each group separately
    let stmts = qql_core::parser::Parser::parse_all(
        "UPSERT INTO docs VALUES {id: 1};\
         DELETE FROM docs WHERE id = 2;\
         QUERY TEXT 'a' MODEL 'test-model' FROM docs USING dense AS DENSE LIMIT 1;\
         QUERY TEXT 'b' MODEL 'test-model' FROM docs USING dense AS DENSE LIMIT 1;",
    )
    .unwrap();
    let results = executor.execute_batch_nodes(stmts, false).await.unwrap();

    assert_eq!(results.len(), 4);
    assert_eq!(results[0].operation, "UPSERT");
    assert_eq!(results[1].operation, "DELETE");
    assert_eq!(results[2].operation, "QUERY");
    assert_eq!(results[3].operation, "QUERY");

    assert_eq!(*update_batch.lock().unwrap(), 1);
    assert_eq!(*query_batch.lock().unwrap(), 1);
}

#[tokio::test]
async fn test_single_mutation_not_batched() {
    let client = MockQdrantClient::default();
    let update_count = client.update_batch_call_count.clone();

    let executor = Executor::new(Box::new(client), Some(test_config()));
    let stmts = qql_core::parser::Parser::parse_all("DELETE FROM docs WHERE id = 1;").unwrap();
    let results = executor.execute_batch_nodes(stmts, false).await.unwrap();

    assert_eq!(results.len(), 1);
    assert!(results[0].ok);
    assert_eq!(
        *update_count.lock().unwrap(),
        0,
        "single mutation must not use update batch"
    );
}

#[tokio::test]
async fn test_continue_preserves_failure_position_and_batch_boundary() {
    let client = MockQdrantClient::default();
    let update_batches = client.update_batch_call_count.clone();
    let individual_calls = client.execute_planned_call_count.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));
    let stmts = qql_core::parser::Parser::parse_all(
        "DELETE FROM docs WHERE id = 1;\
         QUERY TEXT 'missing schema' MODEL 'test-model' FROM docs LIMIT 1;\
         DELETE FROM docs WHERE id = 2;",
    )
    .unwrap();

    let results = executor.execute_batch_nodes(stmts, false).await.unwrap();

    assert_eq!(results.len(), 3);
    assert_eq!(results[0].operation, "DELETE");
    assert!(results[0].ok);
    assert_eq!(results[1].operation, "PREPARE");
    assert!(!results[1].ok);
    assert_eq!(results[2].operation, "DELETE");
    assert!(results[2].ok);
    assert_eq!(*update_batches.lock().unwrap(), 0);
    assert_eq!(*individual_calls.lock().unwrap(), 2);
}

#[tokio::test]
async fn test_stop_dispatches_prior_statement_before_later_prepare_failure() {
    let client = MockQdrantClient::default();
    let individual_calls = client.execute_planned_call_count.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));
    let stmts = qql_core::parser::Parser::parse_all(
        "DELETE FROM docs WHERE id = 1;\
         QUERY TEXT 'missing schema' MODEL 'test-model' FROM docs LIMIT 1;",
    )
    .unwrap();

    let error = executor
        .execute_batch_nodes(stmts, true)
        .await
        .expect_err("the second statement should fail preparation");

    assert!(error.message.contains("no mock info set"));
    assert_eq!(*individual_calls.lock().unwrap(), 1);
}

#[tokio::test]
async fn test_batch_upserts_keep_single_statement_auto_create_semantics() {
    let client = MockQdrantClient::default();
    let creates = client.execute_planned_call_count.clone();
    let update_batches = client.update_batch_call_count.clone();
    let embedder = Arc::new(MockEmbedder {
        dense: vec![0.1, 0.2, 0.3],
        sparse_indices: Vec::new(),
        sparse_values: Vec::new(),
        multi: vec![vec![0.1, 0.2], vec![0.3, 0.4]],
    });
    let mut config = test_config();
    config.embedding_dimension = 3;
    let executor = Executor::with_embedder(Box::new(client), Some(config), Some(embedder));
    let stmts = qql_core::parser::Parser::parse_all(
        "UPSERT INTO docs VALUES {id: 1, text: 'a'} USING DENSE MODEL 'mock';\
         UPSERT INTO docs VALUES {id: 2, text: 'b'} USING DENSE MODEL 'mock';",
    )
    .unwrap();

    let results = executor.execute_batch_nodes(stmts, true).await.unwrap();

    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|result| result.ok));
    assert_eq!(*creates.lock().unwrap(), 1);
    assert_eq!(*update_batches.lock().unwrap(), 1);
}

#[tokio::test]
async fn test_execute_batch_continue_collects_parse_errors_in_order() {
    let client = MockQdrantClient::default();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let report = executor
        .execute_batch(
            &[
                "DELETE FROM docs WHERE id = 1",
                "not qql",
                "DELETE FROM docs WHERE id = 2",
            ],
            OnError::Continue,
        )
        .await
        .unwrap();

    assert!(!report.ok);
    assert_eq!(report.succeeded, 2);
    assert_eq!(report.failed, 1);
    assert_eq!(report.results[0].operation, "DELETE");
    assert_eq!(report.results[1].operation, "PARSE");
    assert_eq!(report.results[2].operation, "DELETE");
}

#[tokio::test]
async fn test_execute_batch_stop_dispatches_prior_entries_before_parse_failure() {
    let client = MockQdrantClient::default();
    let individual_calls = client.execute_planned_call_count.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let error = executor
        .execute_batch(&["DELETE FROM docs WHERE id = 1", "not qql"], OnError::Stop)
        .await
        .expect_err("the second entry should fail parsing");

    assert_eq!(error.code, "QQL-PARSE-STATEMENT");
    assert_eq!(*individual_calls.lock().unwrap(), 1);
}

#[tokio::test]
async fn test_execute_continue_returns_parse_failure_report() {
    let executor = Executor::new(Box::new(MockQdrantClient::default()), Some(test_config()));

    let report = executor
        .execute("not qql", OnError::Continue)
        .await
        .expect("continue mode should report parse failures");

    assert!(!report.ok);
    assert_eq!(report.succeeded, 0);
    assert_eq!(report.failed, 1);
    assert_eq!(report.results[0].operation, "PARSE");
}

#[tokio::test]
async fn test_execute_continue_returns_single_preparation_failure_report() {
    let executor = Executor::new(Box::new(MockQdrantClient::default()), Some(test_config()));

    let report = executor
        .execute(
            "QUERY TEXT 'missing schema' MODEL 'test-model' FROM docs LIMIT 1",
            OnError::Continue,
        )
        .await
        .expect("continue mode should report preparation failures");

    assert!(!report.ok);
    assert_eq!(report.succeeded, 0);
    assert_eq!(report.failed, 1);
    assert_eq!(report.results[0].operation, "PREPARE");
}

#[tokio::test]
async fn empty_script_fails_closed_like_double_semicolon() {
    let executor = Executor::new(Box::new(MockQdrantClient::default()), Some(test_config()));
    let err = executor
        .execute("   ", OnError::Stop)
        .await
        .expect_err("empty script must fail");
    assert_eq!(err.code, "QQL-VALIDATION-EMPTY-SCRIPT");

    let err = executor
        .execute_batch(&[], OnError::Stop)
        .await
        .expect_err("empty batch must fail");
    assert_eq!(err.code, "QQL-VALIDATION-EMPTY-SCRIPT");

    // Parity: `";;"` was already a parse error.
    let err = executor.execute(";;", OnError::Stop).await.unwrap_err();
    assert!(err.code.starts_with("QQL-PARSE"), "{err:?}");
}

#[tokio::test]
async fn closed_client_fails_every_execution_entry_point() {
    let executor = Executor::new(Box::new(MockQdrantClient::default()), Some(test_config()));
    executor.close().await.expect("close is idempotent");
    assert!(executor.is_closed());
    executor.close().await.expect("close twice is fine");

    let err = executor
        .execute("QUERY VECTOR [0.1] FROM docs", OnError::Stop)
        .await
        .expect_err("execute must fail after close");
    assert_eq!(err.code, "QQL-CLIENT-CLOSED");

    let err = executor
        .execute_batch(&["QUERY VECTOR [0.1] FROM docs"], OnError::Stop)
        .await
        .expect_err("execute_batch must fail after close");
    assert_eq!(err.code, "QQL-CLIENT-CLOSED");

    let stmt = qql_core::parser::Parser::parse("QUERY VECTOR [0.1] FROM docs").unwrap();
    let err = executor
        .execute_node(stmt)
        .await
        .expect_err("execute_node must fail after close");
    assert_eq!(err.code, "QQL-CLIENT-CLOSED");
}

#[tokio::test]
async fn query_batch_failure_with_continue_retries_individually() {
    // Two same-collection QUERY statements batch into one RPC. When the batch
    // RPC fails and on_error = continue, the group is retried one-by-one so
    // the first statement's success is preserved and the failed one is
    // reported at its own index.
    let client = MockQdrantClient {
        info: Some(collection_with_vectors(&["dense"], &[])),
        ..Default::default()
    };
    *client.fail_query_batch.lock().unwrap() = true;
    // On the per-op retry, statement 1 (the second execute_planned call) fails.
    *client.fail_execute_planned_call.lock().unwrap() = 2;
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let report = executor
        .execute_batch(
            &[
                "QUERY VECTOR [0.1] FROM docs USING dense",
                "QUERY VECTOR [0.2] FROM docs USING dense",
            ],
            OnError::Continue,
        )
        .await
        .expect("continue mode must return a report");
    assert!(!report.ok);
    assert_eq!(report.succeeded, 1);
    assert_eq!(report.failed, 1);
    assert!(
        report.results[0].ok,
        "first statement must keep its success"
    );
    assert!(
        !report.results[1].ok,
        "second statement must be the failure"
    );
    assert_eq!(
        report.results.len(),
        2,
        "one response per statement, in order"
    );
}

#[tokio::test]
async fn same_collection_query_batch_yields_per_statement_hits() {
    // N1: two same-collection QUERY statements batch into ONE
    // /points/query/batch RPC. The upstream per-item shape (OpenAPI
    // QueryResponse) is {"points": [...]} — the extraction must map each
    // item to its own statement's hits instead of silently returning [].
    let client = MockQdrantClient {
        info: Some(collection_with_vectors(&["dense"], &[])),
        ..Default::default()
    };
    let executor = Executor::new(Box::new(client), Some(test_config()));

    // Per-item strict parsing (each `QueryResponse` carries `points` at its
    // top level) lives in `rest_response::tests`; here we pin the end-to-end
    // executor behavior: one response per statement, in order.
    let report = executor
        .execute_batch(
            &[
                "QUERY VECTOR [0.1] FROM docs USING dense LIMIT 2",
                "QUERY VECTOR [0.2] FROM docs USING dense LIMIT 1",
            ],
            OnError::Stop,
        )
        .await
        .expect("same-collection batch must succeed");
    assert!(report.ok, "{report:?}");
    assert_eq!(report.results.len(), 2, "one response per statement");
    for r in &report.results {
        assert!(r.ok, "every batched statement must succeed: {r:?}");
    }
}

#[tokio::test]
async fn test_explicit_batch_forces_single_group_with_header_opts() {
    let mut client = MockQdrantClient::default();
    client.info = Some(CollectionInfo::default());
    let update_calls = client.update_batch_call_count.clone();
    let last_wait = client.last_update_batch_wait.clone();

    let executor = Executor::new(Box::new(client), Some(test_config()));
    let stmts = qql_core::parser::Parser::parse_all(
        "SHOW COLLECTIONS; BATCH { DELETE FROM docs WHERE id = 1; DELETE FROM docs WHERE id = 2; } WAIT false; SHOW COLLECTIONS;",
    )
    .unwrap();
    let results = executor.execute_batch_nodes(stmts, false).await.unwrap();

    // SHOW barriers flank the block, yet members stay one forced group.
    assert_eq!(results.len(), 4, "one response per statement: {results:?}");
    assert_eq!(*update_calls.lock().unwrap(), 1, "exactly one batch RPC");
    assert_eq!(
        *last_wait.lock().unwrap(),
        Some(false),
        "header WAIT reaches the backend"
    );
    for r in &results {
        assert!(r.ok, "every statement must succeed: {r:?}");
    }
}

#[tokio::test]
async fn test_explicit_query_batch_carries_timeout() {
    let mut client = MockQdrantClient::default();
    client.info = Some(CollectionInfo::default());
    let batch_calls = client.batch_call_count.clone();
    let last_timeout = client.last_batch_timeout.clone();

    let executor = Executor::new(Box::new(client), Some(test_config()));
    let stmts = qql_core::parser::Parser::parse_all(
        "BATCH { QUERY [0.1] FROM docs LIMIT 1; QUERY [0.2] FROM docs LIMIT 1; } PARAMS (timeout = 30)",
    )
    .unwrap();
    let results = executor.execute_batch_nodes(stmts, false).await.unwrap();

    assert_eq!(results.len(), 2, "one response per member: {results:?}");
    assert_eq!(*batch_calls.lock().unwrap(), 1, "exactly one batch RPC");
    assert_eq!(
        *last_timeout.lock().unwrap(),
        Some(30),
        "header timeout reaches the backend"
    );
}

#[tokio::test]
async fn test_execute_node_batch_returns_summary() {
    let mut client = MockQdrantClient::default();
    client.info = Some(CollectionInfo::default());

    let executor = Executor::new(Box::new(client), Some(test_config()));
    let stmt = qql_core::parser::Parser::parse(
        "BATCH { DELETE FROM docs WHERE id = 1; DELETE FROM docs WHERE id = 2; }",
    )
    .unwrap();
    let resp = executor.execute_node(stmt).await.unwrap();

    assert!(resp.ok, "{resp:?}");
    assert_eq!(resp.operation, "BATCH");
    assert!(resp.message.contains("2/2"), "{resp:?}");
}
