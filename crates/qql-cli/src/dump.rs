use std::error::Error;
use std::fs;
use std::path::Path;

use qql::client::ScrollPointsReq;
use qql::executor::Executor;

fn escape_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
        .replace('\0', "\\0")
}

pub fn generate_create_statement(
    collection: &str,
    hybrid: bool,
    dense_name: &str,
    sparse_name: &str,
    dense_model: &str,
    sparse_model: &str,
) -> String {
    let mut stmt = format!("CREATE COLLECTION {}", collection);
    if hybrid {
        if !dense_model.is_empty() {
            stmt.push_str(&format!(
                " HYBRID DENSE MODEL '{}'",
                escape_string(dense_model)
            ));
            if !sparse_model.is_empty() {
                stmt.push_str(&format!(" SPARSE MODEL '{}'", escape_string(sparse_model)));
            }
        } else {
            stmt.push_str(" HYBRID");
            if dense_name != "dense" || sparse_name != "sparse" {
                stmt.push_str(&format!(
                    " DENSE VECTOR '{}' SPARSE VECTOR '{}'",
                    escape_string(dense_name),
                    escape_string(sparse_name)
                ));
            }
        }
    } else if !dense_model.is_empty() {
        stmt.push_str(&format!(" USING MODEL '{}'", escape_string(dense_model)));
    } else if dense_name != "dense" && !dense_name.is_empty() {
        stmt.push_str(&format!(" VECTOR '{}'", escape_string(dense_name)));
    }
    stmt
}

pub fn build_insert_using_clause(
    hybrid: bool,
    dense_name: &str,
    sparse_name: &str,
    dense_model: &str,
    sparse_model: &str,
) -> String {
    if hybrid {
        if !dense_model.is_empty() {
            let mut parts = vec![" USING HYBRID".to_string()];
            parts.push(format!("DENSE MODEL '{}'", escape_string(dense_model)));
            if !sparse_model.is_empty() {
                parts.push(format!("SPARSE MODEL '{}'", escape_string(sparse_model)));
            }
            return parts.join(" ");
        }
        if dense_name != "dense" || sparse_name != "sparse" {
            return format!(
                " USING HYBRID DENSE VECTOR '{}' SPARSE VECTOR '{}'",
                escape_string(dense_name),
                escape_string(sparse_name)
            );
        }
        return " USING HYBRID".to_string();
    }
    if !dense_model.is_empty() {
        return format!(" USING MODEL '{}'", escape_string(dense_model));
    }
    if dense_name != "dense" && !dense_name.is_empty() {
        return format!(" USING VECTOR '{}'", escape_string(dense_name));
    }
    String::new()
}

pub async fn dump_collection(
    executor: &Executor,
    collection: &str,
    output_path: &str,
    batch_size: u32,
    dense_model: &str,
    sparse_model: &str,
) -> Result<(usize, usize), Box<dyn Error>> {
    let ops = executor.ops();

    let exists = ops.collection_exists(collection).await?;
    if !exists {
        return Err(format!("collection '{}' does not exist", collection).into());
    }

    let info = ops.get_collection_info(collection).await?;
    let hybrid = !info.schema.sparse_vectors.is_empty();
    let dense_name = info
        .schema
        .dense_vectors
        .first()
        .cloned()
        .unwrap_or_else(|| "dense".to_string());
    let sparse_name = info
        .schema
        .sparse_vectors
        .first()
        .cloned()
        .unwrap_or_else(|| "sparse".to_string());

    if let Some(parent) = Path::new(output_path).parent() {
        fs::create_dir_all(parent)?;
    }

    let mut body = String::new();
    let create_line = generate_create_statement(
        collection,
        hybrid,
        &dense_name,
        &sparse_name,
        dense_model,
        sparse_model,
    );
    body.push_str(&create_line);
    body.push_str("\n\n");

    // Extract payload schema indexes if raw_json is available
    if let Some(ref raw) = info.raw_json {
        if let Some(payload_schema) = raw.get("payload_schema").and_then(|s| s.as_object()) {
            let mut index_stmts = Vec::new();
            for (field, meta) in payload_schema {
                if let Some(data_type) = meta
                    .get("data_type")
                    .and_then(|t| t.as_str())
                    .or_else(|| meta.get("type").and_then(|t| t.as_str()))
                {
                    index_stmts.push(format!(
                        "CREATE INDEX ON COLLECTION {} FOR {} TYPE {}",
                        collection, field, data_type
                    ));
                }
            }
            if !index_stmts.is_empty() {
                index_stmts.sort();
                body.push_str(&index_stmts.join("\n"));
                body.push_str("\n\n");
            }
        }
    }

    let mut written = 0;
    let mut skipped = 0;
    let mut after = None;

    loop {
        let (points, next_after) = ops
            .scroll(ScrollPointsReq {
                collection_name: collection.to_string(),
                limit: batch_size as u64,
                filter: None,
                after: after.clone(),
            })
            .await?;

        if points.is_empty() {
            break;
        }

        let mut batch_records = Vec::new();
        for point in &points {
            let payload = match &point.payload {
                Some(p) => p,
                None => {
                    skipped += 1;
                    continue;
                }
            };

            let mut record = serde_json::Map::new();
            record.insert("id".to_string(), serde_json::to_value(&point.id)?);

            for (k, v) in payload {
                record.insert(k.clone(), v.clone());
            }

            if let Some(ref vec_val) = point.vector {
                if let Some(obj) = vec_val.as_object() {
                    for (vname, vdata) in obj {
                        let key = format!("_v_{}", vname.replace('_', "__"));
                        record.insert(key, vdata.clone());
                    }
                } else {
                    record.insert("_v".to_string(), vec_val.clone());
                }
            }

            batch_records.push(record);
        }

        if !batch_records.is_empty() {
            body.push_str(&format!("INSERT INTO {} VALUES\n", collection));
            for (idx, rec) in batch_records.iter().enumerate() {
                let rec_json =
                    serde_json::to_string_pretty(&serde_json::Value::Object(rec.clone()))?;
                body.push_str("  ");
                body.push_str(&rec_json.replace('\n', "\n  "));
                if idx + 1 < batch_records.len() {
                    body.push(',');
                }
                body.push('\n');
                written += 1;
            }
            body.push_str(&build_insert_using_clause(
                hybrid,
                &dense_name,
                &sparse_name,
                dense_model,
                sparse_model,
            ));
            body.push_str("\n\n");
        }

        if next_after.is_none() {
            break;
        }
        after = next_after;
    }

    let header = format!("-- QQL dump for {}\n-- Points: {}\n\n", collection, written);
    let footer = format!("-- Written: {}\n-- Skipped: {}\n", written, skipped);
    let final_output = format!("{}{}{}", header, body, footer);

    fs::write(output_path, final_output)?;
    Ok((written, skipped))
}
