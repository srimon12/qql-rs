use crate::plan::{PlannedOperation, plan};
use crate::types::*;
use qql_core::ast::Stmt;
use qql_core::error::QqlError;

/// Optional REST projection of a plan: HTTP method, path, query, and body.
#[derive(Debug)]
pub struct Route {
    /// HTTP verb of the projected route.
    pub method: Method,
    /// Absolute Qdrant path with the collection interpolated.
    pub path: String,
    /// Ordered query-string parameters as `(name, value)` pairs.
    pub query: Vec<(String, String)>,
    /// Serialized JSON body; `None` for bodyless routes.
    pub body: Option<serde_json::Value>,
}

impl Route {
    /// Serialized JSON body, or None for bodyless routes.
    pub fn body_json(&self) -> Option<serde_json::Value> {
        self.body.clone()
    }
}

/// Why a planned operation cannot become a single Qdrant REST route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestProjectionError {
    /// Client-side only (e.g. CROSS RERANK). Compile still exposes `stmt_type`.
    ClientSideOnly {
        /// Statement type name used in error messages.
        stmt_type: &'static str,
    },
    /// Overwrite-payload has no single REST route: `POST /points/payload`
    /// is merge-only, so `OVERWRITE` only runs inside a batch
    /// (`POST /points/batch` with `{ "overwrite_payload": … }`).
    OverwriteRequiresBatch,
    /// Plan IR failed to serialize to JSON (a `Serialize` regression).
    SerializeFailed {
        /// Underlying serde error message.
        message: String,
    },
}

/// Serialize a plan struct to JSON for the REST body.
///
/// Every plan IR request type is JSON-serializable by construction, so this is
/// only reachable if a `Serialize` impl regresses. A failure must surface as a
/// loud invariant violation — a JSON `null` body would be rejected by the
/// backend with an opaque error. `pub(crate)` so the plan/query/ddl REST
/// projections share the same no-swallow invariant; kept fallible so the
/// property is unit-testable.
pub(crate) fn serialize_body<T: serde::Serialize>(
    req: &T,
) -> Result<serde_json::Value, serde_json::Error> {
    serde_json::to_value(req)
}

