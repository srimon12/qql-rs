use crate::filter::value_to_json;
use crate::types::*;
use qql_core::ast::{AlterCollectionStmt, CreateCollectionStmt, CreateIndexStmt, VectorDistance};

pub fn lower_create_collection(stmt: &CreateCollectionStmt) -> CreateCollectionRequest {
    let mut req = CreateCollectionRequest {
        vectors: None,
        sparse_vectors: None,
        hnsw_config: None,
        optimizers_config: None,
        params: None,
        quantization_config: None,
        vectors_config: None,
        shard_number: None,
        sharding_method: None,
        shard_keys: None,
    };

    let mut vectors = serde_json::Map::new();
    for vd in &stmt.vectors {
        let mut v = serde_json::Map::new();
        v.insert("size".into(), serde_json::Value::from(vd.size));
        v.insert(
            "distance".into(),
            serde_json::Value::String(distance_str(vd.distance)),
        );
        if let Some(ref hnsw) = vd.hnsw {
            v.insert("hnsw_config".into(), lower_hnsw_config_val(hnsw));
        }
        if let Some(ref quant) = vd.quantization {
            v.insert(
                "quantization_config".into(),
                lower_quantization_config_val(quant),
            );
        }
        if let Some(ref mv) = vd.multivector {
            let comparator = match mv.comparator {
                qql_core::ast::MultivectorComparator::MaxSim => "max_sim",
            };
            v.insert(
                "multivector_config".into(),
                serde_json::json!({"comparator": comparator}),
            );
        }
        if let Some(ref vec_cfg) = vd.vectors {
            if let Some(on_disk) = vec_cfg.on_disk {
                v.insert("on_disk".into(), serde_json::Value::Bool(on_disk));
            }
        }
        vectors.insert(vd.name.clone(), serde_json::Value::Object(v));
    }
    if !vectors.is_empty() {
        req.vectors = Some(vectors);
    }

    let mut sparse = serde_json::Map::new();
    for sv in &stmt.sparse_vectors {
        let mut opts = serde_json::Map::new();
        if let Some(ref modifier) = sv.modifier {
            opts.insert(
                "modifier".into(),
                serde_json::Value::String(modifier.clone()),
            );
        } else {
            opts.insert("modifier".into(), serde_json::json!("idf"));
        }
        if let Some(ref idx) = sv.index {
            let mut idx_map = serde_json::Map::new();
            if let Some(fst) = idx.full_scan_threshold {
                idx_map.insert("full_scan_threshold".into(), serde_json::Value::from(fst));
            }
            if let Some(od) = idx.on_disk {
                idx_map.insert("on_disk".into(), serde_json::Value::Bool(od));
            }
            if let Some(ref dt) = idx.datatype {
                idx_map.insert("datatype".into(), serde_json::Value::String(dt.clone()));
            }
            if !idx_map.is_empty() {
                opts.insert("index".into(), serde_json::Value::Object(idx_map));
            }
        }
        sparse.insert(sv.name.clone(), serde_json::Value::Object(opts));
    }
    if !sparse.is_empty() {
        req.sparse_vectors = Some(sparse);
    }

    if let Some(ref config) = stmt.config {
        fill_collection_config(&mut req, config);
    }

    if let Some(on_disk) = req
        .vectors_config
        .take()
        .and_then(|config| config.get("on_disk").and_then(serde_json::Value::as_bool))
    {
        if let Some(vectors) = &mut req.vectors {
            for vector in vectors.values_mut() {
                if let Some(vector) = vector.as_object_mut() {
                    vector.insert("on_disk".into(), serde_json::Value::Bool(on_disk));
                }
            }
        }
    }

    req
}

pub fn lower_alter_collection(stmt: &AlterCollectionStmt) -> UpdateCollectionRequest {
    let mut req = UpdateCollectionRequest {
        hnsw_config: None,
        optimizers_config: None,
        params: None,
        quantization_config: None,
    };
    if let Some(ref config) = stmt.config {
        fill_update_collection_config(&mut req, config);
    }
    req
}

