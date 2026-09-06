//! Stable catalog of product features **not available offline** (qql-edge).
//!
//! Every entry has:
//! - a fixed error code `QQL-EDGE-UNSUPPORTED-*` (or a dedicated stable code)
//! - a short "why"
//! - a remediation line pointing users at remote Qdrant when applicable
//!
//! Operational/runtime errors (spawn, path extract, filter convert) stay
//! outside this catalog.

use qql_core::error::QqlError;

/// Offline-unsupported product surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeUnsupported {
    /// `GROUP BY` / `/points/query/groups`.
    GroupBy,
    /// `SHARD '…'` on query/mutation.
    ShardRouting,
    /// Collection create/custom sharding options.
    CollectionSharding,
    /// `CREATE`/`DROP SHARD KEY`.
    ShardKeyDdl,
    /// `ALTER COLLECTION` / update collection.
    AlterCollection,
    /// Collection `WITH PARAMS` (replication, etc.) at create time.
    CollectionParams,
    /// `PARAMS (acorn = …)`.
    Acorn,
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
            Self::GroupBy => "QQL-EDGE-UNSUPPORTED-GROUP-BY",
            Self::ShardRouting | Self::CollectionSharding => "QQL-EDGE-UNSUPPORTED-SHARD",
            Self::ShardKeyDdl => "QQL-EDGE-UNSUPPORTED-SHARD-KEY",
            Self::AlterCollection => "QQL-EDGE-UNSUPPORTED-ALTER",
            Self::CollectionParams => "QQL-EDGE-UNSUPPORTED-COLLECTION-PARAMS",
            Self::Acorn => "QQL-EDGE-UNSUPPORTED-ACORN",
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
            Self::GroupBy => "GROUP BY / query groups",
            Self::ShardRouting => "SHARD routing",
            Self::CollectionSharding => {
                "collection sharding (shard_number / sharding_method / shard_keys)"
            }
            Self::ShardKeyDdl => "CREATE/DROP SHARD KEY",
            Self::AlterCollection => "ALTER COLLECTION",
            Self::CollectionParams => "collection WITH PARAMS (replication, etc.)",
            Self::Acorn => "PARAMS (acorn = …)",
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
            Self::GroupBy => "qdrant-edge has no /points/query/groups endpoint",
            Self::ShardRouting | Self::CollectionSharding | Self::ShardKeyDdl => {
                "qql-edge is a single-node process with no custom shard keys"
            }
            Self::AlterCollection => "qql-edge does not support collection mutation after create",
            Self::CollectionParams => {
                "edge storage is configured via LocalExecutorOptions, not collection PARAMS"
            }
            Self::Acorn => "ACORN is a clustered Qdrant HNSW search feature unavailable offline",
            Self::Timeout => "qql-edge runs in-process without network RPC timeouts",
            Self::Consistency => {
                "qql-edge is a single-node in-process engine without replica consistency levels"
            }
            Self::Quota => {
                "global resource quotas are cluster-wide and require Qdrant's REST /quotas API"
            }
            Self::RecommendAverageVector => {
                "qdrant-edge recommend supports best_score and sum_scores only"
            }
            Self::PointReferenceQuery => {
                "offline path must materialize vectors (TEXT/VECTOR) before search"
            }
            Self::FormulaNary => {
                "the pinned qdrant-edge predates the acosh / max / min Expression variants"
            }
            Self::Route { .. } => "this route is not implemented by the edge backend",
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
pub fn reject_shard_key(shard_key: Option<&str>) -> Result<(), QqlError> {
    if shard_key.is_some() {
        Err(EdgeUnsupported::ShardRouting.error())
    } else {
        Ok(())
    }
}

/// Convenience: reject collection sharding options on create.
pub fn reject_collection_sharding(
    shard_number: Option<u64>,
    sharding_method: Option<&str>,
    shard_keys: Option<&[String]>,
) -> Result<(), QqlError> {
    if shard_number.is_some() || sharding_method.is_some() || shard_keys.is_some() {
        Err(EdgeUnsupported::CollectionSharding.error())
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
            EdgeUnsupported::GroupBy,
            EdgeUnsupported::ShardRouting,
            EdgeUnsupported::CollectionSharding,
            EdgeUnsupported::ShardKeyDdl,
            EdgeUnsupported::AlterCollection,
            EdgeUnsupported::CollectionParams,
            EdgeUnsupported::Acorn,
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
    fn group_by_message_mentions_query_groups_and_remote() {
        let e = EdgeUnsupported::GroupBy.error();
        assert_eq!(e.code, "QQL-EDGE-UNSUPPORTED-GROUP-BY");
        assert!(e.message.contains("query groups"));
        assert!(e.message.contains("remote Qdrant"));
    }
}