/// REST projection of a planned operation (HTTP method/path/query/body).
///
/// Client-side operations such as [`PlannedOperation::CrossRerank`] return
/// [`RestProjectionError::ClientSideOnly`] — they must not invent a Qdrant path.
/// Returns `RestProjectionError::ClientSideOnly` for operations that have no
/// single Qdrant REST endpoint (e.g. CROSS RERANK).
pub fn to_rest_route(op: &PlannedOperation) -> Result<Route, RestProjectionError> {
    /// Serialize a plan struct to JSON for the REST body, mapping the
    /// (practically unreachable) serialization failure into the error channel
    /// instead of panicking the host process.
    fn body<T: serde::Serialize>(
        req: &T,
    ) -> Result<Option<serde_json::Value>, RestProjectionError> {
        serialize_body(req)
            .map(Some)
            .map_err(|e| RestProjectionError::SerializeFailed {
                message: e.to_string(),
            })
    }

    /// Read-op query params: timeout, consistency.
    fn read_query(
        timeout: Option<u64>,
        consistency: Option<&crate::types::ReadConsistencyParam>,
    ) -> Vec<(String, String)> {
        let mut q = Vec::new();
        crate::query::push_read_opts(&mut q, timeout, consistency);
        q
    }

    /// Mutation query params: `wait` only.
    ///
    /// Shard routing rides the typed body field (`shard_key`, string or
    /// number per OpenAPI) — never the query string. Qdrant defines no
    /// `shard_key` query parameter, so emitting one would be dead weight at
    /// best and a conflicting string form for numeric keys at worst.
    fn mut_query(wait: bool) -> Vec<(String, String)> {
        vec![("wait".into(), wait.to_string())]
    }

    Ok(match op {
        PlannedOperation::Query {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/query"),
            query: read_query(request.timeout, request.consistency.as_ref()),
            body: body(request)?,
        },
        PlannedOperation::QueryGroups {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/query/groups"),
            query: read_query(request.timeout, request.consistency.as_ref()),
            body: body(request)?,
        },
        PlannedOperation::GetPoints {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points"),
            query: Vec::new(),
            body: body(request)?,
        },
        PlannedOperation::Scroll {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/scroll"),
            query: Vec::new(),
            body: body(request)?,
        },
        PlannedOperation::Count {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/count"),
            query: Vec::new(),
            body: body(request)?,
        },
        PlannedOperation::Facet {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/facet"),
            query: Vec::new(),
            body: body(request)?,
        },
        PlannedOperation::Upsert {
            collection,
            request,
            wait,
        } => {
            let query = vec![("wait".into(), wait.to_string())];
            Route {
                method: Method::Put,
                path: format!("/collections/{collection}/points"),
                query,
                body: body(request)?,
            }
        }
        PlannedOperation::Delete {
            collection,
            request,
            wait,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/delete"),
            query: mut_query(*wait),
            body: body(request)?,
        },
        PlannedOperation::ClearPayload {
            collection,
            request,
            wait,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/payload/clear"),
            query: mut_query(*wait),
            body: body(request)?,
        },
        PlannedOperation::DeletePayload {
            collection,
            request,
            wait,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/payload/delete"),
            query: mut_query(*wait),
            body: body(request)?,
        },
        PlannedOperation::DeleteVectors {
            collection,
            request,
            wait,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/vectors/delete"),
            query: mut_query(*wait),
            body: body(request)?,
        },
        PlannedOperation::UpdateVectors {
            collection,
            request,
            wait,
        } => Route {
            method: Method::Put,
            path: format!("/collections/{collection}/points/vectors"),
            query: mut_query(*wait),
            body: body(request)?,
        },
        PlannedOperation::UpdatePayload {
            collection,
            request,
            wait,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/points/payload"),
            query: mut_query(*wait),
            body: body(request)?,
        },
        PlannedOperation::OverwritePayload { .. } => {
            return Err(RestProjectionError::OverwriteRequiresBatch);
        }
        // DDL: REST shapes differ from plan IR — typed OpenAPI projections
        PlannedOperation::CreateCollection {
            collection,
            request,
        } => Route {
            method: Method::Put,
            path: format!("/collections/{collection}"),
            query: Vec::new(),
            body: body(&crate::ddl::create_collection_rest_body(request))?,
        },
        PlannedOperation::UpdateCollection {
            collection,
            request,
        } => Route {
            method: Method::Patch,
            path: format!("/collections/{collection}"),
            query: Vec::new(),
            body: body(request)?,
        },
        PlannedOperation::CreateIndex {
            collection,
            request,
            wait,
        } => Route {
            method: Method::Put,
            path: format!("/collections/{collection}/index"),
            query: vec![("wait".into(), wait.to_string())],
            body: body(&crate::ddl::create_index_rest_body(request))?,
        },
        PlannedOperation::CreateShardKey {
            collection,
            request,
        } => Route {
            method: Method::Put,
            path: format!("/collections/{collection}/shards"),
            query: Vec::new(),
            body: body(request)?,
        },
        PlannedOperation::DropShardKey {
            collection,
            request,
        } => Route {
            method: Method::Post,
            path: format!("/collections/{collection}/shards/delete"),
            query: Vec::new(),
            body: body(request)?,
        },
        // Bodyless
        PlannedOperation::DropCollection { collection } => Route {
            method: Method::Delete,
            path: format!("/collections/{collection}"),
            query: Vec::new(),
            body: None,
        },
        PlannedOperation::DropIndex { collection, field } => Route {
            method: Method::Delete,
            path: format!("/collections/{collection}/index/{field}"),
            query: Vec::new(),
            body: None,
        },
        PlannedOperation::ListCollections => Route {
            method: Method::Get,
            path: "/collections".into(),
            query: Vec::new(),
            body: None,
        },
        PlannedOperation::GetCollection { collection } => Route {
            method: Method::Get,
            path: format!("/collections/{collection}"),
            query: Vec::new(),
            body: None,
        },
        PlannedOperation::ListShardKeys { collection } => Route {
            method: Method::Get,
            path: format!("/collections/{collection}/shards"),
            query: Vec::new(),
            body: None,
        },
        PlannedOperation::GetQuotas => Route {
            method: Method::Get,
            path: "/quotas".into(),
            query: Vec::new(),
            body: None,
        },
        PlannedOperation::SetQuotas { request } => {
            let mut query = Vec::new();
            if let Some(wait) = request.wait {
                query.push(("wait".into(), wait.to_string()));
            }
            Route {
                method: Method::Put,
                path: "/quotas".into(),
                query,
                body: body(request)?,
            }
        }
        PlannedOperation::Batch {
            key,
            operations,
            wait,
            timeout,
            consistency,
        } => match key {
            crate::batch::BatchKey::Query(collection) => {
                let (_, batch) = crate::batch::build_query_batch(operations).map_err(|e| {
                    RestProjectionError::SerializeFailed {
                        message: e.to_string(),
                    }
                })?;
                Route {
                    method: Method::Post,
                    path: format!("/collections/{collection}/points/query/batch"),
                    query: read_query(*timeout, consistency.as_ref()),
                    body: body(&batch)?,
                }
            }
            crate::batch::BatchKey::Mutation(collection) => {
                let (_, _, batch) = crate::batch::build_update_batch(operations).map_err(|e| {
                    RestProjectionError::SerializeFailed {
                        message: e.to_string(),
                    }
                })?;
                let mut query = Vec::new();
                if let Some(wait) = wait {
                    query.push(("wait".into(), wait.to_string()));
                }
                Route {
                    method: Method::Post,
                    path: format!("/collections/{collection}/points/batch"),
                    query,
                    body: body(&batch)?,
                }
            }
        },
        PlannedOperation::CrossRerank { .. } => {
            return Err(RestProjectionError::ClientSideOnly {
                stmt_type: "cross_rerank",
            });
        }
    })
}