pub fn lower_create_index(stmt: &CreateIndexStmt) -> CreateIndexRequest {
    let mut extra = serde_json::Map::new();
    for (key, value) in &stmt.options {
        extra.insert(key.clone(), value_to_json(value));
    }
    CreateIndexRequest {
        field_name: stmt.field.clone(),
        field_schema: stmt.field_type.clone(),
        extra,
    }
}

fn fill_collection_config(
    req: &mut CreateCollectionRequest,
    config: &qql_core::ast::CollectionConfig,
) {
    if let Some(ref v) = config.vectors {
        let mut vc = serde_json::Map::new();
        if let Some(on_disk) = v.on_disk {
            vc.insert("on_disk".into(), serde_json::Value::Bool(on_disk));
        }
        if !vc.is_empty() {
            req.vectors_config = Some(serde_json::Value::Object(vc));
        }
    }
    if let Some(ref h) = config.hnsw {
        req.hnsw_config = Some(lower_hnsw_config_val(h));
    }
    if let Some(ref o) = config.optimizers {
        req.optimizers_config = Some(lower_optimizers_config_val(o));
    }
    if let Some(ref p) = config.params {
        let mut pc = serde_json::Map::new();
        if let Some(rf) = p.replication_factor {
            pc.insert("replication_factor".into(), serde_json::Value::from(rf));
        }
        if let Some(wc) = p.write_consistency_factor {
            pc.insert(
                "write_consistency_factor".into(),
                serde_json::Value::from(wc),
            );
        }
        if let Some(rf) = p.read_fan_out_factor {
            pc.insert("read_fan_out_factor".into(), serde_json::Value::from(rf));
        }
        if let Some(rd) = p.read_fan_out_delay_ms {
            pc.insert("read_fan_out_delay_ms".into(), serde_json::Value::from(rd));
        }
        if let Some(od) = p.on_disk_payload {
            pc.insert("on_disk_payload".into(), serde_json::Value::Bool(od));
        }
        if !pc.is_empty() {
            req.params = Some(serde_json::Value::Object(pc));
        }
        if let Some(sn) = p.shard_number {
            req.shard_number = Some(sn);
        }
        req.sharding_method = p.sharding_method.clone();
        req.shard_keys = p.shard_keys.clone();
    }
    if let Some(ref q) = config.quantization {
        req.quantization_config = Some(lower_quantization_config_val(q));
    }
    if let Some(ref qu) = config.quantization_update {
        let mut qup = serde_json::Map::new();
        qup.insert("disabled".into(), serde_json::Value::Bool(qu.disabled));
        if let Some(ref qc) = qu.config {
            qup.insert(
                "quantization_config".into(),
                lower_quantization_config_val(qc),
            );
        }
        req.quantization_config = Some(serde_json::Value::Object(qup));
    }
}

fn fill_update_collection_config(
    req: &mut UpdateCollectionRequest,
    config: &qql_core::ast::CollectionConfig,
) {
    if let Some(ref h) = config.hnsw {
        req.hnsw_config = Some(lower_hnsw_config_val(h));
    }
    if let Some(ref o) = config.optimizers {
        req.optimizers_config = Some(lower_optimizers_config_val(o));
    }
    if let Some(ref p) = config.params {
        let mut pc = serde_json::Map::new();
        if let Some(rf) = p.replication_factor {
            pc.insert("replication_factor".into(), serde_json::Value::from(rf));
        }
        if let Some(wc) = p.write_consistency_factor {
            pc.insert(
                "write_consistency_factor".into(),
                serde_json::Value::from(wc),
            );
        }
        if let Some(rf) = p.read_fan_out_factor {
            pc.insert("read_fan_out_factor".into(), serde_json::Value::from(rf));
        }
        if let Some(rd) = p.read_fan_out_delay_ms {
            pc.insert("read_fan_out_delay_ms".into(), serde_json::Value::from(rd));
        }
        if let Some(od) = p.on_disk_payload {
            pc.insert("on_disk_payload".into(), serde_json::Value::Bool(od));
        }
        if !pc.is_empty() {
            req.params = Some(serde_json::Value::Object(pc));
        }
    }
    if let Some(ref q) = config.quantization {
        req.quantization_config = Some(lower_quantization_config_val(q));
    }
    if let Some(ref qu) = config.quantization_update {
        let mut qup = serde_json::Map::new();
        qup.insert("disabled".into(), serde_json::Value::Bool(qu.disabled));
        if let Some(ref qc) = qu.config {
            qup.insert(
                "quantization_config".into(),
                lower_quantization_config_val(qc),
            );
        }
        req.quantization_config = Some(serde_json::Value::Object(qup));
    }
}

