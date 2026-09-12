//! Wrapped-request endpoint resolution.
//!
//! The authority for endpoint → statement mapping is the "Statement →
//! Endpoint Matrix" in the workspace `AGENTS.md` (27 routes: 26 statements +
//! the `change_aliases` helper). OpenAPI paths outside that matrix are
//! [`ConvertError::UnsupportedEndpoint`], even when `openapi.json` defines a
//! request schema for them — QQL has no statement to emit.

use crate::ConvertError;

/// One matrix row: the operation a wrapped `{method, path}` resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Endpoint {
    /// `POST /collections/{c}/points/query` → `QUERY`.
    Query,
    /// `POST /collections/{c}/points/query/groups` → grouped `QUERY`.
    QueryGroups,
    /// `POST /collections/{c}/points` → `QUERY POINTS`.
    GetPoints,
    /// `POST /collections/{c}/facet` → `FACET`.
    Facet,
    /// `POST /collections/{c}/points/scroll` → `SCROLL`.
    Scroll,
    /// `POST /collections/{c}/points/count` → `COUNT`.
    Count,
    /// `PUT /collections/{c}/points` → `UPSERT`.
    Upsert,
    /// `POST /collections/{c}/points/delete` → `DELETE`.
    Delete,
    /// `POST /collections/{c}/points/payload/clear` → `CLEAR PAYLOAD`.
    ClearPayload,
    /// `POST /collections/{c}/points/payload/delete` → `DELETE PAYLOAD`.
    DeletePayload,
    /// `POST /collections/{c}/points/vectors/delete` → `DELETE VECTOR`.
    DeleteVectors,
    /// `PUT /collections/{c}/points/vectors` → `UPDATE … SET VECTOR`.
    UpdateVectors,
    /// `POST /collections/{c}/points/payload` → `UPDATE … SET PAYLOAD`.
    UpdatePayload,
    /// `PUT /collections/{c}` → `CREATE COLLECTION`.
    CreateCollection,
    /// `PATCH /collections/{c}` → `ALTER COLLECTION`.
    AlterCollection,
    /// `DELETE /collections/{c}` → `DROP COLLECTION` (bodyless).
    DropCollection,
    /// `PUT /collections/{c}/index` → `CREATE INDEX`.
    CreateIndex,
    /// `DELETE /collections/{c}/index/{field}` → `DROP INDEX` (bodyless).
    DropIndex,
    /// `PUT /collections/{c}/shards` → `CREATE SHARD KEY`.
    CreateShardKey,
    /// `POST /collections/{c}/shards/delete` → `DROP SHARD KEY`.
    DropShardKey,
    /// `GET /collections/{c}/shards` → `SHOW SHARD KEYS` (bodyless).
    ShowShardKeys,
    /// `GET /collections` → `SHOW COLLECTIONS` (bodyless).
    ShowCollections,
    /// `GET /collections/{c}` → `SHOW COLLECTION` (bodyless).
    ShowCollection,
    /// `GET /quotas` → `SHOW QUOTAS` (bodyless).
    ShowQuotas,
    /// `PUT /quotas` → `SET QUOTA`.
    SetQuota,
    /// `POST /collections/{c}/points/query/batch` → one query `BATCH` block.
    QueryBatch,
    /// `POST /collections/{c}/points/batch` → one mutation `BATCH` block.
    PointsBatch,
}

impl Endpoint {
    /// Whether the route carries a request body in the OpenAPI spec.
    pub(crate) fn body_required(self) -> bool {
        !matches!(
            self,
            Self::DropCollection
                | Self::DropIndex
                | Self::ShowShardKeys
                | Self::ShowCollections
                | Self::ShowCollection
                | Self::ShowQuotas
        )
    }
}

/// Resolved wrapped request: operation plus path-derived collection/field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EndpointMatch {
    /// Matrix operation.
    pub(crate) op: Endpoint,
    /// Collection from `/collections/{collection}/…`. `None` for
    /// collection-independent routes (`SHOW COLLECTIONS`, quotas).
    pub(crate) collection: Option<String>,
    /// Trailing path segment for `DELETE /collections/{c}/index/{field}`.
    pub(crate) field: Option<String>,
}

