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
        if vd.multivector.is_some() {
            v.insert(
                "multivector_config".into(),
                serde_json::json!({"comparator": "max_sim"}),
            );
        }
        vectors.insert(vd.name.clone(), serde_json::Value::Object(v));
    }
    if !vectors.is_empty() {
        req.vectors = Some(vectors);
    }

    let mut sparse = serde_json::Map::new();
    for sv in &stmt.sparse_vectors {
        sparse.insert(sv.name.clone(), serde_json::json!({"modifier": "idf"}));
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

pub fn lower_alter_collection(stmt: &AlterCollectionStmt) -> CreateCollectionRequest {
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
    if let Some(ref config) = stmt.config {
        fill_collection_config(&mut req, config);
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
        obj.insert(
            "max_optimization_threads".into(),
            serde_json::Value::from(mot.value),
        );
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
        obj.insert("turbo_bits".into(), serde_json::Value::from(turbo_bits));
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
}
