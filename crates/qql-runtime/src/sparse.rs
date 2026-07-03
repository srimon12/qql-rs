use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct SparseVector {
    pub indices: Vec<u32>,
    pub values: Vec<f32>,
}

const OFFSET32: u32 = 2166136261;
const PRIME32: u32 = 16777619;

pub fn hash_token(token: &str) -> u32 {
    let mut h: u32 = OFFSET32;

    let l = token.len() as u64;
    for i in 0..8 {
        h ^= (l >> (i * 8)) as u32 & 0xff;
        h = h.wrapping_mul(PRIME32);
    }

    for &b in token.as_bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(PRIME32);
    }

    h
}

pub fn tokenize(text: &str) -> Vec<String> {
    if is_ascii(text) {
        tokenize_ascii(text)
    } else {
        tokenize_unicode(text)
    }
}

fn is_ascii(s: &str) -> bool {
    s.bytes().all(|b| b < 0x80)
}

fn is_token_byte(ch: u8) -> bool {
    ch.is_ascii_lowercase()
        || ch.is_ascii_uppercase()
        || ch.is_ascii_digit()
        || ch == b'_'
        || ch == b'-'
}

fn is_token_rune(r: char) -> bool {
    r.is_alphabetic() || r.is_ascii_digit() || r == '_' || r == '-'
}

fn to_lower_ascii(s: &str) -> String {
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b.is_ascii_uppercase() {
            let mut buf = Vec::with_capacity(bytes.len());
            buf.extend_from_slice(&bytes[..i]);
            for &b in &bytes[i..] {
                buf.push(if b.is_ascii_uppercase() { b + 32 } else { b });
            }
            return unsafe { String::from_utf8_unchecked(buf) };
        }
    }
    s.to_string()
}

fn unicode_to_lower(s: &str) -> String {
    let mut buf = String::with_capacity(s.len());
    for c in s.chars() {
        for lc in c.to_lowercase() {
            buf.push(lc);
        }
    }
    buf
}

fn maybe_token(s: &str) -> Option<String> {
    if s.len() >= 2 {
        return Some(s.to_string());
    }
    if s.len() == 1 {
        let b = s.as_bytes()[0];
        if b == b'c' {
            return Some(s.to_string());
        }
    }
    None
}

fn append_tokens(tokens: &mut Vec<String>, raw: &str) {
    let has_hyphen = raw.contains('-');
    if !has_hyphen {
        if let Some(tok) = maybe_token(raw) {
            tokens.push(tok);
        }
        return;
    }

    let mut start: Option<usize> = None;
    for (i, ch) in raw.char_indices() {
        if ch == '-' {
            if let Some(s) = start {
                if let Some(tok) = maybe_token(&raw[s..i]) {
                    tokens.push(tok);
                }
                start = None;
            }
        } else {
            if start.is_none() {
                start = Some(i);
            }
        }
    }
    if let Some(s) = start {
        if let Some(tok) = maybe_token(&raw[s..]) {
            tokens.push(tok);
        }
    }
}

fn tokenize_ascii(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut start: Option<usize> = None;
    let bytes = text.as_bytes();

    for i in 0..bytes.len() {
        let ch = bytes[i];
        if is_token_byte(ch) {
            if start.is_none() {
                start = Some(i);
            }
        } else {
            if let Some(s) = start {
                append_tokens(&mut tokens, &to_lower_ascii(&text[s..i]));
                start = None;
            }
        }
    }
    if let Some(s) = start {
        append_tokens(&mut tokens, &to_lower_ascii(&text[s..]));
    }

    tokens
}

fn tokenize_unicode(text: &str) -> Vec<String> {
    let lower = unicode_to_lower(text);
    let mut tokens = Vec::new();
    let mut start: Option<usize> = None;

    for (i, ch) in lower.char_indices() {
        if is_token_rune(ch) {
            if start.is_none() {
                start = Some(i);
            }
        } else {
            if let Some(s) = start {
                append_tokens(&mut tokens, &lower[s..i]);
                start = None;
            }
        }
    }
    if let Some(s) = start {
        append_tokens(&mut tokens, &lower[s..]);
    }

    tokens
}

pub fn build_query(text: &str) -> SparseVector {
    let tokens = tokenize(text);
    if tokens.is_empty() {
        return SparseVector {
            indices: Vec::new(),
            values: Vec::new(),
        };
    }

    let mut counts: HashMap<u32, f32> = HashMap::with_capacity(tokens.len());
    for token in &tokens {
        *counts.entry(hash_token(token)).or_insert(0.0) += 1.0;
    }

    let mut indices: Vec<u32> = counts.keys().copied().collect();
    indices.sort_unstable();

    let values: Vec<f32> = indices
        .iter()
        .map(|idx| 1.0 + (counts[idx] as f64).ln() as f32)
        .collect();

    SparseVector { indices, values }
}

pub fn build_document(text: &str, k1: f64, b: f64, avgdl: f64) -> SparseVector {
    let tokens = tokenize(text);
    if tokens.is_empty() {
        return SparseVector {
            indices: Vec::new(),
            values: Vec::new(),
        };
    }

    let mut counts: HashMap<u32, f32> = HashMap::with_capacity(tokens.len());
    for token in &tokens {
        *counts.entry(hash_token(token)).or_insert(0.0) += 1.0;
    }

    let doc_len = tokens.len() as f64;
    let denom_scale = k1 * (1.0 - b + b * doc_len / avgdl);
    let k1p1 = k1 + 1.0;

    let mut indices: Vec<u32> = counts.keys().copied().collect();
    indices.sort_unstable();

    let values: Vec<f32> = indices
        .iter()
        .map(|idx| {
            let tf_count = counts[idx] as f64;
            let denom = tf_count + denom_scale;
            (tf_count * k1p1 / denom) as f32
        })
        .collect();

    SparseVector { indices, values }
}

pub fn build_query_default(text: &str) -> SparseVector {
    build_query(text)
}

pub fn build_document_default(text: &str) -> SparseVector {
    build_document(text, 1.2, 0.75, 256.0)
}
