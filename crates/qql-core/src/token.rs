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
            pub fn as_str(&self) -> &'static str {
                match self {
                    $( Self::$var => $str, )*
                }
            }
        }
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum TokenKind {
    A,
    Abs,
    Acorn,
    After,
    All,
    Alter,
    And,
    Any,
    As,
    Asc,
    AverageVector,
    B,
    BestScore,
    Between,
    Bool,
    BottomRight,
    By,
    C,
    Candidates,
    Case,
    Center,
    Clear,
    Collection,
    Collections,
    Consistency,
    Context,
    Cosine,
    Count,
    Create,
    Cross,
    Datetime,
    DatetimeKey,
    Dbsf,
    Decay,
    Default,
    Defaults,
    Delete,
    Dense,
    Desc,
    Discover,
    Diversity,
    Dot,
    Drop,
    E,
    Else,
    Embed,
    Empty,
    End,
    Euclid,
    Exact,
    Exclude,
    Exp,
    ExpDecay,
    Exterior,
    False,
    Feedback,
    Field,
    Float,
    For,
    Formula,
    From,
    Fusion,
    GaussDecay,
    Geo,
    GeoBbox,
    GeoDistance,
    GeoPolygon,
    GeoRadius,
    Group,
    HasVector,
    Hnsw,
    HnswEf,
    Hybrid,
    Id,
    Ignore,
    Image,
    In,
    Include,
    Index,
    IndexedOnly,
    Indices,
    Integer,
    Interiors,
    Into,
    Is,
    Key,
    Keys,
    Keyword,
    Lat,
    Limit,
    LinDecay,
    Ln,
    Log,
    Lon,
    Lookup,
    Majority,
    Manhattan,
    Match,
    MatchAny,
    MaxSelectivity,
    Midpoint,
    Mmr,
    Model,
    Multi,
    Multivector,
    Naive,
    Nearest,
    Negative,
    Nested,
    Not,
    Null,
    Offset,
    On,
    Optimizers,
    Or,
    Order,
    Oversampling,
    Params,
    Payload,
    Phrase,
    Point,
    Points,
    Positive,
    Pow,
    Prefetch,
    Quantization,
    Query,
    Quorum,
    Radius,
    Random,
    Recommend,
    Relevance,
    Rerank,
    Rescore,
    Rrf,
    RrfK,
    RrfWeights,
    Sample,
    Scale,
    Score,
    Scroll,
    Set,
    Shard,
    Show,
    Size,
    Sparse,
    Sqrt,
    Strategy,
    SumScores,
    Target,
    Text,
    Then,
    Threshold,
    Timeout,
    TopLeft,
    True,
    Type,
    Update,
    Upsert,
    Using,
    Uuid,
    Values,
    ValuesCount,
    Vector,
    When,
    Where,
    With,
    X,
    Star,
    Identifier,
    String,
    Lbrace,
    Rbrace,
    Lbracket,
    Rbracket,
    Lparen,
    Rparen,
    Colon,
    Comma,
    Equals,
    NotEquals,
    Gt,
    Gte,
    Lt,
    Lte,
    Plus,
    Minus,
    Slash,
    Semicolon,
    Eof,
}

gen_as_str! {
    A => "A",
    Abs => "ABS",
    Acorn => "ACORN",
    After => "AFTER",
    All => "ALL",
    Alter => "ALTER",
    And => "AND",
    Any => "ANY",
    As => "AS",
    Asc => "ASC",
    AverageVector => "AVERAGE_VECTOR",
    B => "B",
    BestScore => "BEST_SCORE",
    Between => "BETWEEN",
    Bool => "BOOL",
    BottomRight => "BOTTOM_RIGHT",
    By => "BY",
    C => "C",
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
    E => "E",
    Else => "ELSE",
    Embed => "EMBED",
    Empty => "EMPTY",
    End => "END",
    Euclid => "EUCLID",
    Exact => "EXACT",
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
    Group => "GROUP",
    HasVector => "HAS_VECTOR",
    Hnsw => "HNSW",
    HnswEf => "HNSW_EF",
    Hybrid => "HYBRID",
    Id => "ID",
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
    MaxSelectivity => "MAX_SELECTIVITY",
    Midpoint => "MIDPOINT",
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
    Quantization => "QUANTIZATION",
    Query => "QUERY",
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
    When => "WHEN",
    Where => "WHERE",
    With => "WITH",
    X => "X",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Token<'a> {
    pub kind: TokenKind,
    pub text: &'a str,
    pub span: Span,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub(crate) pos: usize,
}

impl<'a> Token<'a> {
    pub fn new(kind: TokenKind, text: &'a str, span: Span) -> Self {
        Token {
            kind,
            text,
            pos: span.start,
            span,
        }
    }

    pub fn eof(position: usize) -> Self {
        Token {
            kind: TokenKind::Eof,
            text: "",
            span: Span::point(position),
            pos: position,
        }
    }

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
