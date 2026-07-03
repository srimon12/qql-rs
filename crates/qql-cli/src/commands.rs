use crate::convert;
use crate::dump;
use crate::output;
use crate::script;
use qql_core::ast::Stmt;
use qql_core::parser::Parser;

const VERSION: &str = "0.1.0";

// ── Public handlers ───────────────────────────────────────────

pub fn handle_exec(query: &str, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let plan = explain_query(query)?;
    if json {
        let resp = output::ExecResponse {
            ok: true,
            operation: "exec".to_string(),
            message: plan.clone(),
        };
        let s = serde_json::to_string_pretty(&resp)?;
        println!("{}", s);
    } else {
        println!("{}", plan);
    }
    Ok(())
}

pub fn handle_execute_file(
    path: &str,
    stop_on_error: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let statements = script::read_script(path).map_err(|e| format!("{}", e))?;

    let (ok_count, fail_count) = script::execute_script(statements, stop_on_error, |stmt| {
        let plan = explain_query(stmt)?;
        Ok(plan)
    })?;

    let msg = format!(
        "Executed script {} ({} succeeded, {} failed)",
        path, ok_count, fail_count
    );

    let resp = output::ScriptResponse {
        ok: fail_count == 0,
        command: "execute".to_string(),
        path: path.to_string(),
        succeeded: ok_count,
        failed: fail_count,
        message: msg.clone(),
    };
    let s = serde_json::to_string_pretty(&resp)?;
    println!("{}", s);
    Ok(())
}

pub fn handle_explain(query: &str) -> Result<(), Box<dyn std::error::Error>> {
    let plan = explain_query(query)?;

    let resp = output::ExplainResponse {
        ok: true,
        query: query.to_string(),
        plan: plan.clone(),
    };
    let s = serde_json::to_string_pretty(&resp)?;
    println!("{}", s);
    Ok(())
}

pub fn handle_connect(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    output::print_banner();
    output::print_success(&format!("Connected to \x1b[36m{}\x1b[0m", url));
    println!("Type \x1b[1mhelp\x1b[0m for available commands or \x1b[1mexit\x1b[0m to quit.\n");

    let mut rl = rustyline::DefaultEditor::new()?;

    loop {
        let prompt = "\x1b[32m\x1b[1mqql>\x1b[0m ";
        let line = match rl.readline(prompt) {
            Ok(l) => l,
            Err(_) => {
                println!("\nBye.");
                break;
            }
        };

        let trimmed = line.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }

        let _ = rl.add_history_entry(&trimmed);

        let lower = trimmed.to_lowercase();

        if lower == "exit" || lower == "quit" || lower == "\\q" || lower == ":q" {
            println!("Bye.");
            break;
        }

        if lower == "help" || lower == "\\h" || lower == "?" {
            print_repl_help();
            continue;
        }

        if let Some(args) = cut_command_prefix(&trimmed, "explain") {
            match explain_query(&args) {
                Ok(plan) => {
                    println!("\x1b[1mQuery Plan\x1b[0m");
                    println!("{}", plan);
                }
                Err(e) => output::print_error(&format!("explain error: {}", e)),
            }
            continue;
        }

        if let Some(args) = cut_command_prefix(&trimmed, "execute") {
            match script::read_script(&args) {
                Ok(stmts) => {
                    let (ok, fail) = script::execute_script(stmts, false, |stmt| {
                        explain_query(stmt).map_err(|e| e.to_string())
                    })?;
                    output::print_success(&format!(
                        "Executed script {} ({} succeeded, {} failed)",
                        args, ok, fail
                    ));
                }
                Err(e) => output::print_error(&format!("execute error: {}", e)),
            }
            continue;
        }

        if let Some(args) = cut_command_prefix(&trimmed, "\\e") {
            match script::read_script(&args) {
                Ok(stmts) => {
                    let (ok, fail) = script::execute_script(stmts, false, |stmt| {
                        explain_query(stmt).map_err(|e| e.to_string())
                    })?;
                    output::print_success(&format!(
                        "Executed script {} ({} succeeded, {} failed)",
                        args, ok, fail
                    ));
                }
                Err(e) => output::print_error(&format!("execute error: {}", e)),
            }
            continue;
        }

        if let Some(args) = cut_command_prefix(&trimmed, "dump") {
            let parts: Vec<&str> = args.split_whitespace().collect();
            let dump_parts = if parts.len() >= 3 && parts[0].eq_ignore_ascii_case("collection") {
                &parts[1..]
            } else {
                &parts
            };
            if dump_parts.len() != 2 {
                output::print_error("dump error: usage DUMP [COLLECTION] <name> <output.qql>");
                continue;
            }
            match handle_dump(dump_parts[0], dump_parts[1], 50) {
                Ok(msg) => output::print_success(&msg),
                Err(e) => output::print_error(&format!("dump error: {}", e)),
            }
            continue;
        }

        match explain_query(&trimmed) {
            Ok(plan) => {
                output::print_success(&plan);
            }
            Err(e) => output::print_error(&format!("execution error: {}", e)),
        }
    }

    Ok(())
}