/// Resolve `{method, path}` to a matrix endpoint.
///
/// `path` may carry a leading `/`. Only exact matrix routes resolve; anything
/// else (including OpenAPI routes QQL does not model, such as alias or
/// snapshot operations) fails with [`ConvertError::UnsupportedEndpoint`].
pub(crate) fn parse(method: &str, path: &str) -> Result<EndpointMatch, ConvertError> {
    let trimmed = path.trim_start_matches('/');
    // Normalize a trailing slash (`/collections/` == `/collections`).
    let trimmed = trimmed.strip_suffix('/').unwrap_or(trimmed);
    let parts: Vec<&str> = trimmed.split('/').collect();
    let unsupported = || ConvertError::UnsupportedEndpoint(format!("{method} {trimmed}"));

    let collection_of = |parts: &[&str]| -> Result<Option<String>, ConvertError> {
        match parts.get(1) {
            None | Some(&"") => Err(unsupported()),
            Some(name) => {
                reject_template_collection(name)?;
                Ok(Some((*name).to_string()))
            }
        }
    };

    // Bodyless collection-independent listings.
    if method.eq_ignore_ascii_case("GET") {
        if trimmed == "collections" {
            return Ok(EndpointMatch {
                op: Endpoint::ShowCollections,
                collection: None,
                field: None,
            });
        }
        if trimmed == "quotas" {
            return Ok(EndpointMatch {
                op: Endpoint::ShowQuotas,
                collection: None,
                field: None,
            });
        }
    }
    if method.eq_ignore_ascii_case("PUT") && trimmed == "quotas" {
        return Ok(EndpointMatch {
            op: Endpoint::SetQuota,
            collection: None,
            field: None,
        });
    }

    // Everything else in the matrix is collection-scoped.
    if parts.first() != Some(&"collections") || parts.len() < 2 {
        return Err(unsupported());
    }
    let collection = collection_of(&parts)?;
    let rest = &parts[2..];
    let op = match (method.to_ascii_uppercase().as_str(), rest) {
        ("POST", ["points", "query"]) => Endpoint::Query,
        ("POST", ["points", "query", "groups"]) => Endpoint::QueryGroups,
        ("POST", ["points"]) => Endpoint::GetPoints,
        ("POST", ["facet"]) => Endpoint::Facet,
        ("POST", ["points", "scroll"]) => Endpoint::Scroll,
        ("POST", ["points", "count"]) => Endpoint::Count,
        ("PUT", ["points"]) => Endpoint::Upsert,
        ("POST", ["points", "delete"]) => Endpoint::Delete,
        ("POST", ["points", "query", "batch"]) => Endpoint::QueryBatch,
        ("POST", ["points", "batch"]) => Endpoint::PointsBatch,
        ("POST", ["points", "payload", "clear"]) => Endpoint::ClearPayload,
        ("POST", ["points", "payload", "delete"]) => Endpoint::DeletePayload,
        ("POST", ["points", "vectors", "delete"]) => Endpoint::DeleteVectors,
        ("PUT", ["points", "vectors"]) => Endpoint::UpdateVectors,
        ("POST", ["points", "payload"]) => Endpoint::UpdatePayload,
        ("PUT", []) => Endpoint::CreateCollection,
        ("PATCH", []) => Endpoint::AlterCollection,
        ("DELETE", []) => Endpoint::DropCollection,
        ("PUT", ["index"]) => Endpoint::CreateIndex,
        ("DELETE", ["index", field]) => {
            return Ok(EndpointMatch {
                op: Endpoint::DropIndex,
                collection,
                field: Some((*field).to_string()),
            });
        }
        ("PUT", ["shards"]) => Endpoint::CreateShardKey,
        ("POST", ["shards", "delete"]) => Endpoint::DropShardKey,
        ("GET", ["shards"]) => Endpoint::ShowShardKeys,
        ("GET", []) => Endpoint::ShowCollection,
        _ => return Err(unsupported()),
    };
    Ok(EndpointMatch {
        op,
        collection,
        field: None,
    })
}

/// Collections pasted from docs often carry template markers
/// (`<your-collection>`, `{collection_name}`, `$COLLECTION`). Emitting them
/// would produce QQL that cannot re-parse, so reject them here — one check for
/// the wrapped, snippet, and curl paths alike (bare `--collection` values go
/// through [`crate::convert`] which calls this too).
pub(crate) fn reject_template_collection(name: &str) -> Result<(), ConvertError> {
    if name
        .chars()
        .any(|c| matches!(c, '<' | '>' | '{' | '}' | '$') || c.is_whitespace())
    {
        return Err(ConvertError::InvalidField {
            path: "collection".to_string(),
            detail: format!(
                "looks like a placeholder (`{name}`) — replace it with the real collection name"
            ),
        });
    }
    Ok(())
}
