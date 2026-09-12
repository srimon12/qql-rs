//! Minimal shell-ish tokenizer for pasted `curl` commands.
//!
//! Docs and terminal histories paste multi-line commands with `\`-continuations,
//! `'single'` / `"double"` quoting, `#` comments, and `;` / `&&` / `||`
//! separators. This recovers the word stream without executing anything: no
//! globbing, no expansion. Anything dynamic (`$VAR`, `${VAR}`, `$(…)`,
//! backticks) fails closed with [`ConvertError`], as do pipes and subshells —
//! a converter has no shell to run them in.

use crate::ConvertError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Word {
    pub text: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Item {
    Word(Word),
    Newline(usize),
    Semi,
    And,
    Or,
}

fn err(detail: impl Into<String>) -> ConvertError {
    ConvertError::UndecodableBody {
        detail: format!("curl: {}", detail.into()),
    }
}

/// Split pasted shell text into words and command separators.
///
/// Tracks 1-based original line numbers on every word so multi-command input
/// can report failures per command. `\` + newline continuations are joined
/// (the line count still advances, so numbers stay exact).
pub(crate) fn tokenize(input: &str) -> Result<Vec<Item>, ConvertError> {
    let mut items = Vec::new();
    let mut cur: Option<(String, usize)> = None;
    let mut chars = input.chars().peekable();
    let mut line = 1usize;

    let push = |cur: &mut Option<(String, usize)>, line: usize, c: char| match cur {
        Some((s, _)) => s.push(c),
        None => *cur = Some((c.to_string(), line)),
    };
    let flush = |cur: &mut Option<(String, usize)>, items: &mut Vec<Item>| {
        if let Some((text, line)) = cur.take() {
            items.push(Item::Word(Word { text, line }));
        }
    };

    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\r' => flush(&mut cur, &mut items),
            '\n' => {
                flush(&mut cur, &mut items);
                items.push(Item::Newline(line));
                line += 1;
            }
            '#' => {
                flush(&mut cur, &mut items);
                for ch in chars.by_ref() {
                    if ch == '\n' {
                        items.push(Item::Newline(line));
                        line += 1;
                        break;
                    }
                }
            }
            '\'' => {
                let start = cur.as_ref().map_or(line, |(_, l)| *l);
                let mut s = cur.take().map_or_else(String::new, |(t, _)| t);
                loop {
                    match chars.next() {
                        None => return Err(err("unterminated `'` quote")),
                        Some('\'') => break,
                        Some('\n') => {
                            line += 1;
                            s.push('\n');
                        }
                        Some(q) => s.push(q),
                    }
                }
                cur = Some((s, start));
            }
            '"' => {
                let start = cur.as_ref().map_or(line, |(_, l)| *l);
                let mut s = cur.take().map_or_else(String::new, |(t, _)| t);
                loop {
                    match chars.next() {
                        None => return Err(err("unterminated `\"` quote")),
                        Some('"') => break,
                        Some('\n') => {
                            line += 1;
                            s.push('\n');
                        }
                        Some('\\') => match chars.next() {
                            None => return Err(err("unterminated `\"` quote")),
                            Some('\n') => line += 1,
                            Some(e @ ('"' | '$' | '`' | '\\')) => s.push(e),
                            Some(e) => {
                                s.push('\\');
                                s.push(e);
                            }
                        },
                        Some('$') => match chars.peek() {
                            Some(n)
                                if n.is_alphanumeric() || *n == '_' || *n == '{' || *n == '(' =>
                            {
                                return Err(unresolved_var());
                            }
                            _ => s.push('$'),
                        },
                        Some('`') => return Err(substitution()),
                        Some(q) => s.push(q),
                    }
                }
                cur = Some((s, start));
            }
            '\\' => match chars.next() {
                None => return Err(err("trailing `\\`")),
                Some('\n') => line += 1,
                Some(e) => push(&mut cur, line, e),
            },
            ';' => {
                flush(&mut cur, &mut items);
                items.push(Item::Semi);
            }
            '&' => {
                if chars.peek() == Some(&'&') {
                    chars.next();
                    flush(&mut cur, &mut items);
                    items.push(Item::And);
                } else {
                    return Err(err("single `&` (background jobs cannot convert)"));
                }
            }
            '|' => {
                if chars.peek() == Some(&'|') {
                    chars.next();
                    flush(&mut cur, &mut items);
                    items.push(Item::Or);
                } else {
                    return Err(err("pipes cannot convert — paste one request"));
                }
            }
            '(' | ')' => return Err(err("subshells cannot convert")),
            '`' => return Err(substitution()),
            '$' => match chars.peek() {
                Some(n) if n.is_alphanumeric() || *n == '_' || *n == '{' || *n == '(' => {
                    return Err(unresolved_var());
                }
                _ => push(&mut cur, line, '$'),
            },
            _ => push(&mut cur, line, c),
        }
    }
    flush(&mut cur, &mut items);
    Ok(items)
}

fn unresolved_var() -> ConvertError {
    err("unresolved `$…` — replace variables with literal values")
}

fn substitution() -> ConvertError {
    err("command substitution cannot convert — paste literal values")
}
