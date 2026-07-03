fn escape_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
        .replace('\0', "\\0")
}

/// Generate a CREATE COLLECTION statement.
pub fn generate_create_statement(
    collection: &str,
    _hybrid: bool,
    _dense_name: &str,
    _sparse_name: &str,
    _dense_model: &str,
    _sparse_model: &str,
) -> String {
    let mut stmt = format!("CREATE COLLECTION {}", collection);
    if _hybrid {
        if !_dense_model.is_empty() {
            stmt.push_str(&format!(
                " HYBRID DENSE MODEL '{}'",
                escape_string(_dense_model)
            ));
            if !_sparse_model.is_empty() {
                stmt.push_str(&format!(" SPARSE MODEL '{}'", escape_string(_sparse_model)));
            }
        } else {
            stmt.push_str(" HYBRID");
            if _dense_name != "dense" || _sparse_name != "sparse" {
                stmt.push_str(&format!(
                    " DENSE VECTOR '{}' SPARSE VECTOR '{}'",
                    escape_string(_dense_name),
                    escape_string(_sparse_name)
                ));
            }
        }
    } else if !_dense_model.is_empty() {
        stmt.push_str(&format!(" USING MODEL '{}'", escape_string(_dense_model)));
    } else if _dense_name != "dense" && !_dense_name.is_empty() {
        stmt.push_str(&format!(" VECTOR '{}'", escape_string(_dense_name)));
    }
    stmt
}
