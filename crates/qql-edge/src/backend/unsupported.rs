//! Stable catalog of product features **not available offline** (qql-edge).
//!
//! Every entry has:
//! - a fixed error code `QQL-EDGE-UNSUPPORTED-*` (or a dedicated stable code)
//! - a short "why" naming exactly what the engine lacks
//! - a remediation line pointing users at remote Qdrant when applicable
//!
//! Operational/runtime errors (spawn, path extract, filter convert) stay
//! outside this catalog.

use qql_core::error::QqlError;

/// Offline-unsupported product surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeUnsupported {
    /// `GROUP BY … LOOKUP FROM <collection>` (group-hit hydration lookup).
    GroupLookup,
    /// `SHARD '…'` on query/mutation.
    ShardRouting,
    /// Collection create/custom sharding options.
    CollectionSharding,
    /// `CREATE`/`DROP SHARD KEY`.
    ShardKeyDdl,
    /// `ALTER COLLECTION … WITH PARAMS` (no edge params setter).
    AlterCollectionParams,
    /// `ALTER COLLECTION … QUANTIZATION` (no edge quantization setter).
    AlterCollectionQuantization,
    /// Collection `WITH PARAMS` (replication, etc.) at create time.
    CollectionParams,
    /// Optimizer keys qdrant-edge deliberately excludes.
    OptimizerKey,
    /// `PARAMS (timeout = …)`.
    Timeout,
    /// `PARAMS (consistency = …)`.
    Consistency,
    /// `SHOW QUOTAS` / `SET QUOTA`.
    Quota,
    /// `RECOMMEND … STRATEGY average_vector`.
    RecommendAverageVector,
    /// `MAX` / `MIN` / `ACOSH` formula functions (new Qdrant Expression variants).
    FormulaNary,
    /// Nearest/recommend inputs that are only point IDs (need materialised vectors offline).
    PointReferenceQuery,
    /// Catch-all unknown REST route projection.
    Route { path_hint: &'static str },
}