pub fn lower_hnsw_config_val(config: &qql_core::ast::HnswRuntimeConfig) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    if let Some(m) = config.m {
        obj.insert("m".into(), serde_json::Value::from(m));
    }
    if let Some(ef) = config.ef_construct {
        obj.insert("ef_construct".into(), serde_json::Value::from(ef));
    }
    if let Some(fst) = config.full_scan_threshold {
        obj.insert("full_scan_threshold".into(), serde_json::Value::from(fst));
    }
    if let Some(mit) = config.max_indexing_threads {
        obj.insert("max_indexing_threads".into(), serde_json::Value::from(mit));
    }
    if let Some(od) = config.on_disk {
        obj.insert("on_disk".into(), serde_json::Value::Bool(od));
    }
    if let Some(pm) = config.payload_m {
        obj.insert("payload_m".into(), serde_json::Value::from(pm));
    }
    if let Some(inline) = config.inline_storage {
        obj.insert("inline_storage".into(), serde_json::Value::Bool(inline));
    }
    serde_json::Value::Object(obj)
}

pub fn lower_optimizers_config_val(
    config: &qql_core::ast::OptimizersRuntimeConfig,
) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    if let Some(dt) = config.deleted_threshold {
        obj.insert("deleted_threshold".into(), serde_json::Value::from(dt));
    }
    if let Some(vmvn) = config.vacuum_min_vector_number {
        obj.insert(
            "vacuum_min_vector_number".into(),
            serde_json::Value::from(vmvn),
        );
    }
    if let Some(dsn) = config.default_segment_number {
        obj.insert(
            "default_segment_number".into(),
            serde_json::Value::from(dsn),
        );
    }
    if let Some(mss) = config.max_segment_size {
        obj.insert("max_segment_size".into(), serde_json::Value::from(mss));
    }
    if let Some(mt) = config.memmap_threshold {
        obj.insert("memmap_threshold".into(), serde_json::Value::from(mt));
    }
    if let Some(it) = config.indexing_threshold {
        obj.insert("indexing_threshold".into(), serde_json::Value::from(it));
    }
    if let Some(fi) = config.flush_interval_sec {
        obj.insert("flush_interval_sec".into(), serde_json::Value::from(fi));
    }
    if let Some(ref mot) = config.max_optimization_threads {
        if mot.auto_ {
            obj.insert(
                "max_optimization_threads".into(),
                serde_json::Value::String("auto".into()),
            );
        } else {
            obj.insert(
                "max_optimization_threads".into(),
                serde_json::Value::from(mot.value),
            );
        }
    }
    if let Some(pu) = config.prevent_unoptimized {
        obj.insert("prevent_unoptimized".into(), serde_json::Value::Bool(pu));
    }
    serde_json::Value::Object(obj)
}

