//! Paste shapes: HTTP snippets (`METHOD /path` + JSON) and `curl` commands.
//!
//! Docs and blogs show requests as `METHOD /path` plus a JSON body, or as full
//! `curl` invocations — never the wrapped `{"method","path","body"}` envelope
//! the recorder writes. This module parses those paste shapes back into
//! `(method, path, body)` triples that reuse the shared
//! [`crate::convert::convert_method_path_body`] lowering, so envelope,
//! snippet, and curl inputs cannot drift.
//!
//! Fail-closed rules: `$VAR`/`${VAR}`/`$(…)`/backticks cannot resolve offline,
//! `--data @file` points at bytes the pure [`crate::convert`] function cannot
//! read, and multiple `--data` flags join with `&` in curl (never valid JSON).
//! Full `curl` flag coverage is intentionally bounded — unknown boolean flags
//! are skipped, but anything that carries a value the parse cannot see fails
//! rather than silently misreading the URL or body.

use qql_core::ast::Stmt;
use serde_json::Value;

use crate::ConvertError;
use crate::shell::{self, Item, Word};

pub(crate) struct PastedRequest {
    pub method: String,
    pub path: String,
    pub body: Option<Value>,
    pub line: usize,
}

const METHODS: [&str; 5] = ["GET", "POST", "PUT", "PATCH", "DELETE"];

fn is_method(token: &str) -> bool {
    METHODS.iter().any(|m| m.eq_ignore_ascii_case(token))
}

/// Entry point for [`crate::convert::convert_stmts`]: `None` when the input is
/// not a paste shape (plain JSON / JSONL fall through untouched), otherwise
/// the lowered statements. Errors on a single pasted request surface directly;
/// with several requests the failing one is wrapped in `InvalidLine`.
pub(crate) fn convert_pasted(
    input: &str,
    collection: Option<&str>,
) -> Option<Result<Vec<Stmt>, ConvertError>> {
    let docs = match parse_pasted(input)? {
        Ok(docs) => docs,
        Err(e) => return Some(Err(e)),
    };
    let multi = docs.len() > 1;
    let mut out = Vec::new();
    for doc in &docs {
        match crate::convert::convert_method_path_body(
            &doc.method,
            &doc.path,
            None,
            doc.body.as_ref(),
            collection,
        ) {
            Ok(stmts) => out.extend(stmts),
            Err(e) => {
                return Some(Err(if multi {
                    ConvertError::InvalidLine {
                        line: doc.line,
                        source: Box::new(e),
                    }
                } else {
                    e
                }));
            }
        }
    }
    Some(Ok(out))
}

fn parse_pasted(input: &str) -> Option<Result<Vec<PastedRequest>, ConvertError>> {
    let input = strip_prompt(input);
    let first = input.split_whitespace().next()?;
    if first == "curl" || first == "curl.exe" {
        return Some(parse_curl_docs(input));
    }
    if is_method(first) && header_shape(input.lines().next().unwrap_or("")) {
        return Some(parse_snippet_docs(input));
    }
    None
}

/// A leading `$ ` shell prompt is paste noise, not input.
fn strip_prompt(input: &str) -> &str {
    let trimmed = input.trim_start();
    match trimmed.strip_prefix('$') {
        Some(rest) if rest.starts_with(char::is_whitespace) => rest.trim_start(),
        _ => trimmed,
    }
}

/// Whether a line opens a snippet doc: `METHOD <path-or-URL> [HTTP/x]`.
fn header_shape(line: &str) -> bool {
    let mut parts = line.split_whitespace();
    matches!(parts.next(), Some(m) if is_method(m))
        && matches!(parts.next(), Some(p) if p.starts_with('/') || p.contains("://"))
}

/// Strip `scheme://authority` from a URL, keeping `path?query`. Bare paths
/// pass through; endpoint resolution validates them downstream.
fn url_to_path(url: &str) -> &str {
    match url.find("://") {
        Some(pos) => {
            let after = &url[pos + 3..];
            match after.find('/') {
                Some(i) => &after[i..],
                None => "/",
            }
        }
        None => url,
    }
}

// ── HTTP snippets ────────────────────────────────────────────────────────────

