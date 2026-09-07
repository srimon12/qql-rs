//! Typed AST for QQL statements (`Stmt` and its variants).

pub mod ddl;
pub mod mutation;
pub mod query;
pub mod retrieval;
pub mod types;

pub use ddl::*;
pub use mutation::*;
pub use query::*;
pub use retrieval::*;
pub use types::*;

use alloc::boxed::Box;
use alloc::string::String;

/// Top-level QQL statement parsed from a script.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// `QUERY …` retrieval (all `QueryExpr` forms).
    Query(Box<QueryStmt>),
    /// `SCROLL …` cursor iteration.
    Scroll(Box<ScrollStmt>),
    /// `UPSERT INTO …` point write.
    Upsert(Box<UpsertStmt>),
    /// `CREATE COLLECTION …` DDL.
    CreateCollection(Box<CreateCollectionStmt>),
    /// `CREATE INDEX …` DDL.
    CreateIndex(Box<CreateIndexStmt>),
    /// `DROP INDEX …` DDL.
    DropIndex(Box<DropIndexStmt>),
    /// `CREATE SHARD KEY …` DDL.
    CreateShardKey(Box<CreateShardKeyStmt>),
    /// `DROP SHARD KEY …` DDL.
    DropShardKey(Box<DropShardKeyStmt>),
    /// `ALTER COLLECTION …` DDL.
    AlterCollection(Box<AlterCollectionStmt>),
    /// `DROP COLLECTION …` DDL.
    DropCollection(Box<DropCollectionStmt>),
    /// `SHOW COLLECTIONS` listing.
    ShowCollections,
    /// `SHOW COLLECTION <name>` detail.
    ShowCollection(String),
    /// `SHOW SHARD KEYS ON COLLECTION <name>` listing.
    ShowShardKeys(String),
    /// `DELETE FROM …` point removal.
    Delete(Box<DeleteStmt>),
    /// `CLEAR PAYLOAD …` payload wipe.
    ClearPayload(Box<ClearPayloadStmt>),
    /// `DELETE PAYLOAD <keys> …` payload key removal.
    DeletePayload(Box<DeletePayloadStmt>),
    /// `DELETE VECTOR <names> …` named vector removal.
    DeleteVector(Box<DeleteVectorStmt>),
    /// `UPDATE … SET VECTOR …` vector replacement.
    UpdateVector(Box<UpdateVectorStmt>),
    /// `UPDATE … SET PAYLOAD …` payload merge.
    UpdatePayload(Box<UpdatePayloadStmt>),
    /// `COUNT …` point counting.
    Count(Box<CountStmt>),
    /// `FACET …` categorical aggregation.
    Facet(Box<FacetStmt>),
    /// `SHOW QUOTAS` listing.
    ShowQuotas,
    /// `SET QUOTA (…)` cluster quota replacement.
    SetQuota(Box<SetQuotaStmt>),
}

