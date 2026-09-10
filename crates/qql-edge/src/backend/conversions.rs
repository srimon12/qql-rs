use std::collections::{BTreeMap, HashMap};

use qdrant_edge::{PointId, Record};

use qql::executor::{FacetHit, GroupedSearchResult, SearchHit};
use qql_core::error::QqlError;
use qql_plan::{PlanFacetValue, PlanGroupId, PlanPointId, PlanVectorStruct, PlanVectorValue};

pub(crate) fn to_edge_id(id: impl IntoPlanPointId) -> Result<PointId, QqlError> {
    match id.into_plan_point_id() {
        PlanPointId::Number(n) => Ok(PointId::NumId(n)),
        PlanPointId::String(s) => {
            // qdrant-edge PointId is NumId | Uuid only — bare strings like "doc-1"
            // are not representable. Fail loudly instead of silently dropping the
            // id (the previous filter_map(...ok()) path deleted half a batch).
            uuid::Uuid::parse_str(&s).map(PointId::Uuid).map_err(|e| {
                QqlError::execution(
                    "QQL-EDGE-INVALID-POINT-ID",
                    format!(
                        "invalid point id '{s}': edge mode accepts unsigned integers or UUIDs only ({e})"
                    ),
                    None,
                )
            })
        }
    }
}

/// Convert a list of plan point IDs, propagating the first conversion error.
pub(crate) fn to_edge_ids<I, T>(ids: I) -> Result<Vec<PointId>, QqlError>
where
    I: IntoIterator<Item = T>,
    T: IntoPlanPointId,
{
    ids.into_iter().map(to_edge_id).collect()
}

/// Accept typed plan point IDs (owned or borrowed).
pub(crate) trait IntoPlanPointId {
    fn into_plan_point_id(self) -> PlanPointId;
}

impl IntoPlanPointId for PlanPointId {
    fn into_plan_point_id(self) -> PlanPointId {
        self
    }
}

impl IntoPlanPointId for &PlanPointId {
    fn into_plan_point_id(self) -> PlanPointId {
        self.clone()
    }
}

/// Typed plan ID from a `qdrant-edge` point ID (no JSON hop).
pub(crate) fn from_edge_plan_id(id: &PointId) -> PlanPointId {
    match id {
        PointId::NumId(n) => PlanPointId::Number(*n),
        PointId::Uuid(u) => PlanPointId::String(u.to_string()),
    }
}

fn from_edge_payload(payload: qdrant_edge::Payload) -> HashMap<String, serde_json::Value> {
    payload.0.into_iter().collect()
}

/// Typed search hit from a scored query point.
pub(crate) fn from_edge_scored_point_to_hit(point: qdrant_edge::ScoredPoint) -> SearchHit {
    SearchHit {
        id: from_edge_plan_id(&point.id),
        score: point.score,
        payload: point.payload.map(from_edge_payload),
        collection: None,
        vector: point.vector.map(edge_vector_to_typed),
    }
}

/// Typed search hit from a scroll/retrieve record. Records carry no similarity
/// score, so `score` is `0.0`.
pub(crate) fn from_edge_record_to_hit(record: Record) -> SearchHit {
    SearchHit {
        id: from_edge_plan_id(&record.id),
        score: 0.0,
        payload: record.payload.map(from_edge_payload),
        collection: None,
        vector: record.vector.map(edge_vector_to_typed),
    }
}

/// Typed grouped result from a `qdrant-edge` [`Group`](qdrant_edge::Group).
///
/// The engine's `GroupId` enum is public only through `Group::key` (its module
/// is private), so the crate's own `From<GroupId> for serde_json::Value` impl
/// is the only lossless way to read the key; one scalar hop, not an envelope.
pub(crate) fn from_edge_group_to_typed(
    group: qdrant_edge::Group,
) -> Result<GroupedSearchResult, QqlError> {
    Ok(GroupedSearchResult {
        group_id: plan_group_id_from_edge(group.key)?,
        hits: group
            .hits
            .into_iter()
            .map(from_edge_scored_point_to_hit)
            .collect(),
    })
}

/// Engine group key (string / u64 / i64) → typed [`PlanGroupId`].
fn plan_group_id_from_edge(key: impl Into<serde_json::Value>) -> Result<PlanGroupId, QqlError> {
    match key.into() {
        serde_json::Value::String(keyword) => Ok(PlanGroupId::Keyword(keyword)),
        serde_json::Value::Number(number) => number
            .as_u64()
            .map(PlanGroupId::Unsigned)
            .or_else(|| number.as_i64().map(PlanGroupId::Signed))
            .ok_or_else(|| group_key_error(&number)),
        other => Err(group_key_error(&other)),
    }
}

/// A `GroupId` can only be a string, `u64`, or `i64`; anything else means the
/// engine returned a key outside its own public contract.
fn group_key_error(key: impl std::fmt::Display) -> QqlError {
    QqlError::execution(
        "QQL-EDGE-TYPE",
        format!("qdrant-edge returned an unrepresentable group key: {key}"),
        None,
    )
    .with_field("operation", "query")
}

/// Replace group-hit payloads/vectors with records fetched under the caller's
/// selectors.
///
/// qdrant-edge's grouping driver shapes its candidate requests with only the
/// `with_payload` selector `Fields([group_by])`, so distilled hits carry no
/// other payload. Qdrant's server path hydrates the same way
/// (`Group::hydrate_from`); `records` must come from a `retrieve` issued with
/// the plan's `with_payload` / `with_vector` selectors.
pub(crate) fn hydrate_edge_groups(groups: &mut [qdrant_edge::Group], records: Vec<Record>) {
    let by_id: HashMap<PointId, Record> = records
        .into_iter()
        .map(|record| (record.id, record))
        .collect();
    for group in groups {
        for hit in &mut group.hits {
            if let Some(record) = by_id.get(&hit.id) {
                hit.payload.clone_from(&record.payload);
                hit.vector.clone_from(&record.vector);
            }
        }
    }
}

