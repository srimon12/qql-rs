//! Payload index request types (`CREATE INDEX`).
//!
//! Typed mirror of the OpenAPI `PayloadIndexParams` schemas QQL can emit.
//! `CreateIndexRequest` serializes the flat plan IR; the OpenAPI
//! `field_schema` nesting is a REST projection (`ddl_rest`).

use alloc::string::String;
use alloc::vec::Vec;
use qql_core::ast::MemoryPlacement;
use serde::{Serialize, Serializer};

/// Payload field schema type (`CREATE INDEX ... TYPE <type>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IndexFieldType {
    /// Keyword / exact-match index.
    Keyword,
    /// Integer range/lookup index.
    Integer,
    /// Float range index.
    Float,
    /// Geo bounding-box / radius index.
    Geo,
    /// Full-text index.
    Text,
    /// Boolean index.
    Bool,
    /// Datetime range index.
    Datetime,
    /// UUID index.
    Uuid,
}

impl IndexFieldType {
    /// Canonical lowercase wire name (`keyword`, `text`, …).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Keyword => "keyword",
            Self::Integer => "integer",
            Self::Float => "float",
            Self::Geo => "geo",
            Self::Text => "text",
            Self::Bool => "bool",
            Self::Datetime => "datetime",
            Self::Uuid => "uuid",
        }
    }

    /// Parse an unquoted canonical field type (case-insensitive). Unknown → `None`.
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "keyword" => Some(Self::Keyword),
            "integer" => Some(Self::Integer),
            "float" => Some(Self::Float),
            "geo" => Some(Self::Geo),
            "text" => Some(Self::Text),
            "bool" => Some(Self::Bool),
            "datetime" => Some(Self::Datetime),
            "uuid" => Some(Self::Uuid),
            _ => None,
        }
    }
}

/// OpenAPI `TokenizerType` for text indexes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TextTokenizer {
    /// Split on prefix boundaries.
    Prefix,
    /// Split on whitespace.
    Whitespace,
    /// Split on word boundaries (server default).
    Word,
    /// Language-aware tokenization.
    Multilingual,
}

impl TextTokenizer {
    /// Canonical lowercase wire name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prefix => "prefix",
            Self::Whitespace => "whitespace",
            Self::Word => "word",
            Self::Multilingual => "multilingual",
        }
    }

    /// Parse a tokenizer name (case-insensitive). Unknown → `None`.
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "prefix" => Some(Self::Prefix),
            "whitespace" => Some(Self::Whitespace),
            "word" => Some(Self::Word),
            "multilingual" => Some(Self::Multilingual),
            _ => None,
        }
    }
}

/// OpenAPI `StemmingAlgorithm`: a snowball language or explicit `none`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StemmingAlgorithm {
    /// Snowball stemming for the given language (`{"type":"snowball",...}`).
    Snowball(String),
    /// Explicitly disable stemming (`{"type":"none"}`).
    Disabled,
}

impl StemmingAlgorithm {
    /// `none` (case-insensitive) disables stemming; anything else is a snowball
    /// language, normalized to lowercase to match the OpenAPI enum.
    pub fn parse(value: &str) -> Self {
        if value.eq_ignore_ascii_case("none") {
            Self::Disabled
        } else {
            Self::Snowball(value.to_ascii_lowercase())
        }
    }
}

impl Serialize for StemmingAlgorithm {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(2))?;
        match self {
            Self::Snowball(language) => {
                map.serialize_entry("type", "snowball")?;
                map.serialize_entry("language", language)?;
            }
            Self::Disabled => map.serialize_entry("type", "none")?,
        }
        map.end()
    }
}

/// OpenAPI `StopwordsSet`: custom stopwords (QQL exposes no language lists).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StopwordsSet {
    /// Custom stopwords, merged with any language lists server-side.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub custom: Vec<String>,
}

/// `WITH (…)` options of `CREATE INDEX`, typed per OpenAPI `PayloadSchemaParams`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct IndexOptions {
    /// Tenant-optimization flag (keyword / uuid indexes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_tenant: Option<bool>,
    /// Legacy on-disk flag (prefer `memory`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_disk: Option<bool>,
    /// Build additional HNSW links for this field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_hnsw: Option<bool>,
    /// Lowercase all tokens (text indexes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lowercase: Option<bool>,
    /// Fold accented characters to ASCII (text indexes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ascii_folding: Option<bool>,
    /// Enable phrase matching (text indexes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phrase_matching: Option<bool>,
    /// Support direct lookups (integer indexes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lookup: Option<bool>,
    /// Support range filters (integer indexes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<bool>,
    /// Organize collection storage by this key (integer / float / datetime).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_principal: Option<bool>,
    /// Enable prefix matching (keyword indexes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<bool>,
    /// Minimum token length.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_token_len: Option<u64>,
    /// Maximum token length.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_token_len: Option<u64>,
    /// Tokenizer (text indexes); server default is `word`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokenizer: Option<TextTokenizer>,
    /// Stemming algorithm (text indexes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stemmer: Option<StemmingAlgorithm>,
    /// Stopwords (text indexes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopwords: Option<StopwordsSet>,
    /// Memory placement of the index.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryPlacement>,
}

impl IndexOptions {
    /// True when no option was set.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Plan IR for `CREATE INDEX`.
#[derive(Debug, Clone, Serialize)]
pub struct CreateIndexRequest {
    /// Payload field to index.
    pub field_name: String,
    /// Typed field schema.
    pub field_schema: IndexFieldType,
    /// `WITH (…)` index options.
    #[serde(skip_serializing_if = "IndexOptions::is_empty")]
    pub options: IndexOptions,
}