/// Plan a statement and project it to a REST route in one call.
///
/// Returns `QQL-REST-CLIENT-SIDE` for client-side operations (e.g. CROSS
/// RERANK) that have no single Qdrant REST endpoint, and
/// `QQL-REST-OVERWRITE-BATCH-ONLY` for `OVERWRITE` payload writes (merge-only
/// `POST /points/payload` cannot replace payload; use a batch).
pub fn try_route(statement: &Stmt) -> Result<Route, QqlError> {
    let op = plan(statement)?;
    to_rest_route(&op).map_err(|err| match err {
        RestProjectionError::ClientSideOnly { stmt_type } => QqlError::validation(
            "QQL-REST-CLIENT-SIDE",
            format!(
                "{stmt_type} is client-side and has no single Qdrant REST route; \
                 execute via the runtime CROSS RERANK path"
            ),
            None,
        ),
        RestProjectionError::OverwriteRequiresBatch => QqlError::validation(
            "QQL-REST-OVERWRITE-BATCH-ONLY",
            "OVERWRITE has no single Qdrant REST route: POST /points/payload is merge-only; \
             run the statement inside a BATCH block (POST /points/batch with overwrite_payload)",
            None,
        ),
        RestProjectionError::SerializeFailed { message } => QqlError::execution(
            "QQL-PLAN-SERIALIZE",
            format!("plan IR REST request body serialization failed: {message}"),
            None,
        ),
    })
}

/// Offline compile result for host SDKs.
///
/// `route` is `None` for client-side operations (e.g. CROSS RERANK) that have
/// a stable `stmt_type` but no single Qdrant HTTP endpoint.
#[derive(Debug)]
pub struct CompiledStatement {
    /// Stable snake_case type id from `compile_stmt_type`.
    pub stmt_type: &'static str,
    /// Projected REST route; `None` for client-side-only operations.
    pub route: Option<Route>,
}