pub fn lower_quantization_config_val(
    config: &qql_core::ast::QuantizationConfig,
) -> serde_json::Value {
    let qtype = match config.qtype {
        qql_core::ast::QuantizationType::Scalar => "scalar",
        qql_core::ast::QuantizationType::Binary => "binary",
        qql_core::ast::QuantizationType::Product => "product",
        qql_core::ast::QuantizationType::Turbo => "turbo",
    };
    let mut obj = serde_json::Map::new();
    obj.insert("type".into(), serde_json::Value::String(qtype.into()));
    obj.insert(
        "always_ram".into(),
        serde_json::Value::Bool(config.always_ram),
    );
    if let Some(quantile) = config.quantile {
        obj.insert("quantile".into(), serde_json::Value::from(quantile));
    }
    if let Some(turbo_bits) = config.turbo_bits {
        // Emit both keys: gRPC/plan use turbo_bits; REST/OpenAPI turbo config uses bits.
        obj.insert("turbo_bits".into(), serde_json::Value::from(turbo_bits));
        obj.insert("bits".into(), serde_json::Value::from(turbo_bits));
    }
    if let Some(ref compression) = config.compression {
        obj.insert(
            "compression".into(),
            serde_json::Value::String(compression.clone()),
        );
    }
    if let Some(ref encoding) = config.encoding {
        obj.insert(
            "encoding".into(),
            serde_json::Value::String(encoding.clone()),
        );
    }
    if let Some(ref query_encoding) = config.query_encoding {
        obj.insert(
            "query_encoding".into(),
            serde_json::Value::String(query_encoding.clone()),
        );
    }
    serde_json::Value::Object(obj)
}

