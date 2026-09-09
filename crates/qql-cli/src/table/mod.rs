//! `psql`-style table printer for QQL CLI output.
//!
//! Produces unbordered, aligned tables with a row-count footer.
//! Detects columns automatically from `ExecResponse.data` payloads, supporting
//! both standard tabular views and expanded vertical record displays (`\x`).

mod cell;
mod columns;
mod formatters;
mod renderer;

use formatters::{
    print_collection_info, print_collections_list, print_count, print_facet_table,
    print_groups_table, print_query_table, print_quotas_table, print_shard_keys_table,
};

/// Render a [`qql::executor::ExecutionReport`] to stdout.
///
/// For QUERY/SCROLL/GET_POINTS: prints a table of id, score, and payload fields.
/// For FACET: prints a two-column Value/Count table.
/// For SHOW COLLECTIONS: prints a list table.
/// For SHOW COLLECTION: prints a key-value property table.
/// For SHOW SHARD KEYS: prints a list of custom tenant/routing shard keys.
/// For SHOW QUOTAS: prints a key-value limits table.
/// For COUNT: prints the count.
/// For DDL/DML operations: prints the status message (and affected count if present).
/// When `json` is true, prints the full JSON report instead.
pub fn render_report(
    report: &qql::executor::ExecutionReport,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if json {
        let s = serde_json::to_string_pretty(report)?;
        println!("{}", s);
        return Ok(());
    }

    if report.results.is_empty() {
        println!("(empty result)");
        return Ok(());
    }

    if report.results.len() == 1 {
        render_response(&report.results[0], false)?;
    } else {
        for (i, resp) in report.results.iter().enumerate() {
            if i > 0 {
                println!();
            }
            if report.results.len() > 1 {
                println!("── statement {} ──", i + 1);
            }
            render_response(resp, false)?;
        }
        println!("{} succeeded, {} failed", report.succeeded, report.failed);
    }
    Ok(())
}

