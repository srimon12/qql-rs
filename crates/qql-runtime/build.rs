//! Build script for the `qql` runtime crate.
//!
//! Generates the Typify REST wire types from the vendored `openapi.json`
//! (written to `OUT_DIR/qdrant_types.rs`) at compile time.
use std::env;
use std::fs;
use std::path::Path;
use typify::{TypeSpace, TypeSpaceSettings};

/// Convert rustdoc intra-doc links `[`name`]` into plain code spans `` `name` ``.
///
/// Keeps prose readable while preventing broken-link errors from upstream
/// OpenAPI text that references symbols outside the generated type space.
fn neutralize_intra_doc_links(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(start) = rest.find("[`") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        if let Some(end) = after.find("`]") {
            let inner = &after[..end];
            out.push('`');
            out.push_str(inner);
            out.push('`');
            rest = &after[end + 2..];
        } else {
            out.push_str("[`");
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

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
    // ── OpenAPI types (REST) ──────────────────────────────────────

    println!("cargo:rerun-if-changed=openapi.json");
    println!("cargo:rerun-if-changed=build.rs");

    let content = fs::read_to_string("openapi.json").expect("Failed to read openapi.json");
    let mut openapi: serde_json::Value =
        serde_json::from_str(&content).expect("Invalid OpenAPI JSON");

    sanitize_schema(&mut openapi);

    let schemas = openapi["components"]["schemas"]
        .as_object_mut()
        .expect("No schemas found in OpenAPI file");

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

    if let Some(filter_schema) = schemas.get_mut("Filter") {
        if let Some(props) = filter_schema.pointer_mut("/properties") {
            if let Some(obj) = props.as_object_mut() {
                let array_schema = serde_json::json!({
                    "type": "array",
                    "items": { "$ref": "#/components/schemas/Condition" }
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

    let token_stream = type_space.to_stream();
    let file = syn::parse2(token_stream).expect("Failed to parse generated Rust tokens");
    let formatted = prettyplease::unparse(&file);
    // OpenAPI descriptions sometimes embed Markdown rustdoc links such as
    // [`init_feature_flags`] that refer to Qdrant server-private symbols. Those
    // names are not generated into this crate; neutralize them so `cargo doc
    // -D warnings` stays clean for the public `qql` API surface.
    let formatted = neutralize_intra_doc_links(&formatted);

    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("qdrant_types.rs");
    fs::write(dest_path, formatted).unwrap();

    // ── Protobuf types (gRPC) — only when grpc feature is active ──

    #[cfg(feature = "grpc")]
    {
        let proto_dir = Path::new("proto");
        println!("cargo:rerun-if-changed=proto/");

        let protoc = protoc_bin_vendored::protoc_bin_path()
            .expect("failed to locate the vendored protoc binary");
        env::set_var("PROTOC", protoc);

        tonic_prost_build::configure()
            .build_server(false)
            .build_client(true)
            .compile_protos(
                &[proto_dir.join("qdrant.proto")],
                &[proto_dir.to_path_buf()],
            )
            .expect("Failed to compile proto files");
    }
}
