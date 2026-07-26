//! `psql`-style table printer for QQL CLI output.
//!
//! Produces unbordered, aligned tables with a row-count footer.
//! Detects columns automatically from `ExecResponse.data` payloads.

use std::io::{self, Write};
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Alignment {
    Left,
    Right,
}

#[derive(Debug)]
enum QueryColumnSource {
    Metadata(&'static str),
    Payload(String),
}

#[derive(Debug)]
struct QueryColumn {
    label: String,
    source: QueryColumnSource,
}

#[derive(Debug)]
struct Cell {
    value: String,
    alignment: Alignment,
}

impl Cell {
    fn text(value: impl Into<String>) -> Self {
        Self {
            value: escape_controls(&value.into()),
            alignment: Alignment::Left,
        }
    }

    fn from_json(value: Option<&serde_json::Value>) -> Self {
        let alignment = match value {
            Some(serde_json::Value::Number(_)) => Alignment::Right,
            _ => Alignment::Left,
        };

        let mut cell = Self::text(value.map(stringify_value).unwrap_or_default());
        cell.alignment = alignment;
        cell
    }
}

/// A `psql`-style table that auto-sizes columns.
pub struct Table {
    columns: Vec<String>,
    rows: Vec<Vec<Cell>>,
}

impl Table {
    pub fn new(columns: Vec<String>) -> Self {
        Self {
            columns,
            rows: Vec::new(),
        }
    }

    pub fn add_row(&mut self, row: Vec<String>) {
        self.add_cells(row.into_iter().map(Cell::text).collect());
    }

    fn add_cells(&mut self, mut row: Vec<Cell>) {
        row.truncate(self.columns.len());
        row.resize_with(self.columns.len(), || Cell::text(""));
        self.rows.push(row);
    }

    /// Check whether the table has any data rows.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Print the table to stdout.
    pub fn print(&self) -> io::Result<()> {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        self.render(&mut handle)
    }

    fn render(&self, w: &mut impl Write) -> io::Result<()> {
        if self.columns.is_empty() {
            return Ok(());
        }

        let widths = self.compute_widths();
        let alignments = self.compute_alignments();

        self.write_header(w, &widths)?;
        self.write_separator(w, &widths)?;

        for row in &self.rows {
            self.write_row(w, &widths, &alignments, row)?;
        }

        let label = if self.rows.len() == 1 { "row" } else { "rows" };
        writeln!(w, "({} {label})", self.rows.len())?;
        Ok(())
    }

    fn compute_widths(&self) -> Vec<usize> {
        let n = self.columns.len();
        let mut widths = vec![0usize; n];
        for (i, col) in self.columns.iter().enumerate() {
            widths[i] = display_width(col);
        }
        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                let width = display_width(&cell.value);
                if i < n && width > widths[i] {
                    widths[i] = width;
                }
            }
        }
        widths
    }

    fn compute_alignments(&self) -> Vec<Alignment> {
        self.columns
            .iter()
            .enumerate()
            .map(|(column_index, _)| {
                let has_value = self
                    .rows
                    .iter()
                    .map(|row| &row[column_index])
                    .any(|cell| !cell.value.is_empty());
                let all_numbers = self
                    .rows
                    .iter()
                    .map(|row| &row[column_index])
                    .filter(|cell| !cell.value.is_empty())
                    .all(|cell| cell.alignment == Alignment::Right);
                if has_value && all_numbers {
                    Alignment::Right
                } else {
                    Alignment::Left
                }
            })
            .collect()
    }

    fn write_header(&self, w: &mut impl Write, widths: &[usize]) -> io::Result<()> {
        for (i, (column, width)) in self.columns.iter().zip(widths).enumerate() {
            if i > 0 {
                write!(w, "|")?;
            }
            write!(w, " {} ", center(column, *width))?;
        }
        writeln!(w)
    }

    fn write_separator(&self, w: &mut impl Write, widths: &[usize]) -> io::Result<()> {
        for (i, width) in widths.iter().enumerate() {
            if i > 0 {
                write!(w, "+")?;
            }
            for _ in 0..(*width + 2) {
                write!(w, "-")?;
            }
        }
        writeln!(w)
    }

    fn write_row(
        &self,
        w: &mut impl Write,
        widths: &[usize],
        alignments: &[Alignment],
        cells: &[Cell],
    ) -> io::Result<()> {
        for (i, ((cell, width), alignment)) in cells.iter().zip(widths).zip(alignments).enumerate()
        {
            if i > 0 {
                write!(w, "|")?;
            }
            let padding = width.saturating_sub(display_width(&cell.value));
            match alignment {
                Alignment::Left => write!(w, " {}{} ", cell.value, " ".repeat(padding))?,
                Alignment::Right => write!(w, " {}{} ", " ".repeat(padding), cell.value)?,
            }
        }
        writeln!(w)
    }
}

