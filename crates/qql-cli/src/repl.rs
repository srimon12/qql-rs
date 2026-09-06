//! Interactive QQL REPL (Read-Eval-Print Loop).
//!
//! Provides interactive query execution, multiline statement accumulation,
//! in-REPL formatting, query explanation, health diagnostics, and execution timing.

use std::time::Instant;

pub async fn run_repl(
    url: &str,
    use_edge: bool,
    executor: qql::executor::Executor,
) -> Result<(), Box<dyn std::error::Error>> {
    crate::output::print_banner();
    let target = if use_edge { "local edge" } else { url };
    crate::output::print_success(&format!("Connected to \x1b[36m{}\x1b[0m", target));
    println!(
        "Type \x1b[1mhelp\x1b[0m for available commands, \x1b[1m\\f\x1b[0m to format, or \x1b[1mexit\x1b[0m to quit.\n"
    );

    let mut rl = rustyline::DefaultEditor::new()?;
    let mut buffer = String::new();

    loop {
        let prompt = if buffer.is_empty() {
            "\x1b[32m\x1b[1mqql>\x1b[0m "
        } else {
            "\x1b[32m\x1b[1mqql...>\x1b[0m "
        };

        let line = match rl.readline(prompt) {
            Ok(l) => l,
            Err(_) => {
                println!("\nBye.");
                break;
            }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !buffer.is_empty() {
                buffer.clear();
                println!("\x1b[2m(statement aborted)\x1b[0m");
            }
            continue;
        }

        // Meta-commands apply only when starting a new statement (buffer empty)
        if buffer.is_empty() {
            let lower = trimmed.to_lowercase();

            if lower == "exit" || lower == "quit" || lower == "\\q" || lower == ":q" {
                println!("Bye.");
                break;
            }

            if lower == "help" || lower == "\\h" || lower == "?" {
                print_repl_help();
                let _ = rl.add_history_entry(trimmed);
                continue;
            }

            if lower == "doctor" || lower == "\\d" {
                let _ = rl.add_history_entry(trimmed);
                let _ = crate::commands::handle_doctor(url, use_edge, false, false).await;
                continue;
            }

            if let Some(args) =
                cut_command_prefix(trimmed, "fmt").or_else(|| cut_command_prefix(trimmed, "\\f"))
            {
                let _ = rl.add_history_entry(trimmed);
                match qql_core::fmt::format(&args) {
                    Ok(formatted) => {
                        println!("\x1b[1mFormatted QQL:\x1b[0m\n{}", formatted);
                    }
                    Err(e) => crate::output::print_error(&format!("format error: {}", e)),
                }
                continue;
            }

            if let Some(args) = cut_command_prefix(trimmed, "explain") {
                let _ = rl.add_history_entry(trimmed);
                match crate::commands::explain_query_str(&args) {
                    Ok(plan) => {
                        println!("\x1b[1mQuery Plan:\x1b[0m\n{}", plan);
                    }
                    Err(e) => crate::output::print_error(&format!("explain error: {}", e)),
                }
                continue;
            }

            if let Some(args) = cut_command_prefix(trimmed, "execute")
                .or_else(|| cut_command_prefix(trimmed, "\\e"))
            {
                let _ = rl.add_history_entry(trimmed);
                match crate::script::read_script(&args) {
                    Ok(statements) => {
                        let start = Instant::now();
                        let mut ok_count = 0;
                        let mut fail_count = 0;
                        for (idx, stmt) in statements.iter().enumerate() {
                            match executor
                                .execute(stmt, qql::executor::OnError::Continue)
                                .await
                            {
                                Ok(report) => {
                                    ok_count += report.succeeded;
                                    fail_count += report.failed;
                                    for r in report.results.iter().filter(|r| !r.ok) {
                                        crate::output::print_error(&format!(
                                            "statement {} ({}): {}",
                                            idx + 1,
                                            r.operation,
                                            r.message
                                        ));
                                    }
                                }
                                Err(e) => {
                                    fail_count += 1;
                                    crate::output::print_error(&format!(
                                        "statement {}: {}",
                                        idx + 1,
                                        e
                                    ));
                                }
                            }
                        }
                        let elapsed = start.elapsed();
                        crate::output::print_success(&format!(
                            "Executed script '{}' ({} succeeded, {} failed in {:.2?})",
                            args, ok_count, fail_count, elapsed
                        ));
                    }
                    Err(e) => crate::output::print_error(&format!(
                        "cannot read script file '{}': {}",
                        args, e
                    )),
                }
                continue;
            }

            if let Some(args) = cut_command_prefix(trimmed, "dump") {
                let _ = rl.add_history_entry(trimmed);
                let parts: Vec<&str> = args.split_whitespace().collect();
                let dump_parts = if parts.len() >= 3 && parts[0].eq_ignore_ascii_case("collection")
                {
                    &parts[1..]
                } else {
                    &parts
                };
                if dump_parts.len() != 2 {
                    crate::output::print_error(
                        "dump error: usage DUMP [COLLECTION] <name> <output.qql>",
                    );
                    continue;
                }
                match crate::dump::dump_collection(
                    &executor,
                    dump_parts[0],
                    dump_parts[1],
                    50,
                    None,
                )
                .await
                {
                    Ok(stats) => crate::output::print_success(&format!(
                        "Dumped collection '{}' to {} ({} written, {} skipped, {} batches)",
                        dump_parts[0], dump_parts[1], stats.written, stats.skipped, stats.batches
                    )),
                    Err(e) => crate::output::print_error(&format!("dump error: {}", e)),
                }
                continue;
            }
        }

        // Multiline accumulation
        if !buffer.is_empty() {
            buffer.push('\n');
        }
        buffer.push_str(trimmed);

        if !is_statement_complete(&buffer) {
            continue;
        }

        let full_query = core::mem::take(&mut buffer);
        let _ = rl.add_history_entry(&full_query);

        let start = Instant::now();
        match executor
            .execute(&full_query, qql::executor::OnError::Stop)
            .await
        {
            Ok(report) => {
                let elapsed = start.elapsed();
                if let Err(e) = crate::table::render_report(&report, false) {
                    crate::output::print_error(&format!("display error: {}", e));
                } else {
                    println!("\x1b[2m(completed in {:.2?})\x1b[0m\n", elapsed);
                }
            }
            Err(e) => crate::output::print_error(&format!("execution error: {}", e)),
        }
    }

    executor.close().await?;
    Ok(())
}