/// Typed facet entry from a `qdrant-edge` facet hit. Edge UUID facet values
/// serialize as strings on the REST surface, so they become keywords here.
pub(crate) fn from_edge_facet_hit(hit: qdrant_edge::FacetValueHit) -> FacetHit {
    FacetHit {
        value: match hit.value {
            qdrant_edge::FacetValue::Keyword(keyword) => PlanFacetValue::Keyword(keyword),
            qdrant_edge::FacetValue::Int(integer) => PlanFacetValue::Integer(integer),
            qdrant_edge::FacetValue::Uuid(uuid) => {
                PlanFacetValue::Keyword(uuid::Uuid::from_u128(uuid).to_string())
            }
            qdrant_edge::FacetValue::Bool(flag) => PlanFacetValue::Bool(flag),
        },
        count: hit.count as u64,
    }
}

/// One `qdrant-edge` vector value → typed plan vector value.
fn edge_vector_value_to_typed(vector: qdrant_edge::VectorInternal) -> PlanVectorValue {
    match vector {
        qdrant_edge::VectorInternal::Dense(values) => PlanVectorValue::Dense(values),
        qdrant_edge::VectorInternal::Sparse(sparse) => PlanVectorValue::Sparse {
            indices: sparse.indices,
            values: sparse.values,
        },
        qdrant_edge::VectorInternal::MultiDense(multi) => {
            PlanVectorValue::MultiDense(multi.into_multi_vectors())
        }
    }
}

/// `qdrant-edge` vector struct → typed [`PlanVectorStruct`] (single or named).
fn edge_vector_to_typed(vector: qdrant_edge::VectorStructInternal) -> PlanVectorStruct {
    match vector {
        qdrant_edge::VectorStructInternal::Single(values) => {
            PlanVectorStruct::Single(PlanVectorValue::Dense(values))
        }
        qdrant_edge::VectorStructInternal::MultiDense(multi) => {
            PlanVectorStruct::Single(PlanVectorValue::MultiDense(multi.into_multi_vectors()))
        }
        qdrant_edge::VectorStructInternal::Named(vectors) => PlanVectorStruct::Named(
            vectors
                .into_iter()
                .map(|(name, vector)| (name.to_string(), edge_vector_value_to_typed(vector)))
                .collect::<BTreeMap<_, _>>(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qql_plan::PlanPointId;

    /// Build an engine group. `GroupId` is unnameable outside qdrant-edge, so
    /// the key is deserialized into the struct field's inferred type.
    fn edge_group(
        key: serde_json::Value,
        hits: Vec<qdrant_edge::ScoredPoint>,
    ) -> qdrant_edge::Group {
        qdrant_edge::Group {
            key: serde_json::from_value(key).expect("group key"),
            hits,
        }
    }

    fn edge_hit(id: u64, score: f32) -> qdrant_edge::ScoredPoint {
        qdrant_edge::ScoredPoint {
            id: PointId::NumId(id),
            version: 0,
            score,
            payload: None,
            vector: None,
            shard_key: None,
            order_value: None,
        }
    }

    fn payload(
        entries: impl IntoIterator<Item = (&'static str, serde_json::Value)>,
    ) -> qdrant_edge::Payload {
        qdrant_edge::Payload(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
        )
    }

    #[test]
    fn group_keys_map_to_plan_group_ids() {
        let cases = [
            (serde_json::json!("NYC"), PlanGroupId::Keyword("NYC".into())),
            (serde_json::json!(7u64), PlanGroupId::Unsigned(7)),
            (serde_json::json!(-3), PlanGroupId::Signed(-3)),
        ];
        for (key, expected) in cases {
            let typed = from_edge_group_to_typed(edge_group(key.clone(), vec![edge_hit(1, 0.5)]))
                .unwrap_or_else(|e| panic!("group {key}: {e}"));
            assert_eq!(typed.group_id, expected);
            assert_eq!(typed.hits.len(), 1);
            assert_eq!(typed.hits[0].id, PlanPointId::Number(1));
            assert_eq!(typed.hits[0].score, 0.5);
        }
    }

    #[test]
    fn hydrate_replaces_payload_and_leaves_missing_hits_alone() {
        let mut groups = vec![edge_group(
            serde_json::json!("NYC"),
            vec![edge_hit(1, 0.9), edge_hit(2, 0.8)],
        )];
        let records = vec![Record {
            id: PointId::NumId(1),
            payload: Some(payload([
                ("district", serde_json::json!("NYC")),
                ("price", serde_json::json!(10)),
            ])),
            vector: None,
            shard_key: None,
            order_value: None,
        }];
        hydrate_edge_groups(&mut groups, records);

        let hit = &groups[0].hits[0];
        let payload = hit.payload.as_ref().expect("hydrated payload");
        assert_eq!(payload.0.get("price"), Some(&serde_json::json!(10)));
        // No record for id 2: the hit keeps whatever it had (none here).
        assert!(groups[0].hits[1].payload.is_none());
    }

    #[test]
    fn unrepresentable_group_key_reports_type_error() {
        let error = plan_group_id_from_edge(serde_json::json!([1, 2])).unwrap_err();
        assert_eq!(error.code, "QQL-EDGE-TYPE");
        assert!(error.message.contains("unrepresentable group key"));
    }
}