fn parse_snippet_docs(input: &str) -> Result<Vec<PastedRequest>, ConvertError> {
    let mut docs = Vec::new();
    let mut rest = input;
    let mut line = 1usize;
    loop {
        (line, rest) = skip_blank_lines(rest, line);
        if rest.is_empty() {
            break;
        }
        let (header, after) = split_first_line(rest);
        let header_line = line;
        line += 1;
        let (method, path) = parse_header(header)?;
        rest = after;
        let (body, r, l) = take_json_body(rest, line)?;
        rest = r;
        line = l;
        docs.push(PastedRequest {
            method,
            path,
            body,
            line: header_line,
        });
    }
    if docs.is_empty() {
        return Err(ConvertError::undecodable("no request found"));
    }
    Ok(docs)
}

fn parse_header(header: &str) -> Result<(String, String), ConvertError> {
    let bad = || {
        ConvertError::undecodable(format!(
            "expected `METHOD /path` (e.g. `POST /collections/docs/points/query`), got `{header}`"
        ))
    };
    let mut parts = header.split_whitespace();
    let (Some(method), Some(raw_path)) = (parts.next(), parts.next()) else {
        return Err(bad());
    };
    if !is_method(method) {
        return Err(bad());
    }
    match parts.next() {
        None => {}
        Some(v) if v.to_ascii_uppercase().starts_with("HTTP/") && parts.next().is_none() => {}
        _ => return Err(bad()),
    }
    let path = url_to_path(raw_path);
    reject_dollar(path)?;
    Ok((method.to_string(), path.to_string()))
}

/// Read one JSON body after a snippet header: `None` when the next doc (or
/// EOF) follows immediately, so bodyless routes (`GET /collections`) paste
/// cleanly. Bodies may span lines; the first complete JSON value wins and any
/// trailing non-blank text on its lines fails rather than silently dropping.
fn take_json_body(rest: &str, line: usize) -> Result<(Option<Value>, &str, usize), ConvertError> {
    let mut buf = String::new();
    let mut cur_rest = rest;
    let mut cur_line = line;
    loop {
        if buf.trim().is_empty() {
            (cur_line, cur_rest) = skip_blank_lines(cur_rest, cur_line);
            if cur_rest.is_empty() {
                return Ok((None, cur_rest, cur_line));
            }
            if header_shape(split_first_line(cur_rest).0) {
                return Ok((None, cur_rest, cur_line));
            }
        }
        if cur_rest.is_empty() {
            let msg = match first_json_value(&buf) {
                Err(m) => m,
                _ => "unexpected end of input in JSON body".to_string(),
            };
            return Err(ConvertError::InvalidJson(msg));
        }
        let (this, after) = split_first_line(cur_rest);
        buf.push_str(this);
        buf.push('\n');
        cur_rest = after;
        cur_line += 1;
        match first_json_value(&buf) {
            Ok(Some((value, used))) => {
                if !buf[used..].trim().is_empty() {
                    return Err(ConvertError::InvalidJson(
                        "unexpected trailing characters after JSON body".to_string(),
                    ));
                }
                return Ok((Some(value), cur_rest, cur_line));
            }
            Ok(None) => {}
            Err(msg) => return Err(ConvertError::InvalidJson(msg)),
        }
    }
}

/// First complete JSON value in `buf`: `None` when empty or truncated mid-value
/// (callers feed more lines), `Err` on a real syntax error.
fn first_json_value(buf: &str) -> Result<Option<(Value, usize)>, String> {
    let mut stream = serde_json::Deserializer::from_str(buf).into_iter::<Value>();
    match stream.next() {
        None => Ok(None),
        Some(Ok(value)) => Ok(Some((value, stream.byte_offset()))),
        Some(Err(e)) if e.is_eof() => Ok(None),
        Some(Err(e)) => Err(e.to_string()),
    }
}

fn skip_blank_lines(s: &str, line: usize) -> (usize, &str) {
    let (mut l, mut rest) = (line, s);
    loop {
        let (first, after) = split_first_line(rest);
        if first.trim().is_empty() && !rest.is_empty() {
            rest = after;
            l += 1;
        } else {
            return (l, rest);
        }
    }
}

fn split_first_line(s: &str) -> (&str, &str) {
    match s.find('\n') {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => (s, ""),
    }
}

// ── curl ─────────────────────────────────────────────────────────────────────