impl EdgeUnsupported {
    pub fn code(self) -> &'static str {
        match self {
            Self::GroupLookup => "QQL-EDGE-UNSUPPORTED-GROUP-LOOKUP",
            Self::ShardRouting | Self::CollectionSharding => "QQL-EDGE-UNSUPPORTED-SHARD",
            Self::ShardKeyDdl => "QQL-EDGE-UNSUPPORTED-SHARD-KEY",
            Self::AlterCollectionParams => "QQL-EDGE-UNSUPPORTED-ALTER-PARAMS",
            Self::AlterCollectionQuantization => "QQL-EDGE-UNSUPPORTED-ALTER-QUANTIZATION",
            Self::CollectionParams => "QQL-EDGE-UNSUPPORTED-COLLECTION-PARAMS",
            Self::OptimizerKey => "QQL-EDGE-UNSUPPORTED-OPTIMIZER-KEY",
            Self::Timeout => "QQL-EDGE-UNSUPPORTED-TIMEOUT",
            Self::Consistency => "QQL-EDGE-UNSUPPORTED-CONSISTENCY",
            Self::Quota => "QQL-EDGE-UNSUPPORTED-QUOTA",
            Self::RecommendAverageVector => "QQL-EDGE-UNSUPPORTED-RECOMMEND-STRATEGY",
            Self::PointReferenceQuery => "QQL-EDGE-UNSUPPORTED-POINT-REF",
            Self::FormulaNary => "QQL-EDGE-UNSUPPORTED-FORMULA-FUNCTION",
            Self::Route { .. } => "QQL-EDGE-UNSUPPORTED-ROUTE",
        }
    }

    pub fn feature(self) -> &'static str {
        match self {
            Self::GroupLookup => "GROUP BY … LOOKUP FROM",
            Self::ShardRouting => "SHARD routing",
            Self::CollectionSharding => {
                "collection sharding (shard_number / sharding_method / shard_keys)"
            }
            Self::ShardKeyDdl => "CREATE/DROP SHARD KEY",
            Self::AlterCollectionParams => "ALTER COLLECTION … WITH PARAMS",
            Self::AlterCollectionQuantization => "ALTER COLLECTION … QUANTIZATION",
            Self::CollectionParams => "collection WITH PARAMS (replication, etc.)",
            Self::OptimizerKey => {
                "OPTIMIZERS (memmap_threshold / flush_interval_sec / max_optimization_threads)"
            }
            Self::Timeout => "PARAMS (timeout = …)",
            Self::Consistency => "PARAMS (consistency = …)",
            Self::Quota => "SHOW QUOTAS / SET QUOTA",
            Self::RecommendAverageVector => "RECOMMEND STRATEGY average_vector",
            Self::PointReferenceQuery => "point-id query inputs without embedded vectors",
            Self::FormulaNary => "MAX / MIN / ACOSH formula functions",
            Self::Route { path_hint } => path_hint,
        }
    }

    pub fn why(self) -> &'static str {
        match self {
            Self::GroupLookup => {
                "qdrant-edge groups the queried shard only and has no lookup collection to hydrate hits from"
            }
            Self::ShardRouting => {
                "a qql-edge collection is a single qdrant-edge shard, so there is no shard to route to"
            }
            Self::CollectionSharding => {
                "qdrant-edge creates exactly one shard per collection; it has no shard_number / sharding_method / shard_keys API"
            }
            Self::ShardKeyDdl => {
                "qdrant-edge has no custom shard keys; a collection is a single shard"
            }
            Self::AlterCollectionParams => {
                "qdrant-edge persists HNSW and optimizer config after create; it exposes no collection-params setter"
            }
            Self::AlterCollectionQuantization => {
                "qdrant-edge exposes no quantization setter after create"
            }
            Self::CollectionParams => {
                "qdrant-edge persists only on_disk_payload; replication, write-consistency, fan-out, and payload-memory params have no offline equivalent"
            }
            Self::OptimizerKey => {
                "qdrant-edge has no memmap threshold, no timer flush, and runs optimizations manually"
            }
            Self::Timeout => {
                "qdrant-edge QueryRequest has no timeout field; qql-edge executes in-process with no RPC deadline"
            }
            Self::Consistency => {
                "qdrant-edge is a single-node engine with no replica consistency levels"
            }
            Self::Quota => {
                "qdrant-edge has no quotas API; Qdrant serves quotas only from the cluster REST /quotas endpoint"
            }
            Self::RecommendAverageVector => {
                "qdrant-edge QueryEnum has RecommendBestScore and RecommendSumScores only"
            }
            Self::PointReferenceQuery => {
                "qdrant-edge has no lookup_from / point-vector resolution; inputs must carry a materialized TEXT or VECTOR"
            }
            Self::FormulaNary => "qdrant-edge 0.8 Expression has no ACOSH / MAX / MIN variants",
            Self::Route { .. } => "this route has no qql-edge implementation",
        }
    }

    pub fn remote_hint(self) -> Option<&'static str> {
        match self {
            Self::CollectionParams | Self::PointReferenceQuery => None,
            Self::RecommendAverageVector => Some(
                "Use STRATEGY best_score or sum_scores offline, or remote Qdrant for average_vector",
            ),
            _ => Some("Use remote Qdrant (REST or gRPC) for this feature"),
        }
    }

    pub fn message(self) -> String {
        let mut msg = format!(
            "{} is not supported offline: {}.",
            self.feature(),
            self.why()
        );
        if let Some(hint) = self.remote_hint() {
            msg.push(' ');
            msg.push_str(hint);
            msg.push('.');
        }
        msg
    }

    pub fn error(self) -> QqlError {
        QqlError::execution(self.code(), self.message(), None)
    }
}

/// Convenience: reject optional shard key on DML.
pub fn reject_shard_key<T>(shard_key: Option<T>) -> Result<(), QqlError> {
    if shard_key.is_some() {
        Err(EdgeUnsupported::ShardRouting.error())
    } else {
        Ok(())
    }
}

/// Convenience: reject collection sharding options on create.
pub fn reject_collection_sharding(
    shard_number: Option<u64>,
    sharding_method: Option<qql_plan::ShardingMethod>,
    shard_keys: Option<&[qql_plan::semantic::PlanShardKey]>,
) -> Result<(), QqlError> {
    if shard_number.is_some() || sharding_method.is_some() || shard_keys.is_some() {
        Err(EdgeUnsupported::CollectionSharding.error())
    } else {
        Ok(())
    }
}