#[cfg(feature = "serde")]
impl serde::Serialize for Stmt {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        match self {
            Stmt::Query(s) => serializer.serialize_newtype_variant("Stmt", 0, "Query", s),
            Stmt::Scroll(s) => serializer.serialize_newtype_variant("Stmt", 1, "Scroll", s),
            Stmt::Upsert(s) => serializer.serialize_newtype_variant("Stmt", 2, "Upsert", s),
            Stmt::CreateCollection(s) => {
                serializer.serialize_newtype_variant("Stmt", 3, "CreateCollection", s)
            }
            Stmt::CreateIndex(s) => {
                serializer.serialize_newtype_variant("Stmt", 4, "CreateIndex", s)
            }
            Stmt::DropIndex(s) => serializer.serialize_newtype_variant("Stmt", 5, "DropIndex", s),
            Stmt::CreateShardKey(s) => {
                serializer.serialize_newtype_variant("Stmt", 6, "CreateShardKey", s)
            }
            Stmt::DropShardKey(s) => {
                serializer.serialize_newtype_variant("Stmt", 7, "DropShardKey", s)
            }
            Stmt::AlterCollection(s) => {
                serializer.serialize_newtype_variant("Stmt", 8, "AlterCollection", s)
            }
            Stmt::DropCollection(s) => {
                serializer.serialize_newtype_variant("Stmt", 9, "DropCollection", s)
            }
            // Unit variant. The serialized form is the empty-object tag
            // `{"ShowCollections": {}}` (kept for backward compatibility with
            // consumers that already emit that shape). The manual
            // `Deserialize` accepts both this form and the derived string
            // form `"ShowCollections"`, so serde round-trips.
            Stmt::ShowCollections => {
                let mut map = serializer.serialize_map(Some(1))?;
                let empty = std::collections::BTreeMap::<String, String>::new();
                map.serialize_entry("ShowCollections", &empty)?;
                map.end()
            }
            Stmt::ShowCollection(s) => {
                serializer.serialize_newtype_variant("Stmt", 11, "ShowCollection", s)
            }
            Stmt::ShowShardKeys(s) => {
                serializer.serialize_newtype_variant("Stmt", 12, "ShowShardKeys", s)
            }
            Stmt::Delete(s) => serializer.serialize_newtype_variant("Stmt", 13, "Delete", s),
            Stmt::ClearPayload(s) => {
                serializer.serialize_newtype_variant("Stmt", 14, "ClearPayload", s)
            }
            Stmt::DeletePayload(s) => {
                serializer.serialize_newtype_variant("Stmt", 15, "DeletePayload", s)
            }
            Stmt::DeleteVector(s) => {
                serializer.serialize_newtype_variant("Stmt", 16, "DeleteVector", s)
            }
            Stmt::UpdateVector(s) => {
                serializer.serialize_newtype_variant("Stmt", 17, "UpdateVector", s)
            }
            Stmt::UpdatePayload(s) => {
                serializer.serialize_newtype_variant("Stmt", 18, "UpdatePayload", s)
            }
            Stmt::Count(s) => serializer.serialize_newtype_variant("Stmt", 19, "Count", s),
            Stmt::Facet(s) => serializer.serialize_newtype_variant("Stmt", 20, "Facet", s),
            Stmt::ShowQuotas => {
                let mut map = serializer.serialize_map(Some(1))?;
                let empty = std::collections::BTreeMap::<String, String>::new();
                map.serialize_entry("ShowQuotas", &empty)?;
                map.end()
            }
            Stmt::SetQuota(s) => serializer.serialize_newtype_variant("Stmt", 22, "SetQuota", s),
        }
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Stmt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use core::fmt;
        use serde::de::{Error as _, IgnoredAny, MapAccess, Visitor};

        struct StmtVisitor;

        impl<'de> Visitor<'de> for StmtVisitor {
            type Value = Stmt;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("an externally tagged QQL statement")
            }

