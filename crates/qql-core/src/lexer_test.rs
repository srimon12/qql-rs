#[cfg(test)]
mod tests {
    use crate::lexer::Lexer;
    use crate::token::Token;
    use crate::token::TokenKind;
    use alloc::vec::Vec;

    fn tokenize(input: &str) -> Vec<Token<'_>> {
        Lexer::new(input).filter_map(|r| r.ok()).collect()
    }

    fn tokenize_full(input: &str) -> Result<Vec<Token<'_>>, crate::error::QqlError> {
        Lexer::new(input).collect()
    }

    // ---------------------------------------------------------------
    // Keywords
    // ---------------------------------------------------------------
    #[test]
    fn test_tokenize_keywords() {
        let cases = [
            ("UPSERT", TokenKind::Upsert),
            ("INTO", TokenKind::Into),
            ("COLLECTION", TokenKind::Collection),
            ("VALUES", TokenKind::Values),
            ("USING", TokenKind::Using),
            ("MODEL", TokenKind::Model),
            ("HYBRID", TokenKind::Hybrid),
            ("DENSE", TokenKind::Dense),
            ("SPARSE", TokenKind::Sparse),
            ("RERANK", TokenKind::Rerank),
            ("EXACT", TokenKind::Exact),
            ("WITH", TokenKind::With),
            ("ACORN", TokenKind::Acorn),
            ("QUANTIZE", TokenKind::Quantize),
            ("SCALAR", TokenKind::Scalar),
            ("BINARY", TokenKind::Binary),
            ("PRODUCT", TokenKind::Product),
            ("QUANTILE", TokenKind::Quantile),
            ("ALWAYS", TokenKind::Always),
            ("RAM", TokenKind::Ram),
            ("CREATE", TokenKind::Create),
            ("DROP", TokenKind::Drop),
            ("SHOW", TokenKind::Show),
            ("COLLECTIONS", TokenKind::Collections),
            ("RECOMMEND", TokenKind::Recommend),
            ("LIMIT", TokenKind::Limit),
            ("GROUP", TokenKind::Group),
            ("GROUP_SIZE", TokenKind::GroupSize),
            ("STRATEGY", TokenKind::Strategy),
            ("DELETE", TokenKind::Delete),
            ("UPDATE", TokenKind::Update),
            ("VECTOR", TokenKind::Vector),
            ("PAYLOAD", TokenKind::Payload),
            ("FROM", TokenKind::From),
            ("WHERE", TokenKind::Where),
            ("ID", TokenKind::Id),
            ("AND", TokenKind::And),
            ("OR", TokenKind::Or),
            ("NOT", TokenKind::Not),
            ("IN", TokenKind::In),
            ("BETWEEN", TokenKind::Between),
            ("IS", TokenKind::Is),
            ("NULL", TokenKind::Null),
            ("EMPTY", TokenKind::Empty),
            ("MATCH", TokenKind::Match),
            ("ANY", TokenKind::Any),
            ("PHRASE", TokenKind::Phrase),
            ("SELECT", TokenKind::Select),
            ("AS", TokenKind::As),
            ("TURBO", TokenKind::Turbo),
            ("BITS", TokenKind::Bits),
            ("HNSW", TokenKind::Hnsw),
            ("VECTORS", TokenKind::Vectors),
            ("OPTIMIZERS", TokenKind::Optimizers),
            ("PARAMS", TokenKind::Params),
            ("DISABLED", TokenKind::Disabled),
            ("ALTER", TokenKind::Alter),
            ("SCROLL", TokenKind::Scroll),
            ("AFTER", TokenKind::After),
            ("BY", TokenKind::By),
            ("SET", TokenKind::Set),
            ("INDEX", TokenKind::Index),
            ("ON", TokenKind::On),
            ("FOR", TokenKind::For),
            ("TYPE", TokenKind::Type),
            ("OFFSET", TokenKind::Offset),
            ("SCORE", TokenKind::Score),
            ("THRESHOLD", TokenKind::Threshold),
            ("LOOKUP", TokenKind::Lookup),
            ("COSINE", TokenKind::Cosine),
            ("DOT", TokenKind::Dot),
            ("EUCLID", TokenKind::Euclid),
            ("MANHATTAN", TokenKind::Manhattan),
            ("ORDER", TokenKind::Order),
            ("ASC", TokenKind::Asc),
            ("DESC", TokenKind::Desc),
            ("QUERY", TokenKind::Query),
            ("NEAREST", TokenKind::Nearest),
            ("CONTEXT", TokenKind::Context),
            ("DISCOVER", TokenKind::Discover),
            ("PAIRS", TokenKind::Pairs),
            ("TARGET", TokenKind::Target),
            ("GEO_BBOX", TokenKind::GeoBbox),
            ("GEO_RADIUS", TokenKind::GeoRadius),
            ("VALUES_COUNT", TokenKind::ValuesCount),
            ("HAS_VECTOR", TokenKind::HasVector),
            ("PREFETCH", TokenKind::Prefetch),
            ("FUSION", TokenKind::Fusion),
            ("SAMPLE", TokenKind::Sample),
            ("BOOST", TokenKind::Boost),
            ("DEFAULTS", TokenKind::Defaults),
            ("CASE", TokenKind::Case),
            ("WHEN", TokenKind::When),
            ("THEN", TokenKind::Then),
            ("ELSE", TokenKind::Else),
            ("END", TokenKind::End),
            ("RELEVANCE", TokenKind::Relevance),
            ("FEEDBACK", TokenKind::Feedback),
        ];

        for &(input, expected_kind) in &cases {
            let tokens = tokenize(input);
            assert_eq!(
                tokens.len(),
                1,
                "keyword '{}' should produce exactly one token",
                input
            );
            assert_eq!(tokens[0].kind, expected_kind, "keyword '{}'", input);
            assert_eq!(tokens[0].text, input, "keyword '{}' text", input);
            assert_eq!(tokens[0].pos, 0, "keyword '{}' pos", input);
        }
    }

    // ---------------------------------------------------------------
    // Case-insensitive keywords
    // ---------------------------------------------------------------
    #[test]
    fn test_tokenize_keywords_case_insensitive() {
        let cases = [
            ("upsert", "upsert", TokenKind::Upsert),
            ("Upsert", "Upsert", TokenKind::Upsert),
            ("where", "where", TokenKind::Where),
            ("WhErE", "WhErE", TokenKind::Where),
            ("from", "from", TokenKind::From),
            ("FROM", "FROM", TokenKind::From),
            ("SELECT", "SELECT", TokenKind::Select),
            ("select", "select", TokenKind::Select),
            ("Select", "Select", TokenKind::Select),
        ];

        for &(input, expected_text, expected_kind) in &cases {
            let tokens = tokenize(input);
            assert_eq!(tokens.len(), 1, "case test '{}'", input);
            assert_eq!(tokens[0].kind, expected_kind, "case test '{}' kind", input);
            assert_eq!(tokens[0].text, expected_text, "case test '{}' text", input);
        }
    }

    // ---------------------------------------------------------------
    // String literals
    // ---------------------------------------------------------------
    #[test]
    fn test_tokenize_string_literals() {
        let cases = [
            ("\"hello\"", "hello", 0usize),
            ("'world'", "world", 0),
            ("\"\"", "", 0),
            ("''", "", 0),
        ];

        for &(input, expected_text, expected_pos) in &cases {
            let tokens = tokenize(input);
            assert_eq!(tokens.len(), 1, "string '{}'", input);
            assert_eq!(tokens[0].kind, TokenKind::String, "string '{}' kind", input);
            assert_eq!(tokens[0].text, expected_text, "string '{}' text", input);
            assert_eq!(tokens[0].pos, expected_pos, "string '{}' pos", input);
        }
    }

    #[test]
    fn test_tokenize_string_escape_sequences() {
        // Rust lexer does not process escape sequences; the raw escaped text
        // is included in the token text.  Test current behavior.
        let cases = [
            ("\"hello\\\"world\"", "hello\\\"world"),
            ("'hello\\'world'", "hello\\'world"),
            ("\"hello\\\\world\"", "hello\\\\world"),
            ("\"hello\\nworld\"", "hello\\nworld"),
            ("\"hello\\tworld\"", "hello\\tworld"),
        ];

        for &(input, expected_text) in &cases {
            let tokens = tokenize(input);
            assert_eq!(tokens.len(), 1, "escaped string '{}'", input);
            assert_eq!(
                tokens[0].kind,
                TokenKind::String,
                "escaped string '{}' kind",
                input
            );
            assert_eq!(
                tokens[0].text, expected_text,
                "escaped string '{}' text",
                input
            );
        }
    }

    // ---------------------------------------------------------------
    // Unterminated strings
    // ---------------------------------------------------------------
    #[test]
    fn test_tokenize_unterminated_string() {
        let cases = ["\"hello", "'world", "\"hello\\n"];

        for &input in &cases {
            let result = tokenize_full(input);
            assert!(
                result.is_err(),
                "unterminated string '{}' should error",
                input
            );
        }
    }

    // ---------------------------------------------------------------
    // Numbers (integers and floats)
    // ---------------------------------------------------------------
    #[test]
    fn test_tokenize_numbers() {
        let cases = [
            ("123", TokenKind::Integer, "123", 0usize),
            ("123.456", TokenKind::Float, "123.456", 0),
            ("0", TokenKind::Integer, "0", 0),
            ("1.5", TokenKind::Float, "1.5", 0),
        ];

        for &(input, expected_kind, expected_text, expected_pos) in &cases {
            let tokens = tokenize(input);
            assert_eq!(tokens.len(), 1, "number '{}'", input);
            assert_eq!(tokens[0].kind, expected_kind, "number '{}' kind", input);
            assert_eq!(tokens[0].text, expected_text, "number '{}' text", input);
            assert_eq!(tokens[0].pos, expected_pos, "number '{}' pos", input);
        }
    }

    #[test]
    fn test_tokenize_negative_numbers() {
        let cases = [
            ("-123", TokenKind::Integer, "-123", 0usize),
            ("-123.456", TokenKind::Float, "-123.456", 0),
            ("-0", TokenKind::Integer, "-0", 0),
        ];

        for &(input, expected_kind, expected_text, expected_pos) in &cases {
            let tokens = tokenize(input);
            assert_eq!(tokens.len(), 1, "negative number '{}'", input);
            assert_eq!(
                tokens[0].kind, expected_kind,
                "negative number '{}' kind",
                input
            );
            assert_eq!(
                tokens[0].text, expected_text,
                "negative number '{}' text",
                input
            );
            assert_eq!(
                tokens[0].pos, expected_pos,
                "negative number '{}' pos",
                input
            );
        }
    }

    #[test]
    fn test_tokenize_minus_operator() {
        // A lone '-' without following digit should be a Minus token
        let tokens = tokenize("-");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Minus);
        assert_eq!(tokens[0].text, "-");
    }

    // ---------------------------------------------------------------
    // Identifiers
    // ---------------------------------------------------------------
    #[test]
    fn test_tokenize_identifiers() {
        let cases = [
            ("foo", "foo", 0usize),
            ("foo_bar", "foo_bar", 0),
            ("foo123", "foo123", 0),
            ("_private", "_private", 0),
        ];

        for &(input, expected_text, expected_pos) in &cases {
            let tokens = tokenize(input);
            assert_eq!(tokens.len(), 1, "identifier '{}'", input);
            assert_eq!(
                tokens[0].kind,
                TokenKind::Identifier,
                "identifier '{}' kind",
                input
            );
            assert_eq!(tokens[0].text, expected_text, "identifier '{}' text", input);
            assert_eq!(tokens[0].pos, expected_pos, "identifier '{}' pos", input);
        }
    }

    #[test]
    fn test_tokenize_dotted_paths() {
        let cases = [
            ("meta.source", "meta.source", 0usize),
            ("country.cities.population", "country.cities.population", 0),
            (
                "country.cities[].population",
                "country.cities[].population",
                0,
            ),
            ("meta.from", "meta.from", 0),
        ];

        for &(input, expected_text, expected_pos) in &cases {
            let tokens = tokenize(input);
            assert_eq!(tokens.len(), 1, "dotted '{}'", input);
            assert_eq!(
                tokens[0].kind,
                TokenKind::Identifier,
                "dotted '{}' kind",
                input
            );
            assert_eq!(tokens[0].text, expected_text, "dotted '{}' text", input);
            assert_eq!(tokens[0].pos, expected_pos, "dotted '{}' pos", input);
        }
    }

    #[test]
    fn test_tokenize_score_variable() {
        let tokens = tokenize("$score");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Identifier);
        assert_eq!(tokens[0].text, "$score");
    }

    // ---------------------------------------------------------------
    // Operators
    // ---------------------------------------------------------------
    #[test]
    fn test_tokenize_comparison_operators() {
        let cases = [
            ("=", TokenKind::Equals, "="),
            ("!=", TokenKind::NotEquals, "!="),
            (">", TokenKind::Gt, ">"),
            (">=", TokenKind::Gte, ">="),
            ("<", TokenKind::Lt, "<"),
            ("<=", TokenKind::Lte, "<="),
        ];

        for &(input, expected_kind, expected_text) in &cases {
            let tokens = tokenize(input);
            assert_eq!(tokens.len(), 1, "operator '{}'", input);
            assert_eq!(tokens[0].kind, expected_kind, "operator '{}' kind", input);
            assert_eq!(tokens[0].text, expected_text, "operator '{}' text", input);
            assert_eq!(tokens[0].pos, 0, "operator '{}' pos", input);
        }
    }

    #[test]
    fn test_tokenize_arithmetic_operators() {
        let cases = [("+", TokenKind::Plus, "+"), ("/", TokenKind::Slash, "/")];

        for &(input, expected_kind, expected_text) in &cases {
            let tokens = tokenize(input);
            assert_eq!(tokens.len(), 1, "op '{}'", input);
            assert_eq!(tokens[0].kind, expected_kind, "op '{}' kind", input);
            assert_eq!(tokens[0].text, expected_text, "op '{}' text", input);
        }
    }

    // ---------------------------------------------------------------
    // Punctuation
    // ---------------------------------------------------------------
    #[test]
    fn test_tokenize_punctuation() {
        let cases = [
            ("{", TokenKind::Lbrace, "{"),
            ("}", TokenKind::Rbrace, "}"),
            ("[", TokenKind::Lbracket, "["),
            ("]", TokenKind::Rbracket, "]"),
            ("(", TokenKind::Lparen, "("),
            (")", TokenKind::Rparen, ")"),
            (":", TokenKind::Colon, ":"),
            (",", TokenKind::Comma, ","),
            ("*", TokenKind::Star, "*"),
        ];

        for &(input, expected_kind, expected_text) in &cases {
            let tokens = tokenize(input);
            assert_eq!(tokens.len(), 1, "punct '{}'", input);
            assert_eq!(tokens[0].kind, expected_kind, "punct '{}' kind", input);
            assert_eq!(tokens[0].text, expected_text, "punct '{}' text", input);
            assert_eq!(tokens[0].pos, 0, "punct '{}' pos", input);
        }
    }

    // ---------------------------------------------------------------
    // Full INSERT query
    // ---------------------------------------------------------------
    #[test]
    fn test_tokenize_full_upsert_query() {
        let input = r#"UPSERT INTO mycol VALUES {"text": "hello", "vector": [0.1, 0.2]}"#;
        let tokens = tokenize(input);

        let expected = [
            (TokenKind::Upsert, "UPSERT", 0),
            (TokenKind::Into, "INTO", 7),
            (TokenKind::Identifier, "mycol", 12),
            (TokenKind::Values, "VALUES", 18),
            (TokenKind::Lbrace, "{", 25),
            (TokenKind::String, "text", 26),
            (TokenKind::Colon, ":", 32),
            (TokenKind::String, "hello", 34),
            (TokenKind::Comma, ",", 41),
            (TokenKind::String, "vector", 43),
            (TokenKind::Colon, ":", 51),
            (TokenKind::Lbracket, "[", 53),
            (TokenKind::Float, "0.1", 54),
            (TokenKind::Comma, ",", 57),
            (TokenKind::Float, "0.2", 59),
            (TokenKind::Rbracket, "]", 62),
            (TokenKind::Rbrace, "}", 63),
        ];

        assert_eq!(
            tokens.len(),
            expected.len(),
            "expected {} tokens, got {}",
            expected.len(),
            tokens.len()
        );

        for (i, &(ref expected_kind, expected_text, expected_pos)) in expected.iter().enumerate() {
            assert_eq!(
                tokens[i].kind, *expected_kind,
                "token {}: expected kind {:?}, got {:?}",
                i, expected_kind, tokens[i].kind
            );
            assert_eq!(
                tokens[i].text, expected_text,
                "token {}: expected text '{}', got '{}'",
                i, expected_text, tokens[i].text
            );
            assert_eq!(
                tokens[i].pos, expected_pos,
                "token {}: expected pos {}, got {}",
                i, expected_pos, tokens[i].pos
            );
        }
    }

    // ---------------------------------------------------------------
    // Search query (QUERY NEAREST ... FROM ... LIMIT ...)
    // ---------------------------------------------------------------
    #[test]
    fn test_tokenize_search_query() {
        let input = "QUERY NEAREST 'query text' FROM mycol LIMIT 10";
        let tokens = tokenize(input);

        let expected = [
            (TokenKind::Query, "QUERY", 0),
            (TokenKind::Nearest, "NEAREST", 6),
            (TokenKind::String, "query text", 14),
            (TokenKind::From, "FROM", 27),
            (TokenKind::Identifier, "mycol", 32),
            (TokenKind::Limit, "LIMIT", 38),
            (TokenKind::Integer, "10", 44),
        ];

        assert_eq!(tokens.len(), expected.len());
        for (i, &(kind, text, pos)) in expected.iter().enumerate() {
            assert_eq!(tokens[i].kind, kind, "token {} kind", i);
            assert_eq!(tokens[i].text, text, "token {} text", i);
            assert_eq!(tokens[i].pos, pos, "token {} pos", i);
        }
    }

    // ---------------------------------------------------------------
    // WHERE clause with operators
    // ---------------------------------------------------------------
    #[test]
    fn test_tokenize_where_clause() {
        let input = "WHERE id = '123' AND score >= 0.5";
        let tokens = tokenize(input);

        let expected = [
            (TokenKind::Where, "WHERE", 0),
            (TokenKind::Id, "id", 6),
            (TokenKind::Equals, "=", 9),
            (TokenKind::String, "123", 11),
            (TokenKind::And, "AND", 17),
            (TokenKind::Score, "score", 21),
            (TokenKind::Gte, ">=", 27),
            (TokenKind::Float, "0.5", 30),
        ];

        assert_eq!(tokens.len(), expected.len());
        for (i, &(kind, text, pos)) in expected.iter().enumerate() {
            assert_eq!(tokens[i].kind, kind, "token {} kind", i);
            assert_eq!(tokens[i].text, text, "token {} text", i);
            assert_eq!(tokens[i].pos, pos, "token {} pos", i);
        }
    }

    // ---------------------------------------------------------------
    // Real QQL statements
    // ---------------------------------------------------------------
    #[test]
    fn test_tokenize_qql_search() {
        let input = "QUERY 'search' FROM docs LIMIT 5 USING HYBRID";
        let tokens = tokenize(input);

        let expected = [
            (TokenKind::Query, "QUERY", 0),
            (TokenKind::String, "search", 6),
            (TokenKind::From, "FROM", 15),
            (TokenKind::Identifier, "docs", 20),
            (TokenKind::Limit, "LIMIT", 25),
            (TokenKind::Integer, "5", 31),
            (TokenKind::Using, "USING", 33),
            (TokenKind::Hybrid, "HYBRID", 39),
        ];

        assert_eq!(tokens.len(), expected.len());
        for (i, &(kind, text, pos)) in expected.iter().enumerate() {
            assert_eq!(
                tokens[i].kind, kind,
                "qql_search token {} kind: got {:?}",
                i, tokens[i].kind
            );
            assert_eq!(tokens[i].text, text, "qql_search token {} text", i);
            assert_eq!(tokens[i].pos, pos, "qql_search token {} pos", i);
        }
    }

    #[test]
    fn test_tokenize_qql_upsert() {
        let input = "UPSERT INTO coll VALUES {'id': 1, 'text': 'hello'} USING HYBRID";
        let tokens = tokenize(input);

        let expected = [
            (TokenKind::Upsert, "UPSERT", 0),
            (TokenKind::Into, "INTO", 7),
            (TokenKind::Identifier, "coll", 12),
            (TokenKind::Values, "VALUES", 17),
            (TokenKind::Lbrace, "{", 24),
            (TokenKind::String, "id", 25),
            (TokenKind::Colon, ":", 29),
            (TokenKind::Integer, "1", 31),
            (TokenKind::Comma, ",", 32),
            (TokenKind::String, "text", 34),
            (TokenKind::Colon, ":", 40),
            (TokenKind::String, "hello", 42),
            (TokenKind::Rbrace, "}", 49),
            (TokenKind::Using, "USING", 51),
            (TokenKind::Hybrid, "HYBRID", 57),
        ];

        assert_eq!(tokens.len(), expected.len());
        for (i, &(kind, text, pos)) in expected.iter().enumerate() {
            assert_eq!(tokens[i].kind, kind, "qql_insert token {} kind", i);
            assert_eq!(tokens[i].text, text, "qql_insert token {} text", i);
            assert_eq!(tokens[i].pos, pos, "qql_insert token {} pos", i);
        }
    }

    #[test]
    fn test_tokenize_create_collection() {
        let input = "CREATE COLLECTION docs (dense VECTOR(384, COSINE))";
        let tokens = tokenize(input);

        let expected = [
            (TokenKind::Create, "CREATE", 0),
            (TokenKind::Collection, "COLLECTION", 7),
            (TokenKind::Identifier, "docs", 18),
            (TokenKind::Lparen, "(", 23),
            (TokenKind::Dense, "dense", 24),
            (TokenKind::Vector, "VECTOR", 30),
            (TokenKind::Lparen, "(", 36),
            (TokenKind::Integer, "384", 37),
            (TokenKind::Comma, ",", 40),
            (TokenKind::Cosine, "COSINE", 42),
            (TokenKind::Rparen, ")", 48),
            (TokenKind::Rparen, ")", 49),
        ];

        assert_eq!(tokens.len(), expected.len());
        for (i, &(kind, text, pos)) in expected.iter().enumerate() {
            assert_eq!(tokens[i].kind, kind, "create token {} kind", i);
            assert_eq!(tokens[i].text, text, "create token {} text", i);
            assert_eq!(tokens[i].pos, pos, "create token {} pos", i);
        }
    }

    // ---------------------------------------------------------------
    // Unexpected characters
    // ---------------------------------------------------------------
    #[test]
    fn test_tokenize_unexpected_character() {
        let result = tokenize_full("@");
        assert!(result.is_err(), "'@' should produce an error");
    }

    #[test]
    fn test_tokenize_unexpected_bang() {
        let result = tokenize_full("!");
        assert!(result.is_err(), "bare '!' should produce an error");
    }

    // ---------------------------------------------------------------
    // Empty / whitespace-only input
    // ---------------------------------------------------------------
    #[test]
    fn test_tokenize_empty_input() {
        let tokens = tokenize("");
        assert!(tokens.is_empty(), "empty input should produce no tokens");
    }

    #[test]
    fn test_tokenize_only_whitespace() {
        let tokens = tokenize("   \t\n\r  ");
        assert!(
            tokens.is_empty(),
            "whitespace-only should produce no tokens"
        );
    }

    // ---------------------------------------------------------------
    // Whitespace handling
    // ---------------------------------------------------------------
    #[test]
    fn test_tokenize_whitespace() {
        let input = "  UPSERT   INTO   COLLECTION  ";
        let tokens = tokenize(input);

        let expected = [
            (TokenKind::Upsert, "UPSERT", 2),
            (TokenKind::Into, "INTO", 11),
            (TokenKind::Collection, "COLLECTION", 18),
        ];

        assert_eq!(tokens.len(), expected.len());
        for (i, &(kind, text, pos)) in expected.iter().enumerate() {
            assert_eq!(tokens[i].kind, kind, "whitespace token {} kind", i);
            assert_eq!(tokens[i].text, text, "whitespace token {} text", i);
            assert_eq!(tokens[i].pos, pos, "whitespace token {} pos", i);
        }
    }

    #[test]
    fn test_tokenize_tabs_and_newlines() {
        let input = "UPSERT\tINTO\nCOLLECTION";
        let tokens = tokenize(input);

        let expected = [
            (TokenKind::Upsert, "UPSERT", 0),
            (TokenKind::Into, "INTO", 7),
            (TokenKind::Collection, "COLLECTION", 12),
        ];

        assert_eq!(tokens.len(), expected.len());
        for (i, &(kind, text, pos)) in expected.iter().enumerate() {
            assert_eq!(tokens[i].kind, kind, "tabs token {} kind", i);
            assert_eq!(tokens[i].text, text, "tabs token {} text", i);
            assert_eq!(tokens[i].pos, pos, "tabs token {} pos", i);
        }
    }

    // ---------------------------------------------------------------
    // SQL filter keywords
    // ---------------------------------------------------------------
    #[test]
    fn test_tokenize_sql_filter_keywords() {
        let cases = [
            ("IN", TokenKind::In),
            ("NOT", TokenKind::Not),
            ("BETWEEN", TokenKind::Between),
            ("IS", TokenKind::Is),
            ("NULL", TokenKind::Null),
            ("EMPTY", TokenKind::Empty),
            ("MATCH", TokenKind::Match),
            ("ANY", TokenKind::Any),
            ("PHRASE", TokenKind::Phrase),
        ];

        for &(input, expected_kind) in &cases {
            let tokens = tokenize(input);
            assert_eq!(tokens.len(), 1, "filter kw '{}'", input);
            assert_eq!(tokens[0].kind, expected_kind, "filter kw '{}'", input);
        }
    }

    #[test]
    fn test_tokenize_is_not_null_and_friends() {
        // "IS NOT NULL" should produce three tokens: IS, NOT, NULL
        let input = "IS NOT NULL";
        let tokens = tokenize(input);
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].kind, TokenKind::Is);
        assert_eq!(tokens[0].text, "IS");
        assert_eq!(tokens[1].kind, TokenKind::Not);
        assert_eq!(tokens[1].text, "NOT");
        assert_eq!(tokens[2].kind, TokenKind::Null);
        assert_eq!(tokens[2].text, "NULL");

        // "IS NULL" -> IS, NULL
        let tokens = tokenize("IS NULL");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::Is);
        assert_eq!(tokens[1].kind, TokenKind::Null);

        // "IS EMPTY" -> IS, EMPTY
        let tokens = tokenize("IS EMPTY");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::Is);
        assert_eq!(tokens[1].kind, TokenKind::Empty);
    }

    #[test]
    fn test_tokenize_in_and_not_in() {
        // "NOT IN" -> NOT, IN
        let tokens = tokenize("NOT IN");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::Not);
        assert_eq!(tokens[1].kind, TokenKind::In);

        // "IN" -> IN
        let tokens = tokenize("IN");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::In);
    }

    // ---------------------------------------------------------------
    // Geo / misc keywords unique to Rust token set
    // ---------------------------------------------------------------
    #[test]
    fn test_tokenize_geo_keywords() {
        let cases = [
            ("GEO_BBOX", TokenKind::GeoBbox),
            ("GEO_RADIUS", TokenKind::GeoRadius),
            ("VALUES_COUNT", TokenKind::ValuesCount),
            ("HAS_VECTOR", TokenKind::HasVector),
        ];

        for &(input, expected_kind) in &cases {
            let tokens = tokenize(input);
            assert_eq!(tokens.len(), 1, "geo kw '{}'", input);
            assert_eq!(tokens[0].kind, expected_kind, "geo kw '{}'", input);
        }
    }

    // ---------------------------------------------------------------
    // CASE / WHEN / THEN / ELSE / END keywords
    // ---------------------------------------------------------------
    #[test]
    fn test_tokenize_case_when_keywords() {
        let cases = [
            ("CASE", TokenKind::Case),
            ("WHEN", TokenKind::When),
            ("THEN", TokenKind::Then),
            ("ELSE", TokenKind::Else),
            ("END", TokenKind::End),
            ("BOOST", TokenKind::Boost),
            ("DEFAULTS", TokenKind::Defaults),
        ];

        for &(input, expected_kind) in &cases {
            let tokens = tokenize(input);
            assert_eq!(tokens.len(), 1, "case kw '{}'", input);
            assert_eq!(tokens[0].kind, expected_kind, "case kw '{}'", input);
        }
    }

    // ---------------------------------------------------------------
    // SELECT keyword (used in QQL-style queries)
    // ---------------------------------------------------------------
    #[test]
    fn test_tokenize_select() {
        let tokens = tokenize("SELECT");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Select);
    }

    // ---------------------------------------------------------------
    // TokenKind string representation (as_str / Display)
    // ---------------------------------------------------------------
    #[test]
    fn test_token_kind_string() {
        let cases = [
            (TokenKind::Upsert, "UPSERT"),
            (TokenKind::Eof, "EOF"),
            (TokenKind::Identifier, "IDENTIFIER"),
            (TokenKind::String, "STRING"),
            (TokenKind::Integer, "INTEGER"),
            (TokenKind::Float, "FLOAT"),
        ];

        for (kind, expected_str) in cases {
            assert_eq!(kind.as_str(), expected_str, "TokenKind {:?}.as_str()", kind);
            // Display should match
            let display = alloc::format!("{}", kind);
            assert_eq!(display, expected_str, "TokenKind {:?} Display", kind);
        }
    }

    // ---------------------------------------------------------------
    // Token Display (String representation)
    // ---------------------------------------------------------------
    #[test]
    fn test_token_display() {
        let token = Token::new(TokenKind::Upsert, "UPSERT", 0);
        let s = alloc::format!("{}", token);
        assert_eq!(s, "UPSERT(UPSERT)");
    }

    // ---------------------------------------------------------------
    // EOF token (struct check)
    // ---------------------------------------------------------------
    #[test]
    fn test_eof_token() {
        let eof = Token::eof();
        assert_eq!(eof.kind, TokenKind::Eof);
        assert_eq!(eof.text, "");
        assert_eq!(eof.pos, 0);
    }
}