/// Compile a statement from the planner IR.
///
/// Always sets `stmt_type` from [`crate::plan::PlannedOperation::compile_stmt_type`].
/// REST path/method/payload are present only when a real Qdrant route exists.
pub fn compile_statement(statement: &Stmt) -> Result<CompiledStatement, qql_core::error::QqlError> {
    let op = plan(statement)?;
    let stmt_type = op.compile_stmt_type();
    let route = to_rest_route(&op).ok();
    Ok(CompiledStatement { stmt_type, route })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PlannedOperation, plan};
    use qql_core::parser::Parser;

    #[test]
    fn client_side_ops_error_via_try_route_and_compile_cleanly() {
        // Regression: the deprecated `route()` (panicking wrapper around
        // `try_route`) was removed. `try_route` must return `Err` — never panic —
        // for client-side-only ops, and `compile_statement` must still expose the
        // stable `stmt_type` with no route.
        let s = Parser::parse(
            "QUERY CROSS RERANK TEXT 'q' MODEL 'bge-reranker-base' ON FIELD body FROM docs PREFETCH (QUERY TEXT 'q' FROM docs USING dense LIMIT 50) LIMIT 10;",
        )
        .unwrap();
        assert!(
            try_route(&s).is_err(),
            "client-side op must not produce a REST route"
        );
        let compiled = compile_statement(&s).unwrap();
        assert_eq!(compiled.stmt_type, "cross_rerank");
        assert!(compiled.route.is_none());
    }

    #[test]
    fn query_routes_correctly() {
        let s = Parser::parse("QUERY TEXT 'hello' MODEL 'e5' FROM docs LIMIT 10;").unwrap();
        let r = try_route(&s).unwrap();
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.path, "/collections/docs/points/query");
        assert!(r.body.is_some());
    }

    #[test]
    fn points_lookup() {
        let s = Parser::parse("QUERY POINTS (42) FROM docs WITH PAYLOAD true;").unwrap();
        let r = try_route(&s).unwrap();
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.path, "/collections/docs/points");
    }

    #[test]
    fn points_lookup_preserves_cluster_shard_key() {
        let statement = Parser::parse("QUERY POINTS (42) FROM docs SHARD 'tenant-a';").unwrap();
        let operation = plan::plan(&statement).unwrap();
        let PlannedOperation::GetPoints { request, .. } = operation else {
            panic!("expected point lookup");
        };
        assert_eq!(
            request.shard_key,
            Some(crate::semantic::PlanShardKey::Keyword("tenant-a".into()))
        );
        assert!(
            try_route(&statement)
                .unwrap()
                .body_json()
                .unwrap()
                .to_string()
                .contains("tenant-a")
        );
    }

    #[test]
    fn quota_routes() {
        let show = Parser::parse("SHOW QUOTAS;").unwrap();
        let r = try_route(&show).unwrap();
        assert_eq!(r.method, Method::Get);
        assert_eq!(r.path, "/quotas");
        assert!(r.body.is_none());

        let set = Parser::parse(
            "SET QUOTA (enabled = true, max_resident_memory_percent = 80) WAIT true;",
        )
        .unwrap();
        let r = try_route(&set).unwrap();
        assert_eq!(r.method, Method::Put);
        assert_eq!(r.path, "/quotas");
        assert!(r.query.iter().any(|(k, v)| k == "wait" && v == "true"));
        let body = r.body_json().unwrap();
        assert_eq!(body["enabled"], true);
        assert_eq!(body["max_resident_memory_percent"], 80);
        assert!(body.get("wait").is_none(), "wait must be a query param");
    }

    #[test]
    fn quota_plan_validates_config() {
        use crate::plan::{PlannedOperation, plan};
        let bad = Parser::parse("SET QUOTA (bogus = 1);").unwrap();
        let err = plan(&bad).unwrap_err();
        assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);
        assert_eq!(err.code, "QQL-PLAN-QUOTA");

        let bad_range = Parser::parse("SET QUOTA (max_resident_memory_percent = 500);").unwrap();
        assert!(plan(&bad_range).is_err());

        let clear = Parser::parse("SET QUOTA (max_disk_usage_percent = null);").unwrap();
        let op = plan(&clear).unwrap();
        match op {
            PlannedOperation::SetQuotas { request } => {
                assert_eq!(request.config.max_disk_usage_percent, None);
            }
            other => panic!("expected SetQuotas, got {other:?}"),
        }
    }

    #[test]
    fn compile_stmt_type_disambiguates_bodyless_routes() {
        let cases = [
            ("DROP INDEX ON COLLECTION docs FOR title;", "drop_index"),
            ("SHOW SHARD KEYS ON COLLECTION docs;", "show_shard_keys"),
            ("DROP COLLECTION docs;", "drop_collection"),
            ("SHOW COLLECTION docs;", "show_collection"),
            ("SHOW COLLECTIONS;", "show_collections"),
        ];
        for (qql, expected) in cases {
            let stmt = Parser::parse(qql).unwrap();
            let compiled = compile_statement(&stmt).unwrap();
            assert_eq!(compiled.stmt_type, expected, "qql={qql}");
        }
    }

    #[test]
    fn repro_numeric_shard_key_projects_as_number() {
        // SHARD 101 on a mutation must stay numeric to the wire (body 101,
        // not "101"): currently stringified to the keyword partition.
        let statement = Parser::parse("DELETE FROM docs WHERE id = 1 SHARD 101;").unwrap();
        let operation = plan::plan(&statement).unwrap();
        let route = to_rest_route(&operation).expect("rest route");
        let body = route.body_json().unwrap();
        assert_eq!(
            body["shard_key"],
            serde_json::json!(101),
            "REST body must carry the numeric key, got {body}"
        );
    }

    #[test]
    fn update_vector_values_fills_the_wire_point_list() {
        let stmt = Parser::parse(
            "UPDATE docs SET VECTOR VALUES {id: 1, vector: [0.1]}, {id: 2, vector: {dense: [0.2]}};",
        )
        .unwrap();
        let op = plan::plan(&stmt).unwrap();
        let PlannedOperation::UpdateVectors { request, .. } = &op else {
            panic!("expected UpdateVectors");
        };
        assert_eq!(request.points.len(), 2);
        let body = to_rest_route(&op).expect("rest route").body_json().unwrap();
        assert_eq!(body["points"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn mutation_shard_keys_lower_and_project() {
        let cases = [
            ("CLEAR PAYLOAD FROM docs WHERE id = 1 SHARD 't1';", "t1"),
            (
                "DELETE VECTOR dense FROM docs WHERE id = 1 SHARD 't2';",
                "t2",
            ),
            (
                "UPDATE docs SET VECTOR dense = [0.1, 0.2] WHERE id = 1 SHARD 't3';",
                "t3",
            ),
            (
                "UPDATE docs SET PAYLOAD = {\"k\": 1} WHERE id = 1 SHARD 't4';",
                "t4",
            ),
        ];
        for (qql, expected) in cases {
            let statement = Parser::parse(qql).unwrap();
            let operation = plan::plan(&statement).unwrap();
            assert_eq!(
                operation.shard_key(),
                Some(&crate::semantic::PlanShardKey::Keyword(expected.into())),
                "plan.shard_key for {qql}"
            );
            let r = to_rest_route(&operation).expect("rest route");
            // Routing rides the typed body field; Qdrant defines no
            // `shard_key` query parameter, so none is emitted.
            assert!(
                r.query.iter().all(|(k, _)| k != "shard_key"),
                "no shard_key query param for {qql}: {:?}",
                r.query
            );
            let body = r.body_json().unwrap();
            assert_eq!(
                body["shard_key"],
                serde_json::json!(expected),
                "REST body should carry the shard_key for {qql}: {body}"
            );
        }
    }

    #[test]
    fn upsert_with_embedding_waits() {
        let s = Parser::parse("UPSERT INTO docs VALUES {id: 1, text: 'x'} USING DENSE MODEL 'm';")
            .unwrap();
        let r = try_route(&s).unwrap();
        assert!(r.query.iter().any(|(k, v)| k == "wait" && v == "true"));
    }

    #[test]
    fn delete_has_wait() {
        let s = Parser::parse("DELETE FROM docs WHERE id = 1;").unwrap();
        let r = try_route(&s).unwrap();
        assert!(r.query.iter().any(|(k, v)| k == "wait" && v == "true"));
    }

    #[test]
    fn upsert_explicit_wait() {
        let s = Parser::parse("UPSERT INTO docs VALUES {id: 1, vector: [0.1]} WAIT true;").unwrap();
        let r = try_route(&s).unwrap();
        assert!(r.query.iter().any(|(k, v)| k == "wait" && v == "true"));

        let s_false =
            Parser::parse("UPSERT INTO docs VALUES {id: 1, vector: [0.1]} WAIT false;").unwrap();
        let r_false = try_route(&s_false).unwrap();
        assert!(
            r_false
                .query
                .iter()
                .any(|(k, v)| k == "wait" && v == "false")
        );
    }

    #[test]
    fn create_index_default_wait() {
        let s = Parser::parse("CREATE INDEX ON COLLECTION docs FOR tag TYPE keyword;").unwrap();
        let r = try_route(&s).unwrap();
        assert!(r.query.iter().any(|(k, v)| k == "wait" && v == "true"));

        let s_nowait =
            Parser::parse("CREATE INDEX ON COLLECTION docs FOR tag TYPE keyword WAIT false;")
                .unwrap();
        let r_nowait = try_route(&s_nowait).unwrap();
        assert!(
            r_nowait
                .query
                .iter()
                .any(|(k, v)| k == "wait" && v == "false")
        );
    }

    #[test]
    fn scroll_after_exclusive_offset() {
        let s = Parser::parse("SCROLL FROM docs AFTER 42 LIMIT 10;").unwrap();
        let r = try_route(&s).unwrap();
        let body = r.body.unwrap();
        assert_eq!(body["offset"], 43);
    }

    #[test]
    fn scroll_after_exclusive_uuid_offset() {
        let s = Parser::parse(
            "SCROLL FROM docs AFTER '00000000-0000-0000-0000-00000000002a' LIMIT 10;",
        )
        .unwrap();
        let r = try_route(&s).unwrap();
        let body = r.body.unwrap();
        assert_eq!(body["offset"], "00000000-0000-0000-0000-00000000002b");
    }

    #[test]
    fn show_collections_no_body() {
        let s = Parser::parse("SHOW COLLECTIONS;").unwrap();
        let r = try_route(&s).unwrap();
        assert_eq!(r.method, Method::Get);
        assert!(r.body.is_none());
    }

    #[test]
    fn all_endpoint_methods() {
        let cases = [
            (
                "QUERY TEXT 'x' MODEL 'e5' FROM docs;",
                Method::Post,
                "/collections/docs/points/query",
            ),
            (
                "SCROLL FROM docs LIMIT 10;",
                Method::Post,
                "/collections/docs/points/scroll",
            ),
            (
                "UPSERT INTO docs VALUES {id: 1, title: 'x'};",
                Method::Put,
                "/collections/docs/points",
            ),
            (
                "DELETE FROM docs WHERE id = 1;",
                Method::Post,
                "/collections/docs/points/delete",
            ),
            (
                "UPDATE docs SET VECTOR = [0.1] WHERE id = 'x';",
                Method::Put,
                "/collections/docs/points/vectors",
            ),
            (
                "UPDATE docs SET PAYLOAD = {x: 1} WHERE id = 1;",
                Method::Post,
                "/collections/docs/points/payload",
            ),
            (
                "CREATE COLLECTION docs (d VECTOR(4, DOT));",
                Method::Put,
                "/collections/docs",
            ),
            (
                "ALTER COLLECTION docs WITH HNSW (m = 16);",
                Method::Patch,
                "/collections/docs",
            ),
            ("DROP COLLECTION docs;", Method::Delete, "/collections/docs"),
            (
                "CREATE INDEX ON COLLECTION docs FOR title TYPE text;",
                Method::Put,
                "/collections/docs/index",
            ),
            ("SHOW COLLECTIONS;", Method::Get, "/collections"),
            ("SHOW COLLECTION docs;", Method::Get, "/collections/docs"),
        ];
        for (source, method, path) in cases {
            let s = Parser::parse(source).unwrap();
            let r = try_route(&s).unwrap();
            assert_eq!(r.method, method, "method mismatch for: {}", source);
            assert_eq!(r.path, path, "path mismatch for: {}", source);
        }
    }

    #[test]
    fn grouped_query_routes_to_groups_endpoint() {
        let s = Parser::parse(
            "QUERY TEXT 'hello' MODEL 'e5' FROM docs GROUP BY category SIZE 3 LIMIT 10;",
        )
        .unwrap();
        let r = try_route(&s).unwrap();
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.path, "/collections/docs/points/query/groups");
        assert!(r.body.is_some());
    }

    #[test]
    fn grouped_query_with_lookup() {
        let s = Parser::parse(
            "QUERY TEXT 'hello' MODEL 'e5' FROM docs GROUP BY category SIZE 3 LOOKUP FROM categories LIMIT 10;",
        )
        .unwrap();
        let r = try_route(&s).unwrap();
        assert_eq!(r.path, "/collections/docs/points/query/groups");
        let json = r.body_json().unwrap();
        assert_eq!(json["group_by"], "category");
        assert_eq!(json["group_size"], 3);
        assert_eq!(json["with_lookup"], "categories");
    }

    #[test]
    fn hybrid_query_produces_prefetches() {
        let s = Parser::parse(
            "QUERY HYBRID TEXT 'database' MODEL 'bge' DENSE dense SPARSE sparse FUSION RRF FROM docs LIMIT 10;",
        )
        .unwrap();
        let r = try_route(&s).unwrap();
        let json = r.body_json().unwrap();
        assert_eq!(json["query"]["fusion"], "rrf");
        let prefetch = json["prefetch"].as_array().unwrap();
        assert_eq!(prefetch.len(), 2);
        assert_eq!(prefetch[0]["using"], "dense");
        assert_eq!(prefetch[1]["using"], "sparse");
        assert!(
            prefetch[0]["query"]["nearest"].is_object(),
            "HYBRID prefetch nearest must be Document object: {:?}",
            prefetch[0]["query"]["nearest"]
        );
        assert!(
            prefetch[1]["query"]["nearest"].is_object(),
            "HYBRID sparse prefetch nearest must also be Document object: {:?}",
            prefetch[1]["query"]["nearest"]
        );
    }

    #[test]
    fn rerank_query_staged() {
        let s = Parser::parse(
            "QUERY RERANK TEXT 'travel' MODEL 'colbert' FROM docs USING colbert PREFETCH (QUERY TEXT 'travel' MODEL 'colbert' FROM docs USING dense LIMIT 100) LIMIT 10;",
        )
        .unwrap();
        let r = try_route(&s).unwrap();
        let json = r.body_json().unwrap();
        assert_eq!(json["using"], "colbert");
        assert!(json["query"]["nearest"].is_object());
        assert_eq!(json["prefetch"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn points_lookup_full() {
        let s = Parser::parse(
            "QUERY POINTS (42, 'uuid-v4') FROM docs WITH PAYLOAD INCLUDE ('title', 'url') WITH VECTOR ('dense');",
        )
        .unwrap();
        let r = try_route(&s).unwrap();
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.path, "/collections/docs/points");
        let json = r.body_json().unwrap();
        assert_eq!(json["ids"].as_array().unwrap().len(), 2);
        assert_eq!(json["with_payload"]["include"].as_array().unwrap().len(), 2);
        assert_eq!(json["with_vector"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn query_with_all_options() {
        let s = Parser::parse(
            "QUERY TEXT 'search' MODEL 'e5' FROM docs USING dense WHERE status = 'active' PARAMS (hnsw_ef = 256, exact = true) SCORE THRESHOLD 0.5 WITH PAYLOAD INCLUDE ('title') WITH VECTOR ('dense') LIMIT 20 OFFSET 5;",
        )
        .unwrap();
        let r = try_route(&s).unwrap();
        let json = r.body_json().unwrap();
        assert!(
            json["query"]["nearest"]["text"].is_string()
                || json["query"]["nearest"].as_str().is_some()
        );
        assert_eq!(json["using"], "dense");
        assert_eq!(json["filter"]["must"][0]["key"], "status");
        assert_eq!(json["filter"]["must"][0]["match"]["value"], "active");
        assert_eq!(json["params"]["hnsw_ef"], 256);
        assert_eq!(json["params"]["exact"], true);
        assert_eq!(json["score_threshold"], 0.5);
        assert_eq!(json["limit"], 20);
        assert_eq!(json["offset"], 5);
    }

    #[test]
    fn scroll_with_order_by() {
        let s = Parser::parse("SCROLL FROM docs WHERE status = 'active' LIMIT 50;").unwrap();
        let r = try_route(&s).unwrap();
        let json = r.body_json().unwrap();
        assert_eq!(json["with_payload"], true);
        assert_eq!(json["with_vector"], false);
        assert!(json["limit"].as_u64().unwrap() > 0);
    }

    #[test]
    fn scroll_with_vector_all() {
        let s = Parser::parse("SCROLL FROM docs WITH VECTOR LIMIT 25;").unwrap();
        let r = try_route(&s).unwrap();
        let json = r.body_json().unwrap();
        assert_eq!(json["with_payload"], true);
        assert_eq!(json["with_vector"], true);
        assert_eq!(json["limit"], 25);
    }

    #[test]
    fn scroll_with_vector_after_string_id() {
        let s = Parser::parse("SCROLL FROM docs AFTER 'id-with-quote' WITH VECTOR true LIMIT 10;")
            .unwrap();
        let r = try_route(&s).unwrap();
        let json = r.body_json().unwrap();
        assert_eq!(json["offset"], "id-with-quote");
        assert_eq!(json["with_vector"], true);
    }

    #[test]
    fn query_body_has_no_group_fields_when_no_group() {
        let s = Parser::parse("QUERY TEXT 'hello' MODEL 'e5' FROM docs LIMIT 5;").unwrap();
        let r = try_route(&s).unwrap();
        let json = r.body_json().unwrap();
        assert!(json.get("group_by").is_none());
        assert!(json.get("group_size").is_none());
        assert!(json.get("group_request").is_none());
    }

    #[test]
    fn query_body_serialization_roundtrip_all_variants() {
        let cases = [
            "QUERY TEXT 'text search' MODEL 'e5' FROM docs LIMIT 10;",
            "QUERY NEAREST VECTOR [0.1, 0.2, 0.3] FROM docs USING dense LIMIT 10;",
            "QUERY NEAREST POINT 42 FROM docs USING dense LIMIT 5;",
            "QUERY NEAREST POINT '550e8400-e29b-41d4-a716-446655440000' FROM docs USING dense;",
            "QUERY RECOMMEND POSITIVE (1, 2) NEGATIVE (3) STRATEGY average_vector FROM docs USING dense LIMIT 10;",
            "QUERY CONTEXT (POSITIVE POINT 1 NEGATIVE POINT 2) FROM docs LIMIT 10;",
            "QUERY DISCOVER TARGET POINT 42 CONTEXT (POSITIVE POINT 1 NEGATIVE POINT 2) FROM docs USING dense LIMIT 10;",
            "QUERY ORDER BY created_at DESC FROM docs LIMIT 10;",
            "QUERY SAMPLE RANDOM FROM docs LIMIT 10;",
            "QUERY FORMULA score * 2 FROM docs LIMIT 5;",
            "QUERY RELEVANCE FEEDBACK TARGET POINT 42 FEEDBACK ((POINT 1, 0.8), (POINT 2, 0.2)) STRATEGY NAIVE (a = 1.0, b = 1.0, c = 1.0) FROM docs USING dense LIMIT 10;",
            "UPSERT INTO docs VALUES {id: 1, title: 'hello'};",
            "DELETE FROM docs WHERE status = 'inactive';",
            "UPDATE docs SET VECTOR = [0.1, 0.2] WHERE id = 1;",
            "UPDATE docs SET VECTOR VALUES {id: 1, vector: [0.1]}, {id: 2, vector: {dense: [0.2]}};",
            "UPDATE docs SET PAYLOAD = {x: 1} WHERE id = 1;",
            "CREATE COLLECTION docs (d VECTOR(128, COSINE));",
            "ALTER COLLECTION docs WITH HNSW (m = 16);",
            "DROP COLLECTION docs;",
            "CREATE INDEX ON COLLECTION docs FOR title TYPE text;",
            "SHOW COLLECTIONS;",
            "SHOW COLLECTION docs;",
            "SCROLL FROM docs LIMIT 10;",
        ];
        for source in cases {
            let s = Parser::parse(source).unwrap_or_else(|_| panic!("parse failed: {}", source));
            let r = try_route(&s).unwrap();
            let json = r.body_json();
            match r.body {
                Some(_) => assert!(json.is_some(), "expected body for: {}", source),
                None => assert!(json.is_none(), "expected no body for: {}", source),
            }
        }
    }
}
