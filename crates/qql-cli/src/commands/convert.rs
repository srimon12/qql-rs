//! `qql convert` and `qql fmt`.

/// Maximum curl `@file` body size read by the CLI (64 MiB).
const MAX_BODY_BYTES: u64 = 64 * 1024 * 1024;

/// Read a curl `@file` body, rejecting directories, pipes, devices, and
/// oversized files. Paths resolve against the process working directory.
fn read_body(body_path: &str) -> Result<String, String> {
    let meta = std::fs::metadata(body_path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a regular file".to_string());
    }
    if meta.len() > MAX_BODY_BYTES {
        return Err(format!(
            "file is too large ({} bytes, limit {MAX_BODY_BYTES})",
            meta.len()
        ));
    }
    std::fs::read_to_string(body_path).map_err(|e| e.to_string())
}

/// Parse repeatable `--var KEY=VALUE` flags. A later value for the same key
/// replaces an earlier one.
pub(crate) fn parse_convert_vars(
    raw: &[String],
) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let mut vars: Vec<(String, String)> = Vec::new();
    for item in raw {
        let Some((key, value)) = item.split_once('=') else {
            return Err(format!("--var expects KEY=VALUE, got `{item}`").into());
        };
        if key.is_empty() {
            return Err(format!("--var expects a non-empty key, got `{item}`").into());
        }
        match vars.iter_mut().find(|(name, _)| name == key) {
            Some((_, slot)) => *slot = value.to_string(),
            None => vars.push((key.to_string(), value.to_string())),
        }
    }
    Ok(vars)
}

pub fn handle_convert(
    path: Option<&str>,
    collection: Option<&str>,
    vars: &[(String, String)],
) -> Result<(), Box<dyn std::error::Error>> {
    let input = if let Some(p) = path {
        std::fs::read_to_string(p).map_err(|e| format!("cannot read file: {}", e))?
    } else {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)
            .map_err(|e| format!("cannot read stdin: {}", e))?;
        buf
    };

    let input = input.trim().to_string();
    if input.is_empty() {
        return Err("no input provided".into());
    }

    let read_file = read_body;
    let options = qql_convert::ConvertOptions::new()
        .with_vars(vars)
        .with_read_file(&read_file);
    let statements = qql_convert::convert_with(&input, collection, &options)?;

    for stmt in &statements {
        // `format_stmt` renders no terminator; the CLI prints canonical QQL,
        // so each statement gets its `;` back (and `qql fmt --check` passes).
        println!("{};", stmt);
    }

    Ok(())
}

/// Whether `source` is already canonical.
///
/// `format()` always terminates with a newline; a file without one is still
/// canonical, so only trailing whitespace is ignored on both sides.
pub(crate) fn source_is_canonical(source: &str, formatted: &str) -> bool {
    source.trim_end() == formatted.trim_end()
}

/// Format QQL source into canonical form.
///
/// Reads from `path` (or stdin when `None`). In `check` mode the formatted
/// output is compared against the input and a non-zero exit indicates the
/// source is not formatted. With `write` the formatted output is written back
/// to the file; otherwise it is printed to stdout.
pub fn handle_fmt(
    path: Option<&str>,
    check: bool,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = if let Some(p) = path {
        std::fs::read_to_string(p).map_err(|e| format!("cannot read file '{}': {}", p, e))?
    } else {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)
            .map_err(|e| format!("cannot read stdin: {}", e))?;
        buf
    };

    let formatted = qql_core::fmt::format(&input)?;

    if check {
        if !source_is_canonical(&input, &formatted) {
            let target = path.unwrap_or("<stdin>");
            return Err(format!("{} is not formatted (run `qql fmt` to fix)", target).into());
        }
        return Ok(());
    }

    if write && let Some(p) = path {
        std::fs::write(p, &formatted).map_err(|e| format!("cannot write '{}': {}", p, e))?;
        return Ok(());
    }

    print!("{}", formatted);
    Ok(())
}
