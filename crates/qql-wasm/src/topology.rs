//! Named-vector topology from a collection `result` object.
//!
//! Derives the `USING` resolution view from the shared REST schema reader
//! ([`qql_plan::surface::schema::schema_from_rest_result`]) instead of
//! re-parsing the config JSON here.

use qql_embed::TopologyNames;
use qql_plan::surface::schema::schema_from_rest_result;

/// Dense / sparse / multivector names reported by `GET /collections/{name}`.
pub(crate) fn vector_names_from_collection_result(result: &serde_json::Value) -> TopologyNames {
    let schema = schema_from_rest_result(result);
    let multivector = schema
        .vectors
        .iter()
        .filter(|spec| spec.multivector.is_some())
        .filter_map(|spec| spec.name.clone())
        .collect();
    let mut sparse: Vec<String> = schema
        .sparse_vectors
        .into_iter()
        .map(|spec| spec.name)
        .collect();
    sparse.sort();
    TopologyNames {
        dense: schema.dense_vectors,
        sparse,
        multivector,
    }
}
