use core::fmt;

use crate::error::Span;

// ── Token kind definitions ─────────────────────────────────────
// All variants are listed ONCE in the `token_table!`.
// Two helper macros consume that table to produce:
//   • as_str()     – for all variants
//   • KEYWORDS map – for keyword-only variants

macro_rules! gen_as_str {
    ($($var:ident => $str:expr),* $(,)?) => {
        impl TokenKind {
            /// Returns the canonical display string for this token kind.
            pub fn as_str(&self) -> &'static str {
                match self {
                    $( Self::$var => $str, )*
                }
            }
        }
    };
}

/// The lexical token kinds produced by the QQL lexer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum TokenKind {
    /// The `ABS` keyword.
    Abs,
    /// The `ACORN` keyword.
    Acorn,
    /// The `ACOSH` keyword.
    Acosh,
    /// The `AFTER` keyword.
    After,
    /// The `ALL` keyword.
    All,
    /// The `ALTER` keyword.
    Alter,
    /// The `AND` keyword.
    And,
    /// The `ANY` keyword.
    Any,
    /// The `AS` keyword.
    As,
    /// The `ASC` keyword.
    Asc,
    /// The `AVERAGE_VECTOR` keyword.
    AverageVector,
    /// The `BEST_SCORE` keyword.
    BestScore,
    /// The `BETWEEN` keyword.
    Between,
    /// The `BOOL` keyword.
    Bool,
    /// The `BOTTOM_RIGHT` keyword.
    BottomRight,
    /// The `BY` keyword.
    By,
    /// The `CANDIDATES` keyword.
    Candidates,
    /// The `CASE` keyword.
    Case,
    /// The `CENTER` keyword.
    Center,
    /// The `CLEAR` keyword.
    Clear,
    /// The `COLLECTION` keyword.
    Collection,
    /// The `COLLECTIONS` keyword.
    Collections,
    /// The `CONSISTENCY` keyword.
    Consistency,
    /// The `CONTEXT` keyword.
    Context,
    /// The `COSINE` keyword.
    Cosine,
    /// The `COUNT` keyword.
    Count,
    /// The `CREATE` keyword.
    Create,
    /// The `CROSS` keyword.
    Cross,
    /// The `DATETIME` keyword.
    Datetime,
    /// The `DATETIME_KEY` keyword.
    DatetimeKey,
    /// The `DBSF` keyword.
    Dbsf,
    /// The `DECAY` keyword.
    Decay,
    /// The `DEFAULT` keyword.
    Default,
    /// The `DEFAULTS` keyword.
    Defaults,
    /// The `DELETE` keyword.
    Delete,
    /// The `DENSE` keyword.
    Dense,
    /// The `DESC` keyword.
    Desc,
    /// The `DISCOVER` keyword.
    Discover,
    /// The `DIVERSITY` keyword.
    Diversity,
    /// The `DOT` keyword.
    Dot,
    /// The `DROP` keyword.
    Drop,
    /// The `ELSE` keyword.
    Else,
    /// The `EMBED` keyword.
    Embed,
    /// The `EMPTY` keyword.
    Empty,
    /// The `END` keyword.
    End,
    /// The `EUCLID` keyword.
    Euclid,
    /// The `EXACT` keyword.
    Exact,
    /// The `FACET` keyword.
    Facet,
    /// The `EXCLUDE` keyword.
    Exclude,
    /// The `EXP` keyword.
    Exp,
    /// The `EXP_DECAY` keyword.
    ExpDecay,
    /// The `EXTERIOR` keyword.
    Exterior,
    /// The `FALSE` keyword.
    False,
    /// The `FEEDBACK` keyword.
    Feedback,
    /// The `FIELD` keyword.
    Field,
    /// A float literal token; also the `FLOAT` field-type keyword.
    Float,
    /// The `FOR` keyword.
    For,
    /// The `FORMULA` keyword.
    Formula,
    /// The `FROM` keyword.
    From,
    /// The `FUSION` keyword.
    Fusion,
    /// The `GAUSS_DECAY` keyword.
    GaussDecay,
    /// The `GEO` keyword.
    Geo,
    /// The `GEO_BBOX` keyword.
    GeoBbox,
    /// The `GEO_DISTANCE` keyword.
    GeoDistance,
    /// The `GEO_POLYGON` keyword.
    GeoPolygon,
    /// The `GEO_RADIUS` keyword.
    GeoRadius,
    /// The `GLOBAL` keyword.
    Global,
    /// The `GROUP` keyword.
    Group,
    /// The `HAS_VECTOR` keyword.
    HasVector,
    /// The `HNSW` keyword.
    Hnsw,
    /// The `HNSW_EF` keyword.
    HnswEf,
    /// The `HYBRID` keyword.
    Hybrid,
    /// The `ID` keyword.
    Id,
    /// The `IDF` keyword.
    Idf,
    /// The `IGNORE` keyword.
    Ignore,
    /// The `IMAGE` keyword.
    Image,
    /// The `IN` keyword.
    In,
    /// The `INCLUDE` keyword.
    Include,
    /// The `INDEX` keyword.
    Index,
    /// The `INDEXED_ONLY` keyword.
    IndexedOnly,
    /// The `INDICES` keyword.
    Indices,
    /// An integer literal token; also the `INTEGER` field-type keyword.
    Integer,
    /// The `INTERIORS` keyword.
    Interiors,
    /// The `INTO` keyword.
    Into,
    /// The `IS` keyword.
    Is,
    /// The `KEY` keyword.
    Key,
    /// The `KEYS` keyword.
    Keys,
    /// The `KEYWORD` keyword.
    Keyword,
    /// The `LAT` keyword.
    Lat,
    /// The `LIMIT` keyword.
    Limit,
    /// The `LIN_DECAY` keyword.
    LinDecay,
    /// The `LN` keyword.
    Ln,
    /// The `LOG` keyword.
    Log,
    /// The `LON` keyword.
    Lon,
    /// The `LOOKUP` keyword.
    Lookup,
    /// The `MAJORITY` keyword.
    Majority,
    /// The `MANHATTAN` keyword.
    Manhattan,
    /// The `MATCH` keyword.
    Match,
    /// The `MATCH_ANY` keyword.
    MatchAny,
    /// The `MAX` keyword.
    Max,
    /// The `MAX_SELECTIVITY` keyword.
    MaxSelectivity,
    /// The `MIDPOINT` keyword.
    Midpoint,
    /// The `MIN` keyword.
    Min,
    /// The `MMR` keyword.
    Mmr,
    /// The `MODEL` keyword.
    Model,
    /// The `MULTI` keyword.
    Multi,
    /// The `MULTIVECTOR` keyword.
    Multivector,
    /// The `NAIVE` keyword.
    Naive,
    /// The `NEAREST` keyword.
    Nearest,
    /// The `NEGATIVE` keyword.
    Negative,
    /// The `NESTED` keyword.
    Nested,
    /// The `NOT` keyword.
    Not,
    /// The `NULL` keyword.
    Null,
    /// The `OFFSET` keyword.
    Offset,
    /// The `ON` keyword.
    On,
    /// The `OPTIMIZERS` keyword.
    Optimizers,
    /// The `OR` keyword.
    Or,
    /// The `ORDER` keyword.
    Order,
    /// The `OVERSAMPLING` keyword.
    Oversampling,
    /// The `PARAMS` keyword.
    Params,
    /// The `PAYLOAD` keyword.
    Payload,
    /// The `PHRASE` keyword.
    Phrase,
    /// The `POINT` keyword.
    Point,
    /// The `POINTS` keyword.
    Points,
    /// The `POSITIVE` keyword.
    Positive,
    /// The `POW` keyword.
    Pow,
    /// The `PREFETCH` keyword.
    Prefetch,
    /// The `PREFIX` keyword.
    Prefix,
    /// The `QUANTIZATION` keyword.
    Quantization,
    /// The `QUERY` keyword.
    Query,
    /// The `QUOTA` keyword.
    Quota,
    /// The `QUOTAS` keyword.
    Quotas,
    /// The `QUORUM` keyword.
    Quorum,
    /// The `RADIUS` keyword.
    Radius,
    /// The `RANDOM` keyword.
    Random,
    /// The `RECOMMEND` keyword.
    Recommend,
    /// The `RELEVANCE` keyword.
    Relevance,
    /// The `RERANK` keyword.
    Rerank,
    /// The `RESCORE` keyword.
    Rescore,
    /// The `RRF` keyword.
    Rrf,
    /// The `RRF_K` keyword.
    RrfK,
    /// The `RRF_WEIGHTS` keyword.
    RrfWeights,
    /// The `SAMPLE` keyword.
    Sample,
    /// The `SCALE` keyword.
    Scale,
    /// The `SCORE` keyword.
    Score,
    /// The `SCROLL` keyword.
    Scroll,
    /// The `SET` keyword.
    Set,
    /// The `SHARD` keyword.
    Shard,
    /// The `SHOW` keyword.
    Show,
    /// The `SIZE` keyword.
    Size,
    /// The `SLICE` keyword.
    Slice,
    /// The `SPARSE` keyword.
    Sparse,
    /// The `SQRT` keyword.
    Sqrt,
    /// The `STRATEGY` keyword.
    Strategy,
    /// The `SUM_SCORES` keyword.
    SumScores,
    /// The `TARGET` keyword.
    Target,
    /// The `TEXT` keyword.
    Text,
    /// The `THEN` keyword.
    Then,
    /// The `THRESHOLD` keyword.
    Threshold,
    /// The `TIMEOUT` keyword.
    Timeout,
    /// The `TOP_LEFT` keyword.
    TopLeft,
    /// The `TRUE` keyword.
    True,
    /// The `TYPE` keyword.
    Type,
    /// The `UPDATE` keyword.
    Update,
    /// The `UPSERT` keyword.
    Upsert,
    /// The `USING` keyword.
    Using,
    /// The `UUID` keyword.
    Uuid,
    /// The `VALUES` keyword.
    Values,
    /// The `VALUES_COUNT` keyword.
    ValuesCount,
    /// The `VECTOR` keyword.
    Vector,
    /// The `WAIT` keyword.
    Wait,
    /// The `WHEN` keyword.
    When,
    /// The `WHERE` keyword.
    Where,
    /// The `WITH` keyword.
    With,
    /// A `*` token (wildcard or multiplication).
    Star,
    /// An identifier token (field, collection, or parameter name).
    Identifier,
    /// A string literal token (quoted, raw, backtick, or triple-quoted).
    String,
    /// A `{` token.
    Lbrace,
    /// A `}` token.
    Rbrace,
    /// A `[` token.
    Lbracket,
    /// A `]` token.
    Rbracket,
    /// A `(` token.
    Lparen,
    /// A `)` token.
    Rparen,
    /// A `:` token.
    Colon,
    /// A `,` token.
    Comma,
    /// An `=` token.
    Equals,
    /// A `!=` token.
    NotEquals,
    /// A `>` token.
    Gt,
    /// A `>=` token.
    Gte,
    /// A `<` token.
    Lt,
    /// A `<=` token.
    Lte,
    /// A `+` token.
    Plus,
    /// A `-` token.
    Minus,
    /// A `/` token.
    Slash,
    /// A `;` token.
    Semicolon,
    /// The end-of-input token.
    Eof,
}

