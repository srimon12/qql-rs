//! DDL REST bodies and gRPC IR parity.

use super::{openapi_or_skip, validate_ref};
use qql_core::parser::Parser;
use qql_plan::plan::{plan, to_rest_route};

#[test]
fn ddl_create_and_update_rest_bodies_match_openapi() {
    let Some(openapi) = openapi_or_skip() else {
        return;
    };

    let create = Parser::parse(
            "CREATE COLLECTION docs (dense VECTOR(384, COSINE) WITH QUANTIZATION (type = 'scalar', quantile = 0.99, always_ram = true), sparse SPARSE) \
             WITH HNSW (m = 16, ef_construct = 100) \
             WITH OPTIMIZERS (indexing_threshold = 20000, max_optimization_threads = 'auto') \
             WITH PARAMS (replication_factor = 2, write_consistency_factor = 1, on_disk_payload = true, shard_number = 2, sharding_method = 'custom');",
        )
        .unwrap();
    let op = plan(&create).unwrap();
    let body = to_rest_route(&op).expect("rest route").body_json().unwrap();
    assert!(body.get("params").is_none());
    assert!(body.get("shard_keys").is_none());
    assert_eq!(body["replication_factor"], 2);
    assert_eq!(
        body["vectors"]["dense"]["quantization_config"]["scalar"]["type"],
        "int8"
    );
    assert_eq!(
        body["optimizers_config"]["max_optimization_threads"],
        "auto"
    );
    validate_ref(&openapi, "CreateCollection", &body);

    // Per-vector product/binary/turbo nest correctly for OpenAPI VectorParams.
    let create2 = Parser::parse(
            "CREATE COLLECTION docs (v VECTOR(64, COSINE) WITH QUANTIZATION (type = 'product', compression = 'x16', always_ram = true) WITH MULTIVECTOR (comparator = 'max_sim'));",
        )
        .unwrap();
    let body2 = to_rest_route(&plan(&create2).unwrap())
        .expect("rest")
        .body_json()
        .unwrap();
    assert_eq!(
        body2["vectors"]["v"]["quantization_config"]["product"]["compression"],
        "x16"
    );
    assert_eq!(
        body2["vectors"]["v"]["multivector_config"]["comparator"],
        "max_sim"
    );
    validate_ref(&openapi, "CreateCollection", &body2);

    let alter = Parser::parse(
            "ALTER COLLECTION docs WITH HNSW (ef_construct = 200) WITH PARAMS (replication_factor = 3) WITH QUANTIZATION (type = 'binary', encoding = 'two_bits') \
             WITH VECTOR dense (HNSW (m = 32, memory = 'cold'), QUANTIZATION (type = 'scalar', quantile = 0.99), VECTOR (memory = 'cached', on_disk = true)) \
             WITH VECTOR colbert (QUANTIZATION (disabled = true)) \
             WITH SPARSE bm25 (SPARSE (modifier = 'idf', full_scan_threshold = 5000, memory = 'pinned', datatype = 'float16'));",
        )
        .unwrap();
    let alter_body = to_rest_route(&plan(&alter).unwrap())
        .expect("rest")
        .body_json()
        .unwrap();
    assert_eq!(alter_body["params"]["replication_factor"], 3);
    assert_eq!(
        alter_body["quantization_config"]["binary"]["encoding"],
        "two_bits"
    );
    assert_eq!(alter_body["vectors"]["dense"]["hnsw_config"]["m"], 32);
    assert_eq!(alter_body["vectors"]["dense"]["memory"], "cached");
    assert_eq!(
        alter_body["vectors"]["dense"]["quantization_config"]["scalar"]["quantile"],
        0.99
    );
    assert_eq!(
        alter_body["vectors"]["colbert"]["quantization_config"],
        "Disabled"
    );
    assert_eq!(alter_body["sparse_vectors"]["bm25"]["modifier"], "idf");
    assert_eq!(
        alter_body["sparse_vectors"]["bm25"]["index"]["full_scan_threshold"],
        5000
    );
    validate_ref(&openapi, "UpdateCollection", &alter_body);

    // Unnamed/default-vector form: the empty key on the PATCH `vectors` map.
    let unnamed = Parser::parse("ALTER COLLECTION docs WITH VECTOR (on_disk = true);").unwrap();
    let unnamed_body = to_rest_route(&plan(&unnamed).unwrap())
        .expect("rest")
        .body_json()
        .unwrap();
    assert_eq!(unnamed_body["vectors"][""]["on_disk"], true);
    validate_ref(&openapi, "UpdateCollection", &unnamed_body);

    let disable =
        Parser::parse("ALTER COLLECTION docs WITH QUANTIZATION (disabled = true);").unwrap();
    let disable_body = to_rest_route(&plan(&disable).unwrap())
        .expect("rest")
        .body_json()
        .unwrap();
    assert_eq!(disable_body["quantization_config"], "Disabled");
    validate_ref(&openapi, "UpdateCollection", &disable_body);
}

#[test]
fn ddl_create_index_rest_nests_field_schema() {
    let Some(openapi) = openapi_or_skip() else {
        return;
    };
    let stmt = Parser::parse(
            "CREATE INDEX ON COLLECTION docs FOR title TYPE text WITH (lowercase = true, tokenizer = 'word', min_token_len = 2);",
        )
        .unwrap();
    let body = to_rest_route(&plan(&stmt).unwrap())
        .expect("rest")
        .body_json()
        .unwrap();
    assert_eq!(body["field_name"], "title");
    assert_eq!(body["field_schema"]["type"], "text");
    assert_eq!(body["field_schema"]["lowercase"], true);
    assert_eq!(body["field_schema"]["tokenizer"], "word");
    assert!(body.get("lowercase").is_none());
    validate_ref(&openapi, "CreateFieldIndex", &body);
}

/// gRPC create maps the same plan IR fields as REST OpenAPI projection covers.
#[test]
fn ddl_create_grpc_reads_same_ir_as_rest_projection() {
    use qql_core::ast::Stmt;
    use qql_plan::ddl::{create_collection_rest_body, lower_create_collection};

    let stmt = Parser::parse(
            "CREATE COLLECTION docs (v VECTOR(128, COSINE) WITH QUANTIZATION (type = 'scalar', quantile = 0.95, always_ram = true) WITH HNSW (m = 24) WITH MULTIVECTOR (comparator = 'max_sim')) \
             WITH OPTIMIZERS (indexing_threshold = 1000) \
             WITH PARAMS (replication_factor = 2, write_consistency_factor = 1, on_disk_payload = false);",
        )
        .unwrap();
    let Stmt::CreateCollection(cc) = stmt else {
        panic!()
    };
    let req = lower_create_collection(&cc);
    let rest = serde_json::to_value(create_collection_rest_body(&req)).unwrap();
    assert_eq!(rest["replication_factor"], 2);
    assert_eq!(
        rest["vectors"]["v"]["quantization_config"]["scalar"]["quantile"],
        0.95
    );

    // gRPC converters consume the same typed IR
    let qql_plan::DenseVectorsConfig::Named(map) = req.vectors.as_ref().unwrap() else {
        panic!("expected named vectors");
    };
    let vp = crate::grpc_route::test_api_ddl::vector_params(map.get("v").unwrap());
    assert_eq!(vp.size, 128);
    assert!(vp.hnsw_config.is_some());
    assert!(vp.quantization_config.is_some());
    assert!(vp.multivector_config.is_some());
    assert!(req.optimizers_config.is_some());
}
