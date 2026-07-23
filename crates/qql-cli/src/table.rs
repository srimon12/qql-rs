//! ASCII table printer for QQL CLI output.
//!
//! Produces `psql`-style bordered tables. Self-contained — zero dependencies.
//! Detects columns automatically from `ExecResponse.data` payloads.

use std::io::{self, Write};

/// A bordered ASCII table that auto-sizes columns.
pub struct Table {
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Table {
    pub fn new(columns: Vec<String>) -> Self {
        Self {
            columns,
            rows: Vec::new(),
        }
    }

    pub fn add_row(&mut self, row: Vec<String>) {
        self.rows.push(row);
    }

    /// Check whether the table has any data rows.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Print the table to stdout.
    pub fn print(&self) -> io::Result<()> {
        if self.columns.is_empty() {
            return Ok(());
        }

        let widths = self.compute_widths();
        let stdout = io::stdout();
        let mut handle = stdout.lock();

        // Top border
        self.write_border(&mut handle, &widths, '╭', '┬', '╮')?;

        // Header
        self.write_row(&mut handle, &widths, &self.columns)?;

        // Header-data separator
        self.write_border(&mut handle, &widths, '├', '┼', '┤')?;

        // Data rows
        for row in &self.rows {
            self.write_row(&mut handle, &widths, row)?;
        }

        // Bottom border
        self.write_border(&mut handle, &widths, '╰', '┴', '╯')?;

        writeln!(handle)?;
        Ok(())
    }

    fn compute_widths(&self) -> Vec<usize> {
        let n = self.columns.len();
        let mut widths = vec![0usize; n];
        for (i, col) in self.columns.iter().enumerate() {
            widths[i] = col.len();
        }
        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                if i < n && cell.len() > widths[i] {
                    widths[i] = cell.len();
                }
            }
        }
        // Add padding
        for w in &mut widths {
            *w += 2; // one space on each side
        }
        widths
    }

    fn write_border(
        &self,
        w: &mut impl Write,
        widths: &[usize],
        left: char,
        mid: char,
        right: char,
    ) -> io::Result<()> {
        write!(w, "{}", left)?;
        for (i, width) in widths.iter().enumerate() {
            if i > 0 {
                write!(w, "{}", mid)?;
            }
            for _ in 0..*width {
                write!(w, "─")?;
            }
        }
        writeln!(w, "{}", right)?;
        Ok(())
    }

    fn write_row(&self, w: &mut impl Write, widths: &[usize], cells: &[String]) -> io::Result<()> {
        write!(w, "│")?;
        for (i, cell) in cells.iter().enumerate() {
            let width = widths.get(i).copied().unwrap_or(0);
            let cell_width = cell.chars().count();
            let padding = width.saturating_sub(cell_width);
            // Left pad with 1 space, right pad with remaining
            write!(w, " {} ", cell)?;
            if padding > 2 {
                for _ in 0..padding - 2 {
                    write!(w, " ")?;
                }
            }
            write!(w, "│")?;
        }
        writeln!(w)?;
        Ok(())
    }
}

/// Render an `ExecResponse` to stdout.
///
/// For QUERY/SCROLL responses: prints a table of id, score, and payload fields.
/// For SHOW COLLECTIONS: prints a simple list.
/// For SHOW COLLECTION: prints a key-value table.
/// For COUNT: prints the count.
/// For DDL/DML operations: prints only the message.
/// When `json` is true, prints the full JSON response instead.
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
            print_collections_list(&response.data);
        }
        "SHOW_COLLECTION" | "show_collection" => {
            print_collection_info(&response.data);
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
    let mut table = Table::new(columns.iter().map(|s| s.to_string()).collect());

    for hit in &hits {
        let mut row = Vec::new();
        for col in &columns {
            let val = hit.get(col).map(stringify_value).unwrap_or_default();
            row.push(val);
        }
        table.add_row(row);
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
    let count = data
        .as_ref()
        .and_then(|d| d.get("count"))
        .and_then(|c| c.as_u64())
        .unwrap_or(0);
    println!("  count: {}", count);
}

fn print_collections_list(data: &Option<serde_json::Value>) {
    let cols: Vec<&str> = data
        .as_ref()
        .and_then(|d| d.get("collections"))
        .and_then(|c| c.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    if cols.is_empty() {
        println!("(no collections)");
        return;
    }

    let mut table = Table::new(vec!["Collection".into()]);
    for name in cols {
        table.add_row(vec![name.to_string()]);
    }
    let _ = table.print();
}

fn print_collection_info(data: &Option<serde_json::Value>) {
    let Some(obj) = data.as_ref().and_then(|d| d.as_object()) else {
        if let Some(d) = data {
            println!("{}", serde_json::to_string_pretty(d).unwrap_or_default());
        }
        return;
    };

    let mut table = Table::new(vec!["Property".into(), "Value".into()]);
    for (key, val) in obj {
        table.add_row(vec![key.clone(), stringify_value(val)]);
    }
    let _ = table.print();
}

// ── helpers ──────────────────────────────────────────────────────

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

fn detect_query_columns(hits: &[serde_json::Map<String, serde_json::Value>]) -> Vec<String> {
    let mut cols = vec!["id".to_string(), "score".to_string()];

    // Collect payload keys from the first few hits
    let mut payload_keys = std::collections::BTreeSet::new();
    for hit in hits.iter().take(5) {
        if let Some(payload) = hit.get("payload").and_then(|p| p.as_object()) {
            for key in payload.keys() {
                // Skip deeply nested objects — show only simple values
                if let Some(val) = payload.get(key) {
                    if val.is_string() || val.is_number() || val.is_boolean() {
                        payload_keys.insert(key.clone());
                    }
                }
            }
        }
    }

    cols.extend(payload_keys);
    cols
}

fn stringify_value(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => String::new(),
        _ => {
            // Truncate complex objects
            let s = serde_json::to_string(val).unwrap_or_default();
            if s.len() > 60 {
                format!("{}…", &s[..59])
            } else {
                s
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_renders_borders() {
        let mut table = Table::new(vec!["id".into(), "score".into()]);
        table.add_row(vec!["42".into(), "0.95".into()]);
        table.add_row(vec!["7".into(), "0.80".into()]);

        // Table prints to real stdout — verify it doesn't panic
        let result = std::panic::catch_unwind(|| {
            table.print().unwrap();
        });
        assert!(result.is_ok());
    }

    #[test]
    fn empty_table_no_panic() {
        let table = Table::new(vec!["id".into()]);
        assert!(table.is_empty());
        // print of empty table just prints borders with no rows
        let result = std::panic::catch_unwind(|| {
            table.print().unwrap();
        });
        assert!(result.is_ok());
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
        assert!(cols.contains(&"id".to_string()));
        assert!(cols.contains(&"score".to_string()));
        assert!(cols.contains(&"title".to_string()));
        assert!(cols.contains(&"year".to_string()));
        // nested is skipped (it's an object, not simple)
        assert!(!cols.contains(&"nested".to_string()));
    }
}
