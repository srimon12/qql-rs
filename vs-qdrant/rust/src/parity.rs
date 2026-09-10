//! Typed parity projection shared by both contenders.
//!
//! Scenario functions return each SDK's **native typed results** — the timed
//! path never serializes. For the untimed parity assertions the harness
//! projects both sides onto [`HitView`] (id + score); payload equality is
//! proven separately by the raw-REST probe after ingest.

use qdrant_client::qdrant::ScoredPoint;
use qql::executor::SearchHit;
use qql_plan::PlanPointId;

/// One compared hit: point id plus score.
#[derive(Debug, Clone, PartialEq)]
pub struct HitView {
    /// Numeric point id (the corpus uses integer ids).
    pub id: u64,
    /// Score widened from the SDK's f32 for tolerance comparison.
    pub score: f64,
}

/// Project the official `qdrant-client` typed hits.
pub fn official_hits(points: &[ScoredPoint]) -> Vec<HitView> {
    points
        .iter()
        .map(|p| HitView {
            id: crate::official::point_id_num(&p.id),
            score: f64::from(p.score),
        })
        .collect()
}

/// Project QQL's typed hits.
pub fn qql_hits(hits: &[SearchHit]) -> Vec<HitView> {
    hits.iter()
        .map(|h| HitView {
            id: plan_id_num(&h.id),
            score: f64::from(h.score),
        })
        .collect()
}

/// Numeric id from a typed QQL point id (string ids parse; the corpus is
/// numeric, and the string fallback mirrors the old JSON comparison).
fn plan_id_num(id: &PlanPointId) -> u64 {
    match id {
        PlanPointId::Number(n) => *n,
        PlanPointId::String(s) => s.parse().unwrap_or(0),
    }
}