fn is_statement_complete(query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Quick path: standalone single-line admin commands
    let upper = trimmed.to_uppercase();
    if upper == "SHOW COLLECTIONS"
        || upper == "SHOW QUOTAS"
        || upper.starts_with("SHOW SHARD KEYS")
        || upper.starts_with("SHOW COLLECTION ")
    {
        return true;
    }

    // Use the official lexer to tokenize input, accurately handling comments,
    // backtick strings, and escape sequences without hand-rolled state machines.
    let mut lexer = qql_core::lexer::Lexer::new(trimmed);
    let mut depth = 0;
    let mut has_semicolon = false;

    loop {
        match lexer.next_token() {
            Ok(token) => match token.kind {
                qql_core::token::TokenKind::Eof => break,
                qql_core::token::TokenKind::Lparen
                | qql_core::token::TokenKind::Lbracket
                | qql_core::token::TokenKind::Lbrace => depth += 1,
                qql_core::token::TokenKind::Rparen
                | qql_core::token::TokenKind::Rbracket
                | qql_core::token::TokenKind::Rbrace
                    if depth > 0 =>
                {
                    depth -= 1;
                }
                qql_core::token::TokenKind::Semicolon => {
                    has_semicolon = true;
                }
                _ => {}
            },
            Err(e) => {
                // If lexing encountered an unterminated string or incomplete token,
                // the statement is in progress across lines.
                let msg = e.to_string();
                if msg.contains("unterminated") {
                    return false;
                }
                break;
            }
        }
    }

    if depth > 0 {
        return false;
    }

    if has_semicolon || trimmed.ends_with(';') {
        return true;
    }

    // If all delimiters are closed and the query already parses cleanly as a valid
    // statement, execute it immediately without requiring a semicolon on a separate line.
    qql_core::parser::Parser::parse(trimmed).is_ok()
}

fn cut_command_prefix(input: &str, prefix: &str) -> Option<String> {
    let input_trimmed = input.trim();
    let lower = input_trimmed.to_lowercase();

    if lower.len() <= prefix.len() || !lower.starts_with(prefix) {
        return None;
    }

    let after = &input_trimmed[prefix.len()..];
    if after.starts_with(' ') {
        Some(after.trim().to_string())
    } else {
        None
    }
}