fn distance_str(d: VectorDistance) -> String {
    match d {
        VectorDistance::Cosine => "Cosine".into(),
        VectorDistance::Dot => "Dot".into(),
        VectorDistance::Euclid => "Euclid".into(),
        VectorDistance::Manhattan => "Manhattan".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qql_core::ast::Stmt;
    use qql_core::parser::Parser;

    fn parse_stmt(s: &str) -> Stmt {
        Parser::parse(s).expect("parse failed")
    }

    #[test]
    fn create_collection_dense() {
        let stmt = parse_stmt("CREATE COLLECTION docs (dense VECTOR(384, COSINE));");
        let Stmt::CreateCollection(ref cc) = stmt else {
            panic!()
        };
        let req = lower_create_collection(cc);
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["vectors"]["dense"]["size"], 384);
        assert_eq!(json["vectors"]["dense"]["distance"], "Cosine");
    }

    #[test]
    fn create_collection_with_config() {
        let stmt =
            parse_stmt("CREATE COLLECTION docs (dense VECTOR(128, EUCLID)) WITH HNSW (m = 16);");
        let Stmt::CreateCollection(ref cc) = stmt else {
            panic!()
        };
        let req = lower_create_collection(cc);
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["hnsw_config"]["m"], 16);
    }

    #[test]
    fn create_index() {
        let stmt = parse_stmt(
            "CREATE INDEX ON COLLECTION docs FOR title TYPE text WITH (lowercase = true);",
        );
        let Stmt::CreateIndex(ref ci) = stmt else {
            panic!()
        };
        let req = lower_create_index(ci);
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["field_name"], "title");
        assert_eq!(json["field_schema"], "text");
        assert_eq!(json["lowercase"], true);
    }

    #[test]
    fn lower_product_quantization_includes_compression() {
        let stmt = parse_stmt(
            "CREATE COLLECTION docs (v VECTOR(128, COSINE) WITH QUANTIZATION (type = 'product', compression = 'x16', always_ram = true));",
        );
        let Stmt::CreateCollection(ref cc) = stmt else {
            panic!()
        };
        let req = lower_create_collection(cc);
        let json = serde_json::to_value(&req).unwrap();
        let quant = &json["vectors"]["v"]["quantization_config"];
        assert_eq!(quant["type"], "product");
        assert_eq!(quant["compression"], "x16");
        assert_eq!(quant["always_ram"], true);
    }

    #[test]
    fn lower_binary_quantization_includes_encoding() {
        let stmt = parse_stmt(
            "CREATE COLLECTION docs (v VECTOR(128, COSINE) WITH QUANTIZATION (type = 'binary', encoding = 'two_bits', always_ram = true));",
        );
        let Stmt::CreateCollection(ref cc) = stmt else {
            panic!()
        };
        let req = lower_create_collection(cc);
        let json = serde_json::to_value(&req).unwrap();
        let quant = &json["vectors"]["v"]["quantization_config"];
        assert_eq!(quant["type"], "binary");
        assert_eq!(quant["encoding"], "two_bits");
        assert_eq!(quant["always_ram"], true);
    }

    #[test]
    fn lower_turbo_quantization_includes_bits() {
        let stmt = parse_stmt(
            "CREATE COLLECTION docs (v VECTOR(128, COSINE) WITH QUANTIZATION (type = 'turbo', bits = 1.5, always_ram = true));",
        );
        let Stmt::CreateCollection(ref cc) = stmt else {
            panic!()
        };
        let req = lower_create_collection(cc);
        let json = serde_json::to_value(&req).unwrap();
        let quant = &json["vectors"]["v"]["quantization_config"];
        assert_eq!(quant["type"], "turbo");
        assert_eq!(quant["bits"], 1.5);
        assert_eq!(quant["turbo_bits"], 1.5);
    }

    #[test]
    fn lower_vector_on_disk_and_query_encoding_and_multivector() {
        let stmt = parse_stmt(
            "CREATE COLLECTION docs (v VECTOR(64, COSINE) WITH MULTIVECTOR (comparator = 'max_sim') WITH VECTORS (on_disk = true) WITH QUANTIZATION (type = 'binary', encoding = 'two_bits', query_encoding = 'scalar4bits', always_ram = true));",
        );
        let Stmt::CreateCollection(ref cc) = stmt else {
            panic!()
        };
        let req = lower_create_collection(cc);
        let json = serde_json::to_value(&req).unwrap();
        let v = &json["vectors"]["v"];
        assert_eq!(v["on_disk"], true);
        assert_eq!(v["multivector_config"]["comparator"], "max_sim");
        assert_eq!(v["quantization_config"]["encoding"], "two_bits");
        assert_eq!(v["quantization_config"]["query_encoding"], "scalar4bits");
    }

    #[test]
    fn lower_optimizers_auto_threads() {
        let stmt = parse_stmt(
            "CREATE COLLECTION docs (v VECTOR(8, COSINE)) WITH OPTIMIZERS (max_optimization_threads = 'auto', indexing_threshold = 1000);",
        );
        let Stmt::CreateCollection(ref cc) = stmt else {
            panic!()
        };
        let req = lower_create_collection(cc);
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(
            json["optimizers_config"]["max_optimization_threads"],
            "auto"
        );
        assert_eq!(json["optimizers_config"]["indexing_threshold"], 1000);
    }

    #[test]
    fn lower_sparse_vector_full_config_and_sharding_method() {
        let stmt = parse_stmt(
            "CREATE COLLECTION docs (bm25 SPARSE WITH SPARSE (modifier = 'idf', full_scan_threshold = 10000, on_disk = true, datatype = 'float32')) WITH PARAMS (sharding_method = 'custom', shard_number = 2);",
        );
        let Stmt::CreateCollection(ref cc) = stmt else {
            panic!()
        };
        let req = lower_create_collection(cc);
        let json = serde_json::to_value(&req).unwrap();
        let sparse = &json["sparse_vectors"]["bm25"];
        assert_eq!(sparse["modifier"], "idf");
        assert_eq!(sparse["index"]["full_scan_threshold"], 10000);
        assert_eq!(sparse["index"]["on_disk"], true);
        assert_eq!(sparse["index"]["datatype"], "float32");
        assert_eq!(json["sharding_method"], "custom");
        assert_eq!(json["shard_number"], 2);
    }
}