gen_as_str! {
    Abs => "ABS",
    Acorn => "ACORN",
    Acosh => "ACOSH",
    After => "AFTER",
    All => "ALL",
    Alter => "ALTER",
    And => "AND",
    Any => "ANY",
    As => "AS",
    Asc => "ASC",
    AverageVector => "AVERAGE_VECTOR",
    BestScore => "BEST_SCORE",
    Between => "BETWEEN",
    Bool => "BOOL",
    BottomRight => "BOTTOM_RIGHT",
    By => "BY",
    Candidates => "CANDIDATES",
    Case => "CASE",
    Center => "CENTER",
    Clear => "CLEAR",
    Collection => "COLLECTION",
    Collections => "COLLECTIONS",
    Consistency => "CONSISTENCY",
    Context => "CONTEXT",
    Cosine => "COSINE",
    Count => "COUNT",
    Create => "CREATE",
    Cross => "CROSS",
    Datetime => "DATETIME",
    DatetimeKey => "DATETIME_KEY",
    Dbsf => "DBSF",
    Decay => "DECAY",
    Default => "DEFAULT",
    Defaults => "DEFAULTS",
    Delete => "DELETE",
    Dense => "DENSE",
    Desc => "DESC",
    Discover => "DISCOVER",
    Diversity => "DIVERSITY",
    Dot => "DOT",
    Drop => "DROP",
    Else => "ELSE",
    Embed => "EMBED",
    Empty => "EMPTY",
    End => "END",
    Euclid => "EUCLID",
    Exact => "EXACT",
    Facet => "FACET",
    Exclude => "EXCLUDE",
    Exp => "EXP",
    ExpDecay => "EXP_DECAY",
    Exterior => "EXTERIOR",
    False => "FALSE",
    Feedback => "FEEDBACK",
    Field => "FIELD",
    Float => "FLOAT",
    For => "FOR",
    Formula => "FORMULA",
    From => "FROM",
    Fusion => "FUSION",
    GaussDecay => "GAUSS_DECAY",
    Geo => "GEO",
    GeoBbox => "GEO_BBOX",
    GeoDistance => "GEO_DISTANCE",
    GeoPolygon => "GEO_POLYGON",
    GeoRadius => "GEO_RADIUS",
    Global => "GLOBAL",
    Group => "GROUP",
    HasVector => "HAS_VECTOR",
    Hnsw => "HNSW",
    HnswEf => "HNSW_EF",
    Hybrid => "HYBRID",
    Id => "ID",
    Idf => "IDF",
    Ignore => "IGNORE",
    Image => "IMAGE",
    In => "IN",
    Include => "INCLUDE",
    Index => "INDEX",
    IndexedOnly => "INDEXED_ONLY",
    Indices => "INDICES",
    Integer => "INTEGER",
    Interiors => "INTERIORS",
    Into => "INTO",
    Is => "IS",
    Key => "KEY",
    Keys => "KEYS",
    Keyword => "KEYWORD",
    Lat => "LAT",
    Limit => "LIMIT",
    LinDecay => "LIN_DECAY",
    Ln => "LN",
    Log => "LOG",
    Lon => "LON",
    Lookup => "LOOKUP",
    Majority => "MAJORITY",
    Manhattan => "MANHATTAN",
    Match => "MATCH",
    MatchAny => "MATCH_ANY",
    Max => "MAX",
    MaxSelectivity => "MAX_SELECTIVITY",
    Midpoint => "MIDPOINT",
    Min => "MIN",
    Mmr => "MMR",
    Model => "MODEL",
    Multi => "MULTI",
    Multivector => "MULTIVECTOR",
    Naive => "NAIVE",
    Nearest => "NEAREST",
    Negative => "NEGATIVE",
    Nested => "NESTED",
    Not => "NOT",
    Null => "NULL",
    Offset => "OFFSET",
    On => "ON",
    Optimizers => "OPTIMIZERS",
    Or => "OR",
    Order => "ORDER",
    Oversampling => "OVERSAMPLING",
    Params => "PARAMS",
    Payload => "PAYLOAD",
    Phrase => "PHRASE",
    Point => "POINT",
    Points => "POINTS",
    Positive => "POSITIVE",
    Pow => "POW",
    Prefetch => "PREFETCH",
    Prefix => "PREFIX",
    Quantization => "QUANTIZATION",
    Query => "QUERY",
    Quota => "QUOTA",
    Quotas => "QUOTAS",
    Quorum => "QUORUM",
    Radius => "RADIUS",
    Random => "RANDOM",
    Recommend => "RECOMMEND",
    Relevance => "RELEVANCE",
    Rerank => "RERANK",
    Rescore => "RESCORE",
    Rrf => "RRF",
    RrfK => "RRF_K",
    RrfWeights => "RRF_WEIGHTS",
    Sample => "SAMPLE",
    Scale => "SCALE",
    Score => "SCORE",
    Scroll => "SCROLL",
    Set => "SET",
    Shard => "SHARD",
    Show => "SHOW",
    Size => "SIZE",
    Slice => "SLICE",
    Sparse => "SPARSE",
    Sqrt => "SQRT",
    Strategy => "STRATEGY",
    SumScores => "SUM_SCORES",
    Target => "TARGET",
    Text => "TEXT",
    Then => "THEN",
    Threshold => "THRESHOLD",
    Timeout => "TIMEOUT",
    TopLeft => "TOP_LEFT",
    True => "TRUE",
    Type => "TYPE",
    Update => "UPDATE",
    Upsert => "UPSERT",
    Using => "USING",
    Uuid => "UUID",
    Values => "VALUES",
    ValuesCount => "VALUES_COUNT",
    Vector => "VECTOR",
    Wait => "WAIT",
    When => "WHEN",
    Where => "WHERE",
    With => "WITH",
    Star => "*",
    Identifier => "IDENTIFIER",
    String => "STRING",
    Lbrace => "LBRACE",
    Rbrace => "RBRACE",
    Lbracket => "LBRACKET",
    Rbracket => "RBRACKET",
    Lparen => "LPAREN",
    Rparen => "RPAREN",
    Colon => "COLON",
    Comma => "COMMA",
    Equals => "EQUALS",
    NotEquals => "NOT_EQUALS",
    Gt => "GT",
    Gte => "GTE",
    Lt => "LT",
    Lte => "LTE",
    Plus => "PLUS",
    Minus => "MINUS",
    Slash => "SLASH",
    Semicolon => "SEMICOLON",
    Eof => "EOF",
}