fn print_repl_help() {
    let help = "\x1b[1mAvailable Statements:\x1b[0m\n\
\n  \x1b[33mUPSERT INTO\x1b[0m <name> \x1b[33mVALUES\x1b[0m {id: 1, text: '...', ...}\n\
\n  \x1b[33mCREATE COLLECTION\x1b[0m <name> [\x1b[33mHYBRID\x1b[0m [\x1b[33mRERANK\x1b[0m]]\n\
\n  \x1b[33mDROP COLLECTION\x1b[0m <name>\n\
\n  \x1b[33mSHOW COLLECTIONS\x1b[0m\n\
\n  \x1b[33mQUERY\x1b[0m ['<text>' | [<vector>] | NEAREST POINT <id> | ...]\n\
      \x1b[33mFROM\x1b[0m <collection> [\x1b[33mUSING\x1b[0m <vector> [\x1b[33mAS DENSE|SPARSE\x1b[0m]] \x1b[33mLIMIT\x1b[0m <n>\n\
\n  \x1b[33mQUERY POINTS\x1b[0m (<id>, ...) \x1b[33mFROM\x1b[0m <name> [\x1b[33mWITH PAYLOAD false\x1b[0m]\n\
\n  \x1b[33mFACET\x1b[0m <field> \x1b[33mFROM\x1b[0m <name> [\x1b[33mWHERE\x1b[0m <filter>] [\x1b[33mLIMIT\x1b[0m <n>] [\x1b[33mEXACT true\x1b[0m]\n\
\n  \x1b[33mSCROLL FROM\x1b[0m <name> [\x1b[33mWHERE\x1b[0m <filter>] [\x1b[33mAFTER\x1b[0m '<id>'] [\x1b[33mWITH VECTOR\x1b[0m] \x1b[33mLIMIT\x1b[0m <n>\n\
\n  \x1b[33mDELETE FROM\x1b[0m <name> \x1b[33mWHERE\x1b[0m id = '<id>' | <field> = '<value>'\n\
\n\x1b[1mBuilt-in Commands:\x1b[0m\n\
\n  \x1b[36mhelp\x1b[0m, \x1b[36m\\h\x1b[0m, \x1b[36m?\x1b[0m       Show this help card (note: bare ? triggers help)\n\
  \x1b[36mdoctor\x1b[0m, \x1b[36m\\d\x1b[0m         Check connection health and loaded model hosts\n\
  \x1b[36mfmt <qql>\x1b[0m, \x1b[36m\\f\x1b[0m      Format QQL into canonical syntax\n\
  \x1b[36mexplain <qql>\x1b[0m     Show hierarchical tree query execution plan\n\
  \x1b[36mexecute <file>\x1b[0m, \x1b[36m\\e\x1b[0m Execute a .qql script file against Qdrant\n\
  \x1b[36mdump <name> <file>\x1b[0m Dump collection schema and points to .qql\n\
  \x1b[36mexit\x1b[0m, \x1b[36mquit\x1b[0m, \x1b[36m\\q\x1b[0m    Exit the shell\n\
\n\x1b[1mKeyboard Shortcuts:\x1b[0m\n\
\n  Ctrl-C         Cancel current input / abort multiline buffer\n\
  Ctrl-D         Exit shell\n";
    println!("{}", help);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_statement_complete_with_comments() {
        assert!(is_statement_complete(
            "QUERY TEXT 'x' FROM docs -- it's a comment\n;"
        ));
        assert!(is_statement_complete(
            "QUERY TEXT 'x' FROM docs -- (see docs\n;"
        ));
    }

    #[test]
    fn test_is_statement_complete_with_quotes() {
        assert!(is_statement_complete(
            "UPSERT INTO t VALUES {id: 1, text: 'it\\'s ok'};"
        ));
        assert!(!is_statement_complete("QUERY TEXT 'unterminated"));
    }

    #[test]
    fn test_is_statement_complete_single_line_queries() {
        assert!(is_statement_complete(
            "QUERY TEXT 'hello' FROM docs LIMIT 5;"
        ));
        assert!(is_statement_complete("SHOW COLLECTIONS"));
    }
}