fn parse_curl_docs(input: &str) -> Result<Vec<PastedRequest>, ConvertError> {
    let mut commands: Vec<Vec<Word>> = Vec::new();
    let mut cur: Vec<Word> = Vec::new();
    for item in shell::tokenize(input)? {
        match item {
            Item::Word(w) => {
                if cur.is_empty() && w.text == "$" {
                    continue; // per-command shell prompt
                }
                cur.push(w);
            }
            Item::Newline(_) | Item::Semi | Item::And | Item::Or => {
                if !cur.is_empty() {
                    commands.push(std::mem::take(&mut cur));
                }
            }
        }
    }
    if !cur.is_empty() {
        commands.push(cur);
    }
    if commands.is_empty() {
        return Err(ConvertError::undecodable("curl: no command found"));
    }
    commands.iter().map(|cmd| parse_curl_command(cmd)).collect()
}

/// Short flags that consume the next token (or an attached value).
const SHORT_VALUE_FLAGS: &str = "XduHAemow";
/// Short boolean flags (also valid combined, e.g. `-sSf`).
const SHORT_BOOL_FLAGS: &str = "sSfkvqNLlipO#012346gG";
/// Long flags that consume the next token (or `=value`).
const LONG_VALUE_FLAGS: [&str; 22] = [
    "request",
    "url",
    "header",
    "user",
    "user-agent",
    "referer",
    "max-time",
    "connect-timeout",
    "retry",
    "retry-delay",
    "output",
    "write-out",
    "proxy",
    "proxy-user",
    "cacert",
    "cert",
    "key",
    "data",
    "data-raw",
    "data-ascii",
    "data-binary",
    "json",
];

fn parse_curl_command(words: &[Word]) -> Result<PastedRequest, ConvertError> {
    let line = words[0].line;
    if words[0].text != "curl" && words[0].text != "curl.exe" {
        return Err(ConvertError::undecodable(format!(
            "expected a `curl` command, got `{}`",
            words[0].text
        )));
    }
    let mut method: Option<String> = None;
    let mut url: Option<String> = None;
    let mut datas: Vec<String> = Vec::new();
    let mut get_flag = false;
    let mut head_flag = false;

    let mut i = 1;
    while i < words.len() {
        let w = &words[i].text;
        if w == "-X" || w == "--request" {
            i += 1;
            method = Some(next_value(words, i, w)?.to_string());
        } else if w.starts_with("-X") && w.len() > 2 {
            method = Some(w[2..].to_string());
        } else if is_data_flag(w) {
            i += 1;
            datas.push(check_data_value(next_value(words, i, w)?)?);
        } else if w.starts_with("-d") && w.len() > 2 {
            datas.push(check_data_value(&w[2..])?);
        } else if w == "-T" || w == "--upload-file" || w == "-K" || w == "--config" {
            return Err(ConvertError::undecodable(format!(
                "curl: `{w}` reads a file — paste literal values instead"
            )));
        } else if w == "--data-urlencode" {
            return Err(ConvertError::undecodable(
                "curl: `--data-urlencode` has no QQL mapping — pass a JSON body with `-d`",
            ));
        } else if w.starts_with("--") {
            i = parse_long_flag(
                words,
                i,
                &mut method,
                &mut url,
                &mut datas,
                &mut get_flag,
                &mut head_flag,
            )?;
        } else if w.starts_with('-') && w.len() > 1 {
            i = parse_shorts(
                words,
                i,
                &mut method,
                &mut datas,
                &mut get_flag,
                &mut head_flag,
            )?;
        } else if url.is_none() {
            url = Some(w.clone());
        } else {
            return Err(ConvertError::undecodable(format!(
                "curl: unexpected extra argument `{w}`"
            )));
        }
        i += 1;
    }

    let Some(url) = url else {
        return Err(ConvertError::undecodable(
            "curl: no URL — expected `curl [flags] <url>`",
        ));
    };
    let body = match datas.len() {
        0 => None,
        1 => Some(
            serde_json::from_str(&datas[0])
                .map_err(|e| ConvertError::InvalidJson(e.to_string()))?,
        ),
        _ => {
            return Err(ConvertError::undecodable(
                "curl: multiple `--data` flags join with `&` — pass a single JSON body",
            ));
        }
    };
    let method = match method {
        Some(m) => m,
        None if head_flag => "HEAD".to_string(),
        None if get_flag || body.is_none() => "GET".to_string(),
        None => "POST".to_string(),
    };
    if body.is_some() && (method.eq_ignore_ascii_case("GET") || method.eq_ignore_ascii_case("HEAD"))
    {
        return Err(ConvertError::undecodable(format!(
            "curl: {method} with `--data` has no QQL mapping — drop `--data` or pass `-X POST`"
        )));
    }
    let path = url_to_path(&url);
    reject_dollar(path)?;
    Ok(PastedRequest {
        method,
        path: path.to_string(),
        body,
        line,
    })
}