include!("keywords.generated.rs");

impl TokenKind {
    /// Returns true for every kind except string, punctuation, and end-of-input tokens.
    pub fn is_keyword_or_identifier(&self) -> bool {
        !matches!(
            self,
            Self::String
                | Self::Lbrace
                | Self::Rbrace
                | Self::Lbracket
                | Self::Rbracket
                | Self::Lparen
                | Self::Rparen
                | Self::Colon
                | Self::Comma
                | Self::Equals
                | Self::NotEquals
                | Self::Gt
                | Self::Gte
                | Self::Lt
                | Self::Lte
                | Self::Plus
                | Self::Minus
                | Self::Slash
                | Self::Semicolon
                | Self::Eof
        )
    }
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A lexed token: its kind, source text, and byte-offset span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Token<'a> {
    /// The token kind.
    pub kind: TokenKind,
    /// The raw source text of the token.
    pub text: &'a str,
    /// The byte-offset span of the token in the source input.
    pub span: Span,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub(crate) pos: usize,
}

impl<'a> Token<'a> {
    /// Creates a token from a kind, source text, and span.
    pub fn new(kind: TokenKind, text: &'a str, span: Span) -> Self {
        Token {
            kind,
            text,
            pos: span.start,
            span,
        }
    }

    /// Creates the end-of-input token at the given byte offset.
    pub fn eof(position: usize) -> Self {
        Token {
            kind: TokenKind::Eof,
            text: "",
            span: Span::point(position),
            pos: position,
        }
    }

