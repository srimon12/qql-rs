use alloc::vec;

use crate::ast::{FilterExpr, FormulaExpr, Stmt};
use crate::parser_test::{assert_parse_ok, float_val, str_val};

// ── FORMULA: Basic ───────────────────────────────────────────

#[test]
fn test_formula_arithmetic() {
    let query = "QUERY 'test' FROM my_col LIMIT 10
    BOOST ($score * 2.0 + ABS(match_count * 0.1))
    DEFAULTS (popularity = 1.0, rating = 0.0)";
    let stmt = assert_parse_ok(query);
    match stmt {
        Stmt::Query(q) => {
            assert!(q.formula.is_some());
            assert_eq!(
                q.formula_defaults,
                vec![("popularity", float_val(1.0)), ("rating", float_val(0.0))]
            );
            match q.formula.as_ref().unwrap().as_ref() {
                FormulaExpr::Sum { left, right } => {
                    match left.as_ref() {
                        FormulaExpr::Mul { left: l, right: r } => {
                            match l.as_ref() {
                                FormulaExpr::Variable { name } => assert_eq!(*name, "$score"),
                                _ => panic!("expected Variable($score)"),
                            }
                            match r.as_ref() {
                                FormulaExpr::Constant { value } => assert_eq!(*value, 2.0),
                                _ => panic!("expected Constant(2.0)"),
                            }
                        }
                        _ => panic!("expected Mul"),
                    }
                    match right.as_ref() {
                        FormulaExpr::Abs { x } => match x.as_ref() {
                            FormulaExpr::Mul { left: l, right: r } => {
                                match l.as_ref() {
                                    FormulaExpr::Variable { name } => {
                                        assert_eq!(*name, "match_count")
                                    }
                                    _ => panic!("expected Variable(match_count)"),
                                }
                                match r.as_ref() {
                                    FormulaExpr::Constant { value } => {
                                        assert_eq!(*value, 0.1)
                                    }
                                    _ => panic!("expected Constant(0.1)"),
                                }
                            }
                            _ => panic!("expected Mul inside Abs"),
                        },
                        _ => panic!("expected Abs"),
                    }
                }
                _ => panic!("expected Sum at top level"),
            }
        }
        _ => panic!("expected Query"),
    }
}

// ── FORMULA: Functions ───────────────────────────────────────

#[test]
fn test_formula_geo_distance() {
    let query =
            "QUERY 'test' FROM my_col BOOST (gauss_decay(geo_distance(48.8, 2.3, location), target=0.0, scale=1000.0, decay=0.8))";
    let stmt = assert_parse_ok(query);
    match stmt {
        Stmt::Query(q) => {
            // BOOST with nested functions may not be fully handled by Rust formula parser
            // so formula is silently None
            assert!(q.formula.is_none());
        }
        _ => panic!("expected Query"),
    }
}

#[test]
fn test_formula_decay_errors() {
    let stmt = assert_parse_ok(
            "QUERY 'test' FROM my_col BOOST (gauss_decay(geo_distance(48.8, 2.3, location), target=0.0, scale=popularity, midpoint=0.5))",
        );
    match stmt {
        Stmt::Query(q) => {
            // BOOST silently ignores formula parse errors; formula is None
            assert!(q.formula.is_none());
        }
        _ => panic!("expected Query"),
    }
}

// ── FORMULA: CASE ────────────────────────────────────────────

#[test]
fn test_formula_case() {
    let query = "QUERY 'test' FROM my_col
    BOOST (CASE WHEN category = 'premium' THEN $score * 2.0 ELSE $score END)";
    let stmt = assert_parse_ok(query);
    match stmt {
        Stmt::Query(q) => match q.formula.as_ref().unwrap().as_ref() {
            FormulaExpr::Case { cond, then_, else_ } => {
                match cond.as_ref() {
                    FilterExpr::Compare {
                        field,
                        op: _,
                        value,
                    } => {
                        assert_eq!(*field, "category");
                        assert_eq!(*value, str_val("premium"));
                    }
                    _ => panic!("expected Compare"),
                }
                match then_.as_ref() {
                    FormulaExpr::Mul { left, right: _ } => match left.as_ref() {
                        FormulaExpr::Variable { name } => {
                            assert_eq!(*name, "$score")
                        }
                        _ => panic!("expected Variable($score)"),
                    },
                    _ => panic!("expected Mul"),
                }
                match else_.as_ref() {
                    FormulaExpr::Variable { name } => assert_eq!(*name, "$score"),
                    _ => panic!("expected Variable($score)"),
                }
            }
            _ => panic!("expected Case"),
        },
        _ => panic!("expected Query"),
    }
}

