use alloc::boxed::Box;
use alloc::vec;

use crate::ast::{FilterExpr, Stmt};
use crate::parser_test::{assert_parse_ok, i64_val};

// ── Filter: AND / OR / NOT ───────────────────────────────────

#[test]
fn test_filter_and() {
    let stmt = assert_parse_ok("SCROLL FROM c WHERE a = 1 AND b = 2 LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::And {
                    operands: vec![
                        FilterExpr::Compare {
                            field: String::from("a"),
                            op: String::from("="),
                            value: i64_val(1),
                        },
                        FilterExpr::Compare {
                            field: String::from("b"),
                            op: String::from("="),
                            value: i64_val(2),
                        },
                    ],
                }))
            );
        }
        _ => panic!("expected Scroll stmt"),
    }
}

#[test]
fn test_filter_or() {
    let stmt = assert_parse_ok("SCROLL FROM c WHERE a = 1 OR b = 2 LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::Or {
                    operands: vec![
                        FilterExpr::Compare {
                            field: String::from("a"),
                            op: String::from("="),
                            value: i64_val(1),
                        },
                        FilterExpr::Compare {
                            field: String::from("b"),
                            op: String::from("="),
                            value: i64_val(2),
                        },
                    ],
                }))
            );
        }
        _ => panic!("expected Scroll stmt"),
    }
}

#[test]
fn test_filter_not() {
    let stmt = assert_parse_ok("SCROLL FROM c WHERE NOT a = 1 LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::Not {
                    operand: Box::new(FilterExpr::Compare {
                        field: String::from("a"),
                        op: String::from("="),
                        value: i64_val(1),
                    }),
                }))
            );
        }
        _ => panic!("expected Scroll stmt"),
    }
}

#[test]
fn test_filter_complex() {
    let stmt =
        assert_parse_ok("SCROLL FROM c WHERE (a = 1 AND b = 2) OR (c = 3 AND NOT d = 4) LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::Or {
                    operands: vec![
                        FilterExpr::And {
                            operands: vec![
                                FilterExpr::Compare {
                                    field: String::from("a"),
                                    op: String::from("="),
                                    value: i64_val(1),
                                },
                                FilterExpr::Compare {
                                    field: String::from("b"),
                                    op: String::from("="),
                                    value: i64_val(2),
                                },
                            ],
                        },
                        FilterExpr::And {
                            operands: vec![
                                FilterExpr::Compare {
                                    field: String::from("c"),
                                    op: String::from("="),
                                    value: i64_val(3),
                                },
                                FilterExpr::Not {
                                    operand: Box::new(FilterExpr::Compare {
                                        field: String::from("d"),
                                        op: String::from("="),
                                        value: i64_val(4),
                                    }),
                                },
                            ],
                        },
                    ],
                }))
            );
        }
        _ => panic!("expected Scroll stmt"),
    }
}

#[test]
fn test_filter_precedence() {
    let stmt = assert_parse_ok("SCROLL FROM c WHERE a = 1 AND b = 2 OR c = 3 LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::Or {
                    operands: vec![
                        FilterExpr::And {
                            operands: vec![
                                FilterExpr::Compare {
                                    field: String::from("a"),
                                    op: String::from("="),
                                    value: i64_val(1),
                                },
                                FilterExpr::Compare {
                                    field: String::from("b"),
                                    op: String::from("="),
                                    value: i64_val(2),
                                },
                            ],
                        },
                        FilterExpr::Compare {
                            field: String::from("c"),
                            op: String::from("="),
                            value: i64_val(3),
                        },
                    ],
                }))
            );
        }
        _ => panic!("expected Scroll stmt"),
    }
}