/// Returns the terminal-cell width according to Unicode Standard Annex #11.
fn display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

/// Escapes control characters so a value cannot break table structure or emit
/// terminal control sequences. Printable Unicode remains unchanged.
fn escape_controls(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_control() {
            escaped.extend(character.escape_default());
        } else {
            escaped.push(character);
        }
    }
    escaped
}

fn center(value: &str, width: usize) -> String {
    let padding = width.saturating_sub(display_width(value));
    let left = padding / 2;
    format!(
        "{}{}{}",
        " ".repeat(left),
        value,
        " ".repeat(padding - left)
    )
}

/// Render an [`ExecutionReport`] to stdout.
///
/// For QUERY/SCROLL responses: prints a table of id, score, and payload fields.
/// For SHOW COLLECTIONS: prints a simple list.
/// For SHOW COLLECTION: prints a key-value table.
/// For COUNT: prints the count.
/// For DDL/DML operations: prints only the message.
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
fn render_response(
    response: &qql::executor::ExecResponse,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if json {
        let s = serde_json::to_string_pretty(response)?;
        println!("{}", s);
        return Ok(());
    }

    match response.operation.as_str() {
        "QUERY" | "SCROLL" => {
            print_query_table(&response.data)?;
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

fn print_query_table(data: &Option<serde_json::Value>) -> Result<(), Box<dyn std::error::Error>> {
    let hits = extract_hits(data);
    if hits.is_empty() {
        println!("(no results)");
        return Ok(());
    }

    let columns = detect_query_columns(&hits);
    let mut table = Table::new(columns.iter().map(|column| column.label.clone()).collect());

    for hit in &hits {
        let mut row = Vec::with_capacity(columns.len());
        for col in &columns {
            row.push(query_cell(hit, col));
        }
        table.add_cells(row);
    }

    table.print()?;
    Ok(())
}

fn print_groups_table(data: &Option<serde_json::Value>) -> Result<(), Box<dyn std::error::Error>> {
    let groups = data
        .as_ref()
        .and_then(|d| d.get("result"))
        .and_then(|r| r.get("groups"))
        .and_then(|g| g.as_array())
        .or_else(|| {
            data.as_ref()
                .and_then(|d| d.get("groups"))
                .and_then(|g| g.as_array())
        });

    let Some(groups) = groups else {
        println!("{}", serde_json::to_string_pretty(data)?);
        return Ok(());
    };

    if groups.is_empty() {
        println!("(no groups)");
        return Ok(());
    }

    let mut table = Table::new(vec!["group_id".into(), "count".into()]);

    for g in groups {
        let id = stringify_value(&g.get("id").cloned().unwrap_or_default());
        let hits = g
            .get("hits")
            .and_then(|h| h.as_array())
            .map(|a| a.len().to_string())
            .unwrap_or_else(|| "0".into());
        table.add_row(vec![id, hits]);
    }

    table.print()?;
    Ok(())
}

fn print_count(data: &Option<serde_json::Value>) {
    println!("  count: {}", count_value(data));
}

fn print_collections_list(
    data: &Option<serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    let cols = collection_names(data);

    if cols.is_empty() {
        println!("(no collections)");
        return Ok(());
    }

    let mut table = Table::new(vec!["Collection".into()]);
    for name in cols {
        table.add_row(vec![name]);
    }
    table.print()?;
    Ok(())
}

fn print_collection_info(
    data: &Option<serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(obj) = result_value(data).and_then(serde_json::Value::as_object) else {
        if let Some(d) = data {
            println!("{}", serde_json::to_string_pretty(d)?);
        }
        return Ok(());
    };

    let mut table = Table::new(vec!["Property".into(), "Value".into()]);
    for (key, val) in obj {
        table.add_row(vec![key.clone(), stringify_value(val)]);
    }
    table.print()?;
    Ok(())
}

// ── helpers ──────────────────────────────────────────────────────

/// Returns Qdrant's `result` object when present, otherwise the response.
fn result_value(data: &Option<serde_json::Value>) -> Option<&serde_json::Value> {
    data.as_ref()
        .map(|value| value.get("result").unwrap_or(value))
}

fn count_value(data: &Option<serde_json::Value>) -> u64 {
    result_value(data)
        .and_then(|value| value.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
}

fn collection_names(data: &Option<serde_json::Value>) -> Vec<String> {
    result_value(data)
        .and_then(|value| value.get("collections"))
        .and_then(serde_json::Value::as_array)
        .map(|collections| {
            collections
                .iter()
                .filter_map(|collection| {
                    collection
                        .as_str()
                        .or_else(|| collection.get("name").and_then(serde_json::Value::as_str))
                })
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn extract_hits(
    data: &Option<serde_json::Value>,
) -> Vec<serde_json::Map<String, serde_json::Value>> {
    let Some(data) = data else { return Vec::new() };
    // QUERY returns Vec<SearchHit>
    if let Some(arr) = data.as_array() {
        return arr
            .iter()
            .filter_map(|v| v.as_object().cloned())
            .map(|m| m.into_iter().collect())
            .collect();
    }
    // SCROLL returns {"result": {"points": [...]}}
    data.get("result")
        .and_then(|r| r.get("points"))
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_object().cloned())
                .map(|m| m.into_iter().collect())
                .collect()
        })
        .unwrap_or_default()
}

fn detect_query_columns(hits: &[serde_json::Map<String, serde_json::Value>]) -> Vec<QueryColumn> {
    let mut cols = vec![QueryColumn {
        label: "id".into(),
        source: QueryColumnSource::Metadata("id"),
    }];
    if hits.iter().any(|hit| hit.contains_key("score")) {
        cols.push(QueryColumn {
            label: "score".into(),
            source: QueryColumnSource::Metadata("score"),
        });
    }

    // Payload keys form the remaining logical columns. Inspect every result:
    // sampling silently loses fields that occur only in later hits.
    let mut payload_keys = std::collections::BTreeSet::new();
    for hit in hits {
        if let Some(payload) = hit.get("payload").and_then(|p| p.as_object()) {
            for key in payload.keys() {
                payload_keys.insert(key.clone());
            }
        }
    }

    for key in payload_keys {
        // `id` and `score` identify point metadata. Prefix colliding payload
        // keys so the table remains unambiguous.
        let mut label = if matches!(key.as_str(), "id" | "score") {
            format!("payload.{key}")
        } else {
            key.clone()
        };
        while cols.iter().any(|column| column.label == label) {
            label = format!("payload.{label}");
        }
        cols.push(QueryColumn {
            label,
            source: QueryColumnSource::Payload(key),
        });
    }
    cols
}

fn query_cell(hit: &serde_json::Map<String, serde_json::Value>, column: &QueryColumn) -> Cell {
    let value = match &column.source {
        QueryColumnSource::Metadata(key) => hit.get(*key),
        QueryColumnSource::Payload(key) => hit
            .get("payload")
            .and_then(serde_json::Value::as_object)
            .and_then(|payload| payload.get(key)),
    };
    Cell::from_json(value)
}

fn stringify_value(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => String::new(),
        _ => serde_json::to_string(val).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
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
        // JSON values are retained instead of being silently omitted.
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
}