pub fn handle_convert(path: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
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

    let statements = convert::json_to_qql(&input)?;

    for stmt in &statements {
        println!("{}", stmt);
    }

    Ok(())
}

pub fn handle_dump(
    collection: &str,
    output: &str,
    _batch_size: u32,
) -> Result<String, Box<dyn std::error::Error>> {
    // Mock dump: generate a header and CREATE COLLECTION statement
    let header = format!("-- QQL dump for {}", collection);
    let create = dump::generate_create_statement(collection, false, "dense", "sparse", "", "");
    let body = format!("-- Points: 0\n\n{}", create);
    let footer = "-- Written: 0\n-- Skipped: 0".to_string();

    let final_output = format!("{}\n\n{}\n\n{}", header, body, footer);

    if let Some(parent) = std::path::Path::new(output).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output, &final_output)?;

    Ok(format!(
        "Dumped collection '{}' to {} (0 written, 0 skipped)",
        collection, output
    ))
}

pub fn handle_version() -> Result<(), Box<dyn std::error::Error>> {
    let resp = output::VersionResponse {
        ok: true,
        command: "version".to_string(),
        version: VERSION.to_string(),
        message: format!("qql version {}", VERSION),
    };
    let s = serde_json::to_string_pretty(&resp)?;
    println!("{}", s);
    Ok(())
}

// ── Explain implementation ────────────────────────────────────

fn explain_query(query: &str) -> Result<String, String> {
    let stmt = Parser::parse(query).map_err(|e| format!("parse error: {}", e))?;

    let mut plan = String::new();
    explain_stmt(&stmt, &mut plan);
    plan.push_str("Action: Explain-only mode (no Qdrant server)\n");

    Ok(plan)
}

fn explain_stmt(stmt: &Stmt, plan: &mut String) {
    match stmt {
        Stmt::ShowCollections => {
            plan.push_str("Statement: SHOW COLLECTIONS\n");
        }
        Stmt::ShowCollection(collection) => {
            plan.push_str(&format!("Statement: SHOW COLLECTION {}\n", collection));
        }
        Stmt::CreateCollection(s) => {
            plan.push_str(&format!("Statement: CREATE COLLECTION {}\n", s.collection));
            if let Some(model) = &s.model {
                plan.push_str(&format!("Model: {}\n", model));
            }
            if s.rerank {
                plan.push_str("Type: HYBRID + RERANK (dense + sparse + ColBERT multivector)\n");
            } else if s.hybrid {
                plan.push_str("Type: HYBRID (dense + sparse)\n");
            } else {
                plan.push_str("Type: DENSE\n");
            }
            for v in &s.vectors {
                plan.push_str(&format!("Vector: {}, Size: {}\n", v.name, v.size));
            }
        }
        Stmt::AlterCollection(s) => {
            plan.push_str(&format!("Statement: ALTER COLLECTION {}\n", s.collection));
        }
        Stmt::DropCollection(s) => {
            plan.push_str(&format!("Statement: DROP COLLECTION {}\n", s.collection));
        }
        Stmt::Insert(s) => {
            plan.push_str(&format!("Statement: INSERT INTO {}\n", s.collection));
            if let Some(model) = &s.model {
                plan.push_str(&format!("Model: {}\n", model));
            }
            plan.push_str(&format!("Rows: {}\n", s.values_list.len()));
        }
        Stmt::Select(s) => {
            plan.push_str(&format!(
                "Statement: SELECT * FROM {} WHERE id = '{:?}'\n",
                s.collection, s.point_id
            ));
        }
        Stmt::Scroll(s) => {
            plan.push_str(&format!(
                "Statement: SCROLL FROM {} LIMIT {}\n",
                s.collection, s.limit
            ));
        }
        Stmt::Query(q) => {
            let mode_str = match q.mode {
                qql_core::ast::QueryMode::OrderBy => "ORDER BY",
                qql_core::ast::QueryMode::Sample => "SAMPLE",
                qql_core::ast::QueryMode::RelevanceFeedback => "RELEVANCE FEEDBACK",
                _ => "",
            };
            let coll = q
                .collection
                .as_ref()
                .map(|c| c.as_ref())
                .unwrap_or("<none>");
            if !mode_str.is_empty() {
                plan.push_str(&format!(
                    "Statement: QUERY {} FROM {} LIMIT {}\n",
                    mode_str, coll, q.limit
                ));
            } else {
                plan.push_str(&format!(
                    "Statement: QUERY FROM {} LIMIT {}\n",
                    coll, q.limit
                ));
            }
            if let Some(text) = &q.query_text {
                plan.push_str(&format!("Query: '{}'\n", text));
            }
            if !q.raw_vector.is_empty() {
                plan.push_str(&format!("Raw Vector: {:?}\n", q.raw_vector));
            }
            match q.query_type {
                qql_core::ast::QueryType::Hybrid => plan.push_str("Using: HYBRID\n"),
                qql_core::ast::QueryType::Sparse => plan.push_str("Using: SPARSE\n"),
                qql_core::ast::QueryType::Dense => {}
            }
            if let Some(u) = &q.using_ {
                plan.push_str(&format!("Using: '{}'\n", u));
            }
            if let Some(m) = &q.model {
                plan.push_str(&format!("Model: {}\n", m));
            }
            if q.offset > 0 {
                plan.push_str(&format!("Offset: {}\n", q.offset));
            }
            if let Some(th) = &q.score_threshold {
                plan.push_str(&format!("Score threshold: {}\n", th));
            }
            if let Some(gb) = &q.group_by {
                plan.push_str(&format!("Group by: {}\n", gb));
            }
            if q.rerank {
                plan.push_str("Rerank: enabled\n");
            }
            if !q.ctes.is_empty() {
                plan.push_str(&format!("CTEs: {} defined\n", q.ctes.len()));
            }
            if !q.prefetch_refs.is_empty() {
                plan.push_str(&format!("Prefetch refs: {}\n", q.prefetch_refs.len()));
            }
            if let Some(ft) = &q.fusion_type {
                plan.push_str(&format!("Fusion: {}\n", ft));
            }
        }
        Stmt::Delete(s) => {
            if let Some(field) = &s.field {
                plan.push_str(&format!(
                    "Statement: DELETE FROM {} WHERE {} = '{:?}'\n",
                    s.collection, field, s.value
                ));
            } else {
                plan.push_str(&format!(
                    "Statement: DELETE FROM {} WHERE id = '{:?}'\n",
                    s.collection, s.point_id
                ));
            }
        }
        Stmt::UpdateVector(s) => {
            plan.push_str(&format!(
                "Statement: UPDATE {} SET VECTOR = [...] WHERE id = '{:?}'\n",
                s.collection, s.point_id
            ));
        }
        Stmt::UpdatePayload(s) => {
            plan.push_str(&format!(
                "Statement: UPDATE {} SET PAYLOAD = {{...}} WHERE id = '{:?}'\n",
                s.collection, s.point_id
            ));
        }
        Stmt::CreateIndex(s) => {
            plan.push_str(&format!(
                "Statement: CREATE INDEX ON COLLECTION {} FOR {} TYPE {}\n",
                s.collection, s.field, s.field_type
            ));
        }
    }
}