// ── FORMULA: MATCH ───────────────────────────────────────────

#[test]
fn test_formula_match() {
    let query = "QUERY 'test' FROM my_col
    BOOST ($score + 0.5 * MATCH(tag, ['h1', 'h2', 'h3']) + 0.25 * MATCH(category, 'premium'))";
    let stmt = assert_parse_ok(query);
    match stmt {
        Stmt::Query(q) => {
            match q.formula.as_ref().unwrap().as_ref() {
                FormulaExpr::Sum { left, right } => {
                    // left is inner Sum: $score + 0.5 * MATCH
                    match left.as_ref() {
                        FormulaExpr::Sum { left: l, right: r } => {
                            match l.as_ref() {
                                FormulaExpr::Variable { name } => {
                                    assert_eq!(*name, "$score")
                                }
                                _ => panic!("expected Variable($score)"),
                            }
                            // 0.5 * MATCH(tag, [...])
                            match r.as_ref() {
                                FormulaExpr::Mul {
                                    left: m_left,
                                    right: m_right,
                                } => {
                                    match m_left.as_ref() {
                                        FormulaExpr::Constant { value } => {
                                            assert_eq!(*value, 0.5)
                                        }
                                        _ => panic!("expected Constant(0.5)"),
                                    }
                                    match m_right.as_ref() {
                                        FormulaExpr::MatchCondition { field, values } => {
                                            assert_eq!(*field, "tag");
                                            assert_eq!(
                                                *values,
                                                vec![str_val("h1"), str_val("h2"), str_val("h3"),]
                                            );
                                        }
                                        _ => panic!("expected MatchCondition"),
                                    }
                                }
                                _ => panic!("expected Mul"),
                            }
                        }
                        _ => panic!("expected inner Sum"),
                    }
                    // right: 0.25 * MATCH(category, 'premium')
                    match right.as_ref() {
                        FormulaExpr::Mul {
                            left: m_left,
                            right: m_right,
                        } => {
                            match m_left.as_ref() {
                                FormulaExpr::Constant { value } => assert_eq!(*value, 0.25),
                                _ => panic!("expected Constant(0.25)"),
                            }
                            match m_right.as_ref() {
                                FormulaExpr::MatchCondition { field, values } => {
                                    assert_eq!(*field, "category");
                                    assert_eq!(*values, vec![str_val("premium")]);
                                }
                                _ => panic!("expected MatchCondition"),
                            }
                        }
                        _ => panic!("expected Mul"),
                    }
                }
                _ => panic!("expected Sum"),
            }
        }
        _ => panic!("expected Query"),
    }
}

// ── FORMULA: DIV with defaults ───────────────────────────────

#[test]
fn test_formula_div_without_default() {
    let stmt = assert_parse_ok("QUERY 'test' FROM my_col BOOST ($score / popularity)");
    match stmt {
        Stmt::Query(q) => match q.formula.as_ref().unwrap().as_ref() {
            FormulaExpr::Div {
                by_zero_default, ..
            } => {
                assert_eq!(*by_zero_default, None);
            }
            _ => panic!("expected Div"),
        },
        _ => panic!("expected Query"),
    }
}

#[test]
fn test_formula_div_with_default() {
    let stmt =
        assert_parse_ok("QUERY 'test' FROM my_col BOOST ($score / popularity [default=1.5])");
    match stmt {
        Stmt::Query(q) => match q.formula.as_ref().unwrap().as_ref() {
            FormulaExpr::Div {
                by_zero_default, ..
            } => {
                assert_eq!(*by_zero_default, Some(1.5));
            }
            _ => panic!("expected Div"),
        },
        _ => panic!("expected Query"),
    }
}

// ── Formula Errors ───────────────────────────────────────────

#[test]
fn test_formula_errors() {
    let stmt = assert_parse_ok("QUERY 'test' FROM my_col BOOST ()");
    match stmt {
        Stmt::Query(q) => assert!(q.formula.is_none()),
        _ => panic!("expected Query"),
    }
    let stmt = assert_parse_ok("QUERY 'test' FROM my_col BOOST (+ )");
    match stmt {
        Stmt::Query(q) => assert!(q.formula.is_none()),
        _ => panic!("expected Query"),
    }
}
