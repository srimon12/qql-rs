//! Skill examples stay parseable.
//!
//! Every `skills/qql-skill/examples/*.qql` file must parse with the current
//! grammar. This guards the skill against documenting syntax the parser
//! rejects.

use qql_core::parser::Parser;

#[test]
fn skill_examples_parse() {
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../skills/qql-skill/examples"
    );
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().map(|e| e == "qql").unwrap_or(false))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no .qql examples found");
    for path in files {
        let src = std::fs::read_to_string(&path).unwrap();
        let stmts = Parser::parse_all(&src)
            .unwrap_or_else(|e| panic!("{} failed to parse: {e:?}", path.display()));
        assert!(
            !stmts.is_empty(),
            "{} parsed to zero statements",
            path.display()
        );
    }
}