/// Render a single `ExecResponse` to stdout.
pub fn render_response(
    response: &qql::executor::ExecResponse,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if json {
        let s = serde_json::to_string_pretty(response)?;
        println!("{}", s);
        return Ok(());
    }

    match response.operation.as_str() {
        "QUERY" | "SCROLL" | "CROSS_RERANK" | "GET_POINTS" => {
            print_query_table(&response.data)?;
        }
        "FACET" => {
            print_facet_table(&response.data)?;
        }
        "QUERY_GROUPS" => {
            print_groups_table(&response.data)?;
        }
        "COUNT" => {
            print_count(&response.data);
        }
        "SHOW_COLLECTIONS" => {
            print_collections_list(&response.data)?;
        }
        "SHOW_COLLECTION" | "show_collection" => {
            print_collection_info(&response.data)?;
        }
        "SHOW_SHARD_KEYS" => {
            print_shard_keys_table(&response.data)?;
        }
        "SHOW_QUOTAS" => {
            print_quotas_table(&response.data)?;
        }
        _ => {
            // DDL/DML: just print the message
            println!("{}", response.message);
            if let Some(ref data) = response.data {
                // For operations like UPSERT that have data (count), show it
                if let Some(count) = data.get("count").and_then(|c| c.as_u64()) {
                    println!("  count: {}", count);
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::cell::{Alignment, Cell, escape_controls, stringify_value};
    use super::columns::{detect_query_columns, query_cell};
    use super::formatters::{collection_names, count_value};
    use super::renderer::Table;
    use super::*;

    #[test]
    fn table_renders_psql_layout() {
        let mut table = Table::new(vec!["id".into(), "score".into()]);
        table.add_cells(vec![
            Cell::text("42"),
            Cell {
                value: "0.95".into(),
                alignment: Alignment::Right,
            },
        ]);

        let mut output = Vec::new();
        table.render(&mut output).unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            " id | score \n----+-------\n 42 |  0.95 \n(1 row)\n"
        );
    }

    #[test]
    fn table_with_max_col_width_truncates() {
        let mut table = Table::new(vec!["id".into(), "content".into()]).with_max_col_width(10);
        table.add_row(vec![
            "1".into(),
            "a very long text that will truncate".into(),
        ]);

        let mut output = Vec::new();
        table.render(&mut output).unwrap();
        let s = String::from_utf8(output).unwrap();
        assert!(s.contains("a very lo…"));
    }

    #[test]
    fn empty_table_renders_header_and_zero_row_footer() {
        let table = Table::new(vec!["id".into()]);
        assert!(table.is_empty());

        let mut output = Vec::new();
        table.render(&mut output).unwrap();

        assert_eq!(String::from_utf8(output).unwrap(), " id \n----\n(0 rows)\n");
    }

    #[test]
    fn rows_are_normalized_to_the_declared_columns() {
        let mut table = Table::new(vec!["one".into(), "two".into()]);
        table.add_row(vec!["value".into()]);

        let mut output = Vec::new();
        table.render(&mut output).unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            "  one  | two \n-------+-----\n value |     \n(1 row)\n"
        );
    }

    #[test]
    fn unicode_cells_align_using_terminal_width() {
        let mut table = Table::new(vec!["city".into(), "status".into()]);
        table.add_row(vec!["東京".into(), "ready".into()]);
        table.add_row(vec!["Oslo".into(), "👩‍🔬".into()]);

        let mut output = Vec::new();
        table.render(&mut output).unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            " city | status \n------+--------\n 東京 | ready  \n Oslo | 👩‍🔬     \n(2 rows)\n"
        );
    }

    #[test]
    fn control_characters_are_escaped_before_rendering() {
        assert_eq!(escape_controls("line\n\t\u{1b}"), r"line\n\t\u{1b}");
    }

    #[test]
    fn qdrant_result_envelopes_render_collection_names_and_counts() {
        let collections = Some(serde_json::json!({
            "result": {
                "collections": [{"name": "berlin_airbnb"}, {"name": "sec10k"}]
            }
        }));
        let count = Some(serde_json::json!({"result": {"count": 2500}}));

        assert_eq!(collection_names(&collections), ["berlin_airbnb", "sec10k"]);
        assert_eq!(count_value(&count), 2500);
    }

    #[test]
    fn stringify_handles_all_types() {
        assert_eq!(stringify_value(&serde_json::json!("hello")), "hello");
        assert_eq!(stringify_value(&serde_json::json!(42)), "42");
        assert_eq!(stringify_value(&serde_json::json!(true)), "true");
        assert_eq!(stringify_value(&serde_json::json!(null)), "");
        assert_eq!(stringify_value(&serde_json::json!([1, 2, 3])), "[1,2,3]");
    }

    #[test]
    fn detect_columns_from_search_hits() {
        let hits = vec![{
            let mut m = serde_json::Map::new();
            m.insert("id".into(), serde_json::json!("abc"));
            m.insert("score".into(), serde_json::json!(0.95));
            m.insert(
                "payload".into(),
                serde_json::json!({"title": "hello", "year": 2024, "nested": {"deep": true}}),
            );
            m
        }];
        let cols = detect_query_columns(&hits);
        let labels = cols
            .iter()
            .map(|column| column.label.as_str())
            .collect::<Vec<_>>();
        assert!(labels.contains(&"id"));
        assert!(labels.contains(&"score"));
        assert!(labels.contains(&"title"));
        assert!(labels.contains(&"year"));
        assert!(labels.contains(&"nested"));
    }

    #[test]
    fn query_cells_read_payload_fields_and_preserve_json() {
        let hit = serde_json::json!({
            "id": "abc",
            "score": 0.95,
            "payload": {
                "title": "hello",
                "year": 2024,
                "nested": {"deep": true}
            }
        });
        let hit = hit.as_object().unwrap();
        let columns = detect_query_columns(std::slice::from_ref(hit));
        let column = |label| columns.iter().find(|column| column.label == label).unwrap();

        assert_eq!(query_cell(hit, column("id")).value, "abc");
        assert_eq!(query_cell(hit, column("score")).value, "0.95");
        assert_eq!(query_cell(hit, column("title")).value, "hello");
        assert_eq!(query_cell(hit, column("year")).value, "2024");
        assert_eq!(query_cell(hit, column("nested")).value, r#"{"deep":true}"#);
        assert_eq!(query_cell(hit, column("year")).alignment, Alignment::Right);
    }

    #[test]
    fn colliding_payload_keys_are_labeled_and_read_unambiguously() {
        let hit = serde_json::json!({
            "id": "point-1",
            "score": 0.95,
            "payload": {"id": "external-id", "score": 10}
        });
        let hit = hit.as_object().unwrap();
        let columns = detect_query_columns(std::slice::from_ref(hit));
        let column = |label| columns.iter().find(|column| column.label == label).unwrap();

        assert!(columns.iter().any(|column| column.label == "payload.id"));
        assert!(columns.iter().any(|column| column.label == "payload.score"));
        assert_eq!(query_cell(hit, column("payload.id")).value, "external-id");
        assert_eq!(query_cell(hit, column("payload.score")).value, "10");
    }

    #[test]
    fn compute_alignments_distinguishes_numeric_and_text_columns() {
        let mut table = Table::new(vec!["name".to_string(), "count".to_string()]);
        table.add_cells(vec![
            Cell::from_json(Some(&serde_json::json!("alice"))),
            Cell::from_json(Some(&serde_json::json!(42))),
        ]);
        table.add_cells(vec![
            Cell::from_json(Some(&serde_json::json!("bob"))),
            Cell::from_json(Some(&serde_json::json!(100))),
        ]);
        let alignments = table.compute_alignments();
        assert_eq!(alignments, vec![Alignment::Left, Alignment::Right]);
    }

    #[test]
    fn render_response_dispatches_facet_and_get_points() {
        let get_points_resp = qql::executor::ExecResponse {
            ok: true,
            operation: "GET_POINTS".into(),
            message: "Found 1 hits".into(),
            data: Some(serde_json::json!([
                {"id": 10, "payload": {"tag": "test"}}
            ])),
            telemetry: None,
            typed_hits: std::sync::OnceLock::new(),
        };
        assert!(render_response(&get_points_resp, false).is_ok());

        let facet_resp = qql::executor::ExecResponse {
            ok: true,
            operation: "FACET".into(),
            message: "Found 2 facet hit(s)".into(),
            data: Some(serde_json::json!([
                {"value": "books", "count": 15},
                {"value": "electronics", "count": 8}
            ])),
            telemetry: None,
            typed_hits: std::sync::OnceLock::new(),
        };
        assert!(render_response(&facet_resp, false).is_ok());

        let shard_resp = qql::executor::ExecResponse {
            ok: true,
            operation: "SHOW_SHARD_KEYS".into(),
            message: "Shard keys listed".into(),
            data: Some(serde_json::json!({
                "result": { "shard_keys": ["tenant_1", "tenant_2"] }
            })),
            telemetry: None,
            typed_hits: std::sync::OnceLock::new(),
        };
        assert!(render_response(&shard_resp, false).is_ok());
    }
}
