use crate::error::{QqlError, Span};
use pest::error::InputLocation;
use pest::Parser as _;

#[derive(pest_derive::Parser)]
#[grammar = "grammar/qql.generated.pest"]
struct CanonicalSyntaxParser;

pub(super) fn validate_statement(input: &str) -> Result<(), QqlError> {
    CanonicalSyntaxParser::parse(Rule::single_statement, input)
        .map(|_| ())
        .map_err(grammar_error)
}

pub(super) fn validate_script(input: &str) -> Result<(), QqlError> {
    CanonicalSyntaxParser::parse(Rule::script, input)
        .map(|_| ())
        .map_err(grammar_error)
}

fn grammar_error(error: pest::error::Error<Rule>) -> QqlError {
    let span = match error.location {
        InputLocation::Pos(position) => Span::point(position),
        InputLocation::Span((start, end)) => Span::new(start, end),
    };
    QqlError::parse(
        "QQL-PARSE-GRAMMAR",
        "input does not match the canonical QQL grammar",
        span,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_grammar_accepts_scripts() {
        validate_script("SHOW COLLECTIONS; COUNT FROM docs;").unwrap();
    }

    #[test]
    fn canonical_grammar_rejects_legacy_statements() {
        for source in [
            "SELECT * FROM docs",
            "INSERT INTO docs VALUES {id: 1}",
            "BOOST ($score * 2)",
            "CREATE COLLECTION docs VECTORS (dense VECTOR (4, COSINE))",
            "CREATE COLLECTION docs (VECTOR (4, COSINE))",
            "CREATE COLLECTION docs (dense (4, COSINE))",
            "CREATE COLLECTION docs (dense VECTOR (4, COSINE) WITH VECTORS (on_disk = true))",
            "ALTER COLLECTION docs WITH QUANTIZE (type = 'scalar')",
            "CREATE SHARD 'tenant' ON COLLECTION docs",
            "QUERY TEXT 'x' FROM docs PARAMS (k = 30)",
            "QUERY TEXT 'x' FROM docs PARAMS (weights = [1.0])",
            "CREATE INDEX ON COLLECTION docs FOR title TYPE 'text'",
        ] {
            assert!(validate_statement(source).is_err(), "{source}");
        }
    }
}