            /// Derived externally-tagged form of the unit variant.
            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if value == "ShowCollections" {
                    Ok(Stmt::ShowCollections)
                } else {
                    Err(E::unknown_variant(value, &["ShowCollections"]))
                }
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let key = map
                    .next_key::<alloc::string::String>()?
                    .ok_or_else(|| A::Error::custom("expected a statement tag"))?;
                let stmt = match key.as_str() {
                    "Query" => Stmt::Query(map.next_value()?),
                    "Scroll" => Stmt::Scroll(map.next_value()?),
                    "Upsert" => Stmt::Upsert(map.next_value()?),
                    "CreateCollection" => Stmt::CreateCollection(map.next_value()?),
                    "CreateIndex" => Stmt::CreateIndex(map.next_value()?),
                    "DropIndex" => Stmt::DropIndex(map.next_value()?),
                    "CreateShardKey" => Stmt::CreateShardKey(map.next_value()?),
                    "DropShardKey" => Stmt::DropShardKey(map.next_value()?),
                    "AlterCollection" => Stmt::AlterCollection(map.next_value()?),
                    "DropCollection" => Stmt::DropCollection(map.next_value()?),
                    // Canonical serialized form (`{"ShowCollections": {}}`);
                    // the payload is ignored, mirroring the derived impl's
                    // permissive unit-variant handling.
                    "ShowCollections" => {
                        map.next_value::<IgnoredAny>()?;
                        Stmt::ShowCollections
                    }
                    "ShowQuotas" => {
                        map.next_value::<IgnoredAny>()?;
                        Stmt::ShowQuotas
                    }
                    "ShowCollection" => Stmt::ShowCollection(map.next_value()?),
                    "ShowShardKeys" => Stmt::ShowShardKeys(map.next_value()?),
                    "Delete" => Stmt::Delete(map.next_value()?),
                    "ClearPayload" => Stmt::ClearPayload(map.next_value()?),
                    "DeletePayload" => Stmt::DeletePayload(map.next_value()?),
                    "DeleteVector" => Stmt::DeleteVector(map.next_value()?),
                    "UpdateVector" => Stmt::UpdateVector(map.next_value()?),
                    "UpdatePayload" => Stmt::UpdatePayload(map.next_value()?),
                    "Count" => Stmt::Count(map.next_value()?),
                    "Facet" => Stmt::Facet(map.next_value()?),
                    "SetQuota" => Stmt::SetQuota(map.next_value()?),
                    _ => {
                        return Err(A::Error::unknown_variant(
                            &key,
                            &[
                                "Query",
                                "Scroll",
                                "Upsert",
                                "CreateCollection",
                                "CreateIndex",
                                "DropIndex",
                                "CreateShardKey",
                                "DropShardKey",
                                "AlterCollection",
                                "DropCollection",
                                "ShowCollections",
                                "ShowCollection",
                                "ShowShardKeys",
                                "Delete",
                                "ClearPayload",
                                "DeletePayload",
                                "DeleteVector",
                                "UpdateVector",
                                "UpdatePayload",
                                "Count",
                                "Facet",
                                "ShowQuotas",
                                "SetQuota",
                            ],
                        ));
                    }
                };
                if map.next_key::<IgnoredAny>()?.is_some() {
                    return Err(A::Error::custom("duplicate statement tag"));
                }
                Ok(stmt)
            }
        }

        deserializer.deserialize_any(StmtVisitor)
    }
}

impl Stmt {
    /// Return the canonical keyword name for this statement variant (e.g. "QUERY", "CREATE COLLECTION").
    pub fn stmt_kind(&self) -> &'static str {
        match self {
            Self::Query(_) => "QUERY",
            Self::Scroll(_) => "SCROLL",
            Self::Count(_) => "COUNT",
            Self::Facet(_) => "FACET",
            Self::Upsert(_) => "UPSERT",
            Self::Delete(_) => "DELETE",
            Self::ClearPayload(_) => "CLEAR PAYLOAD",
            Self::DeletePayload(_) => "DELETE PAYLOAD",
            Self::DeleteVector(_) => "DELETE VECTOR",
            Self::UpdateVector(_) => "UPDATE VECTOR",
            Self::UpdatePayload(_) => "UPDATE PAYLOAD",
            Self::CreateCollection(_) => "CREATE COLLECTION",
            Self::AlterCollection(_) => "ALTER COLLECTION",
            Self::DropCollection(_) => "DROP COLLECTION",
            Self::CreateIndex(_) => "CREATE INDEX",
            Self::DropIndex(_) => "DROP INDEX",
            Self::CreateShardKey(_) => "CREATE SHARD KEY",
            Self::DropShardKey(_) => "DROP SHARD KEY",
            Self::ShowCollections => "SHOW COLLECTIONS",
            Self::ShowCollection(_) => "SHOW COLLECTION",
            Self::ShowShardKeys(_) => "SHOW SHARD KEYS",
            Self::ShowQuotas => "SHOW QUOTAS",
            Self::SetQuota(_) => "SET QUOTA",
        }
    }
}

impl core::fmt::Display for Stmt {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", crate::fmt::format_stmt(self))
    }
}