    /// Returns true when this token can stand in for a keyword or identifier.
    pub fn is_keyword_or_identifier(&self) -> bool {
        match self.kind {
            TokenKind::String
            | TokenKind::Lbrace
            | TokenKind::Rbrace
            | TokenKind::Lbracket
            | TokenKind::Rbracket
            | TokenKind::Lparen
            | TokenKind::Rparen
            | TokenKind::Colon
            | TokenKind::Comma
            | TokenKind::Equals
            | TokenKind::NotEquals
            | TokenKind::Gt
            | TokenKind::Gte
            | TokenKind::Lt
            | TokenKind::Lte
            | TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Slash
            | TokenKind::Semicolon
            | TokenKind::Eof => false,
            TokenKind::Integer | TokenKind::Float => self
                .text
                .bytes()
                .next()
                .is_some_and(|b| b.is_ascii_alphabetic()),
            _ => true,
        }
    }
}

impl<'a> fmt::Display for Token<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({})", self.kind, self.text)
    }
}

/// Resolves a word to its keyword token kind, matching ASCII case-insensitively.
pub fn lookup_keyword(s: &str) -> Option<TokenKind> {
    let bytes = s.as_bytes();
    let len = bytes.len();
    if len == 0 || len > 32 {
        return None;
    }
    let mut buf = [0u8; 32];
    for (i, b) in bytes.iter().enumerate() {
        buf[i] = b.to_ascii_uppercase();
    }
    let upper = core::str::from_utf8(&buf[..len]).ok()?;
    KEYWORDS.get(upper).copied()
}
