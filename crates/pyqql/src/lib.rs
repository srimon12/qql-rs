use pyo3::exceptions::PySyntaxError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use qql_core::lexer::Lexer;
use qql_core::parser::Parser;


#[pyfunction]
fn parse(input: &str) -> PyResult<String> {
    match Parser::parse(input) {
        Ok(stmt) => Ok(format!("{:#?}", stmt)),
        Err(e) => Err(PySyntaxError::new_err(e.to_string())),
    }
}

#[pyfunction]
fn tokenize<'py>(input: &str, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDict>>> {
    let lexer = Lexer::new(input);
    let mut result = Vec::new();
    for token_result in lexer {
        let token = token_result.map_err(|e| PySyntaxError::new_err(e.to_string()))?;
        let d = PyDict::new(py);
        d.set_item("kind", token.kind.as_str())?;
        d.set_item("text", token.text)?;
        d.set_item("pos", token.pos as i64)?;
        result.push(d);
    }
    Ok(result)
}

#[pymodule]
fn pyqql(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(parse, m)?)?;
    m.add_function(wrap_pyfunction!(tokenize, m)?)?;
    Ok(())
}