// ── REPL helpers ──────────────────────────────────────────────

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
    println!(
        r#"
\x1b[1mAvailable Statements:\x1b[0m

  \x1b[33mINSERT INTO\x1b[0m COLLECTION <name> \x1b[33mVALUES\x1b[0m {{'text': '...', ...}}

  \x1b[33mCREATE COLLECTION\x1b[0m <name> [\x1b[33mHYBRID\x1b[0m [\x1b[33mRERANK\x1b[0m]]

  \x1b[33mDROP COLLECTION\x1b[0m <name>

  \x1b[33mSHOW COLLECTIONS\x1b[0m

  \x1b[33mQUERY\x1b[0m ['<text>' | <id> | NEAREST '<text>' | ...]
      \x1b[33mFROM\x1b[0m <collection> \x1b[33mLIMIT\x1b[0m <n>

  \x1b[33mSELECT\x1b[0m * \x1b[33mFROM\x1b[0m <name> \x1b[33mWHERE id =\x1b[0m '<id>|<int>'

  \x1b[33mSCROLL FROM\x1b[0m <name> [\x1b[33mWHERE\x1b[0m <filter>] [\x1b[33mAFTER\x1b[0m '<id>'] \x1b[33mLIMIT\x1b[0m <n>

  \x1b[33mDELETE FROM\x1b[0m <name> \x1b[33mWHERE\x1b[0m id = '<id>' | <field> = '<value>'

\x1b[1mBuilt-in Commands:\x1b[0m

  \x1b[36mhelp\x1b[0m, \x1b[36m?\x1b[0m           Show this help
  \x1b[36mexplain <query>\x1b[0m  Show query plan without executing
  \x1b[36mexecute <file>\x1b[0m  Run a .qql script file
  \x1b[36m\e <file>\x1b[0m        Shortcut for execute
  \x1b[36mdump <name> <file>\x1b[0m  Dump a collection to a .qql script file
  \x1b[36mexit\x1b[0m, \x1b[36mquit\x1b[0m      Exit the shell

\x1b[1mKeyboard Shortcuts:\x1b[0m

  Ctrl-C         Cancel current input
  Ctrl-D         Exit shell
"#
    );
}
