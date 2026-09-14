//! `qql convert` and `qql fmt`.

pub fn handle_convert(
    path: Option<&str>,
    collection: Option<&str>,
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

    let statements = qql_convert::convert(&input, collection)?;

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
