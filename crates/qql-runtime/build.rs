use std::env;
use std::fs;
use std::path::Path;
use typify::{TypeSpace, TypeSpaceSettings};

fn sanitize_schema(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.remove("default");
            let is_integer = map.get("type").and_then(|t| t.as_str()) == Some("integer");
            if is_integer {
                for key in &["minimum", "maximum", "multipleOf"] {
                    if let Some(val) = map.get_mut(*key) {
                        if let Some(f) = val.as_f64() {
                            *val = serde_json::Value::Number(serde_json::Number::from(f as i64));
                        }
                    }
                }
            }
            for (_, val) in map.iter_mut() {
                sanitize_schema(val);
            }
        }
        serde_json::Value::Array(arr) => {
            for val in arr.iter_mut() {
                sanitize_schema(val);
            }
        }
        _ => {}
    }
}

fn main() {
    println!("cargo:rerun-if-changed=../../openapi.json");
    println!("cargo:rerun-if-changed=build.rs");

    let content = fs::read_to_string("../../openapi.json").expect("Failed to read openapi.json");
    let mut openapi: serde_json::Value =
        serde_json::from_str(&content).expect("Invalid OpenAPI JSON");

    // Sanitize the schema to prevent typify validation errors
    sanitize_schema(&mut openapi);

    let schemas = openapi["components"]["schemas"]
        .as_object_mut()
        .expect("No schemas found in OpenAPI file");

    // Simplify ExtendedPointId to avoid conflicting TryFrom implementations
    schemas.insert(
        "ExtendedPointId".to_string(),
        serde_json::json!({
            "type": "object",
            "properties": {
                "num": { "type": "integer", "format": "uint64" },
                "uuid": { "type": "string" }
            }
        }),
    );

    // Simplify StartFrom to avoid conflicting TryFrom implementations
    schemas.insert(
        "StartFrom".to_string(),
        serde_json::json!({
            "type": "object",
            "properties": {
                "integer": { "type": "integer", "format": "int64" },
                "double": { "type": "number", "format": "double" },
                "datetime": { "type": "string" }
            }
        }),
    );

    // Simplify Filter to avoid typify generating broken anyOf array wrappers
    if let Some(filter_schema) = schemas.get_mut("Filter") {
        if let Some(props) = filter_schema.pointer_mut("/properties") {
            if let Some(obj) = props.as_object_mut() {
                let array_schema = serde_json::json!({
                    "type": "array",
                    "items": {
                        "$ref": "#/components/schemas/Condition"
                    }
                });
                if obj.contains_key("must") {
                    obj.insert("must".to_string(), array_schema.clone());
                }
                if obj.contains_key("must_not") {
                    obj.insert("must_not".to_string(), array_schema.clone());
                }
                if obj.contains_key("should") {
                    obj.insert("should".to_string(), array_schema.clone());
                }
            }
        }
    }

    // Rename DocumentOptions to TextDocumentOptions to avoid recursive naming collision bug in typify
    if let Some(doc_options_schema) = schemas.remove("DocumentOptions") {
        schemas.insert("TextDocumentOptions".to_string(), doc_options_schema);
    }
    if let Some(document_schema) = schemas.get_mut("Document") {
        if let Some(options_schema) = document_schema.pointer_mut("/properties/options/anyOf/0") {
            if let Some(ref_val) = options_schema.get_mut("$ref") {
                if ref_val == "#/components/schemas/DocumentOptions" {
                    *ref_val = serde_json::json!("#/components/schemas/TextDocumentOptions");
                }
            }
        }
    }

    let type_defs: Vec<(String, schemars::schema::Schema)> = schemas
        .iter()
        .map(|(name, schema_val)| {
            let schema: schemars::schema::Schema = serde_json::from_value(schema_val.clone())
                .unwrap_or_else(|e| panic!("Failed to parse schema for {}: {}", name, e));
            (name.clone(), schema)
        })
        .collect();

    let mut type_space = TypeSpace::new(&TypeSpaceSettings::default());
    type_space.add_ref_types(type_defs).unwrap();

    let contents = type_space.to_stream().to_string();

    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("qdrant_types.rs");
    fs::write(dest_path, contents).unwrap();
}