/// Convenience: reject the create-time `WITH PARAMS` keys qdrant-edge cannot
/// persist. `on_disk_payload` **is** honored (it maps onto `EdgeConfig`), so a
/// params block carrying only that key is accepted.
pub fn reject_collection_params(
    params: Option<&qql_plan::CollectionParams>,
) -> Result<(), QqlError> {
    let unsupported = params.is_some_and(|params| {
        params.replication_factor.is_some()
            || params.write_consistency_factor.is_some()
            || params.read_fan_out_factor.is_some()
            || params.read_fan_out_delay_ms.is_some()
            || params.payload.is_some()
    });
    if unsupported {
        Err(EdgeUnsupported::CollectionParams.error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_codes_are_stable_and_unique_for_primary_features() {
        let features = [
            EdgeUnsupported::GroupLookup,
            EdgeUnsupported::ShardRouting,
            EdgeUnsupported::CollectionSharding,
            EdgeUnsupported::ShardKeyDdl,
            EdgeUnsupported::AlterCollectionParams,
            EdgeUnsupported::AlterCollectionQuantization,
            EdgeUnsupported::CollectionParams,
            EdgeUnsupported::OptimizerKey,
            EdgeUnsupported::RecommendAverageVector,
            EdgeUnsupported::Quota,
            EdgeUnsupported::PointReferenceQuery,
        ];
        let mut codes = std::collections::BTreeSet::new();
        for f in features {
            assert!(
                f.code().starts_with("QQL-EDGE-UNSUPPORTED-"),
                "{}",
                f.code()
            );
            // Collection sharding shares SHARD code with routing (same product class).
            if matches!(f, EdgeUnsupported::CollectionSharding) {
                assert_eq!(f.code(), EdgeUnsupported::ShardRouting.code());
            } else {
                assert!(codes.insert(f.code()), "duplicate code {}", f.code());
            }
            let msg = f.message();
            assert!(!msg.is_empty());
            assert!(
                msg.contains("not supported offline") || msg.contains("not supported"),
                "{msg}"
            );
            if f.remote_hint().is_some() {
                assert!(
                    msg.to_ascii_lowercase().contains("remote")
                        || msg.to_ascii_lowercase().contains("best_score"),
                    "expected remediation in: {msg}"
                );
            }
        }
    }

    #[test]
    fn group_lookup_message_names_the_engine_gap() {
        let e = EdgeUnsupported::GroupLookup.error();
        assert_eq!(e.code, "QQL-EDGE-UNSUPPORTED-GROUP-LOOKUP");
        assert!(e.message.contains("LOOKUP FROM"));
        assert!(e.message.contains("no lookup collection"));
    }

    #[test]
    fn alter_field_rejections_are_precise() {
        let params = EdgeUnsupported::AlterCollectionParams.error();
        assert_eq!(params.code, "QQL-EDGE-UNSUPPORTED-ALTER-PARAMS");
        assert!(params.message.contains("no collection-params setter"));

        let quantization = EdgeUnsupported::AlterCollectionQuantization.error();
        assert_eq!(quantization.code, "QQL-EDGE-UNSUPPORTED-ALTER-QUANTIZATION");
        assert!(quantization.message.contains("no quantization setter"));
    }

    #[test]
    fn create_params_accept_only_on_disk_payload() {
        assert!(reject_collection_params(None).is_ok());

        let on_disk_only = qql_plan::CollectionParams {
            on_disk_payload: Some(false),
            ..Default::default()
        };
        assert!(reject_collection_params(Some(&on_disk_only)).is_ok());

        let replicated = qql_plan::CollectionParams {
            replication_factor: Some(3),
            ..Default::default()
        };
        let error = reject_collection_params(Some(&replicated)).unwrap_err();
        assert_eq!(error.code, "QQL-EDGE-UNSUPPORTED-COLLECTION-PARAMS");
        assert!(error.message.contains("only on_disk_payload"));
    }
}
