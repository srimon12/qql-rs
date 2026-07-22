use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// Sparse embedding (indices + values). Transport-neutral — not a protobuf type.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SparseVector {
    pub indices: Vec<u32>,
    pub values: Vec<f32>,
}

const OFFSET32: u32 = 2166136261;
const PRIME32: u32 = 16777619;

/// Fast Identity Hasher for u32 keys (avoids SipHash overhead).
#[derive(Default)]
pub struct IdentityHasher(u64);

impl Hasher for IdentityHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }

    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 << 8) | (b as u64);
        }
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.0 = i as u64;
    }
}

type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<IdentityHasher>>;

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

/// Fast on-the-fly hash computation for ASCII slices with lowercase conversion.
#[inline]
fn hash_token_bytes(bytes: &[u8]) -> u32 {
    let mut h: u32 = OFFSET32;

    let l = bytes.len() as u64;
    for i in 0..8 {
        h ^= (l >> (i * 8)) as u32 & 0xff;
        h = h.wrapping_mul(PRIME32);
    }

    for &b in bytes {
        let lower_b = if b.is_ascii_uppercase() { b + 32 } else { b };
        h ^= lower_b as u32;
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
            return String::from_utf8(buf).unwrap_or_else(|_| s.to_ascii_lowercase());
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

#[inline]
fn is_valid_token_len(len: usize, first_byte: u8) -> bool {
    len >= 2 || (len == 1 && (first_byte == b'c' || first_byte == b'C'))
}

/// Zero-allocation fast pass token hashing for ASCII strings
fn hash_tokens_ascii_fast(text: &str) -> (Vec<u32>, usize) {
    let bytes = text.as_bytes();
    let mut raw_hashes = Vec::with_capacity(bytes.len() / 4 + 1);
    let mut start: Option<usize> = None;

    for i in 0..bytes.len() {
        let ch = bytes[i];
        if is_token_byte(ch) {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(s) = start {
            let slice = &bytes[s..i];
            let len = slice.len();
            let first_b = slice[0];

            if !slice.contains(&b'-') {
                if is_valid_token_len(len, first_b) {
                    raw_hashes.push(hash_token_bytes(slice));
                }
            } else {
                let mut sub_start: Option<usize> = None;
                for idx in 0..slice.len() {
                    if slice[idx] == b'-' {
                        if let Some(ss) = sub_start {
                            let sub_slice = &slice[ss..idx];
                            if is_valid_token_len(sub_slice.len(), sub_slice[0]) {
                                raw_hashes.push(hash_token_bytes(sub_slice));
                            }
                            sub_start = None;
                        }
                    } else if sub_start.is_none() {
                        sub_start = Some(idx);
                    }
                }
                if let Some(ss) = sub_start {
                    let sub_slice = &slice[ss..];
                    if is_valid_token_len(sub_slice.len(), sub_slice[0]) {
                        raw_hashes.push(hash_token_bytes(sub_slice));
                    }
                }
            }
            start = None;
        }
    }

    if let Some(s) = start {
        let slice = &bytes[s..];
        let len = slice.len();
        let first_b = slice[0];

        if !slice.contains(&b'-') {
            if is_valid_token_len(len, first_b) {
                raw_hashes.push(hash_token_bytes(slice));
            }
        } else {
            let mut sub_start: Option<usize> = None;
            for idx in 0..slice.len() {
                if slice[idx] == b'-' {
                    if let Some(ss) = sub_start {
                        let sub_slice = &slice[ss..idx];
                        if is_valid_token_len(sub_slice.len(), sub_slice[0]) {
                            raw_hashes.push(hash_token_bytes(sub_slice));
                        }
                        sub_start = None;
                    }
                } else if sub_start.is_none() {
                    sub_start = Some(idx);
                }
            }
            if let Some(ss) = sub_start {
                let sub_slice = &slice[ss..];
                if is_valid_token_len(sub_slice.len(), sub_slice[0]) {
                    raw_hashes.push(hash_token_bytes(sub_slice));
                }
            }
        }
    }

    let token_count = raw_hashes.len();
    (raw_hashes, token_count)
}

pub fn build_query(text: &str) -> SparseVector {
    let (counts, _total_tokens) = if is_ascii(text) {
        let (hashes, total_tokens) = hash_tokens_ascii_fast(text);
        let mut counts: FastMap<u32, f32> = FastMap::with_capacity_and_hasher(
            hashes.len(),
            BuildHasherDefault::<IdentityHasher>::default(),
        );
        for h in hashes {
            *counts.entry(h).or_insert(0.0) += 1.0;
        }
        (counts, total_tokens)
    } else {
        let tokens = tokenize(text);
        let mut counts: FastMap<u32, f32> = FastMap::with_capacity_and_hasher(
            tokens.len(),
            BuildHasherDefault::<IdentityHasher>::default(),
        );
        let total_tokens = tokens.len();
        for token in &tokens {
            *counts.entry(hash_token(token)).or_insert(0.0) += 1.0;
        }
        (counts, total_tokens)
    };

    if counts.is_empty() {
        return SparseVector {
            indices: Vec::new(),
            values: Vec::new(),
        };
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
    let (counts, total_tokens) = if is_ascii(text) {
        let (hashes, total_tokens) = hash_tokens_ascii_fast(text);
        let mut counts: FastMap<u32, f32> = FastMap::with_capacity_and_hasher(
            hashes.len(),
            BuildHasherDefault::<IdentityHasher>::default(),
        );
        for h in hashes {
            *counts.entry(h).or_insert(0.0) += 1.0;
        }
        (counts, total_tokens)
    } else {
        let tokens = tokenize(text);
        let mut counts: FastMap<u32, f32> = FastMap::with_capacity_and_hasher(
            tokens.len(),
            BuildHasherDefault::<IdentityHasher>::default(),
        );
        let total_tokens = tokens.len();
        for token in &tokens {
            *counts.entry(hash_token(token)).or_insert(0.0) += 1.0;
        }
        (counts, total_tokens)
    };

    if counts.is_empty() {
        return SparseVector {
            indices: Vec::new(),
            values: Vec::new(),
        };
    }

    let doc_len = total_tokens as f64;
    let safe_avgdl = if avgdl <= 0.0 { 256.0 } else { avgdl };
    let denom_scale = k1 * (1.0 - b + b * doc_len / safe_avgdl);
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