fn is_data_flag(w: &str) -> bool {
    matches!(
        w,
        "-d" | "--data" | "--data-raw" | "--data-ascii" | "--data-binary" | "--json"
    )
}

fn next_value<'a>(words: &'a [Word], i: usize, flag: &str) -> Result<&'a str, ConvertError> {
    words
        .get(i)
        .map(|w| w.text.as_str())
        .ok_or_else(|| ConvertError::undecodable(format!("curl: `{flag}` needs a value")))
}

/// `--data @file` (and `@-` stdin) point at bytes an offline converter cannot
/// read. Shell process substitution (`<(…)`) is not a file either.
fn check_data_value(value: &str) -> Result<String, ConvertError> {
    if value.starts_with('@') {
        return Err(ConvertError::undecodable(
            "curl: `--data @file` reads a file — paste the JSON body inline instead",
        ));
    }
    Ok(value.to_string())
}

fn parse_long_flag(
    words: &[Word],
    i: usize,
    method: &mut Option<String>,
    url: &mut Option<String>,
    datas: &mut Vec<String>,
    get_flag: &mut bool,
    head_flag: &mut bool,
) -> Result<usize, ConvertError> {
    let w = &words[i].text;
    let (name, attached) = match w[2..].split_once('=') {
        Some((n, v)) => (n, Some(v)),
        None => (w[2..].as_ref(), None),
    };
    let mut i = i;
    let value = match attached {
        Some(v) => v.to_string(),
        None if LONG_VALUE_FLAGS.contains(&name) => {
            i += 1;
            next_value(words, i, w)?.to_string()
        }
        None => String::new(),
    };
    match name {
        "request" => *method = Some(value),
        "data" | "data-raw" | "data-ascii" | "data-binary" | "json" => {
            datas.push(check_data_value(&value)?);
        }
        "url" => *url = Some(value),
        "get" => *get_flag = true,
        "head" => *head_flag = true,
        "data-urlencode" | "config" | "upload-file" => {
            return Err(ConvertError::undecodable(format!(
                "curl: `--{name}` has no QQL mapping — paste literal values instead"
            )));
        }
        _ => {} // unknown boolean flags (e.g. `--fail`, `--compressed`) are inert
    }
    Ok(i)
}

fn parse_shorts(
    words: &[Word],
    i: usize,
    method: &mut Option<String>,
    datas: &mut Vec<String>,
    get_flag: &mut bool,
    head_flag: &mut bool,
) -> Result<usize, ConvertError> {
    let w = &words[i].text;
    let mut i = i;
    let mut chars = w[1..].chars();
    while let Some(c) = chars.next() {
        if SHORT_VALUE_FLAGS.contains(c) {
            let rest: String = chars.collect();
            let value = if rest.is_empty() {
                i += 1;
                next_value(words, i, w)?.to_string()
            } else {
                rest
            };
            match c {
                'X' => *method = Some(value),
                'd' => datas.push(check_data_value(&value)?),
                'u' | 'H' | 'A' | 'e' | 'm' | 'o' | 'w' => {} // credentials / tuning: inert
                _ => unreachable!(),
            }
            break; // the remainder was the value
        } else if SHORT_BOOL_FLAGS.contains(c) {
            match c {
                'G' => *get_flag = true,
                'I' => *head_flag = true,
                _ => {}
            }
        } else if c == 'T' || c == 'K' {
            return Err(ConvertError::undecodable(format!(
                "curl: `-{c}` reads a file — paste literal values instead"
            )));
        } else {
            return Err(ConvertError::undecodable(format!(
                "curl: unsupported flag `-{c}` in `{w}`"
            )));
        }
    }
    Ok(i)
}

/// `$` survives only inside single quotes (no shell expansion there), so a `$`
/// in the final path is always a variable or placeholder the paste left
/// behind — emitting it would produce QQL that cannot re-parse.
fn reject_dollar(path: &str) -> Result<(), ConvertError> {
    if path.contains('$') {
        return Err(ConvertError::UndecodableBody {
            detail: "path contains `$` — replace variables/placeholders with literal values"
                .to_string(),
        });
    }
    Ok(())
}
