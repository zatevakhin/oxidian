use std::collections::BTreeSet;

use crate::{Error, Vault, VaultIndex, VaultPath};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnlinkedMention {
    pub source: VaultPath,
    pub target: VaultPath,
    pub line: u32,
    pub term: String,
    pub line_text: String,
}

impl VaultIndex {
    pub fn unlinked_mentions(
        &self,
        vault: &Vault,
        target: &VaultPath,
        limit: usize,
    ) -> crate::Result<Vec<UnlinkedMention>> {
        let Some(target_note) = self.note(target) else {
            return Ok(Vec::new());
        };

        let terms = mention_terms(target, target_note);
        if terms.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for (source, _note) in self.notes_iter() {
            if source == target {
                continue;
            }
            let abs = vault.to_abs(source);
            let text = std::fs::read_to_string(&abs).map_err(|e| Error::io(&abs, e))?;
            for m in scan_mentions_in_text(source, target, &terms, &text) {
                out.push(m);
                if out.len() >= limit {
                    return Ok(out);
                }
            }
        }

        Ok(out)
    }
}

fn mention_terms(target: &VaultPath, note: &crate::NoteMeta) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if let Some(stem) = target.as_path().file_stem().and_then(|s| s.to_str()) {
        let s = stem.trim();
        if !s.is_empty() {
            out.insert(s.to_lowercase());
        }
    }
    let title = note.title.trim();
    if !title.is_empty() {
        out.insert(title.to_lowercase());
    }
    for a in &note.aliases {
        let a = a.trim();
        if !a.is_empty() {
            out.insert(a.to_lowercase());
        }
    }
    out
}

fn scan_mentions_in_text(
    source: &VaultPath,
    target: &VaultPath,
    terms: &BTreeSet<String>,
    text: &str,
) -> Vec<UnlinkedMention> {
    let mut out = Vec::new();
    let (body, body_start_line) = split_frontmatter_text(text);

    let mut in_fenced = false;
    for (ix, line) in body.lines().enumerate() {
        let line_no = body_start_line + ix as u32;
        let t = line.trim_start();
        if t.starts_with("```") {
            in_fenced = !in_fenced;
            continue;
        }
        if in_fenced {
            continue;
        }

        let cleaned = strip_link_spans(line);
        let hay = cleaned.to_lowercase();

        for term in terms {
            if term.is_empty() {
                continue;
            }
            if find_wordish(&hay, term) {
                out.push(UnlinkedMention {
                    source: source.clone(),
                    target: target.clone(),
                    line: line_no,
                    term: term.clone(),
                    line_text: line.to_string(),
                });
            }
        }
    }

    out
}

fn split_frontmatter_text(content: &str) -> (&str, u32) {
    let Some(rest) = content
        .strip_prefix("---\n")
        .or_else(|| content.strip_prefix("---\r\n"))
    else {
        return (content, 1);
    };

    let mut idx = 0usize;
    let bytes = rest.as_bytes();
    while idx < bytes.len() {
        let line_end = match bytes[idx..].iter().position(|b| *b == b'\n') {
            Some(off) => idx + off + 1,
            None => bytes.len(),
        };
        let line = &rest[idx..line_end];
        let line_trim = line.trim_end_matches(['\r', '\n']);
        if line_trim == "---" {
            let body = &rest[line_end..];
            let start_line = 1 + content[..content.len() - body.len()]
                .bytes()
                .filter(|b| *b == b'\n')
                .count() as u32;
            return (body, start_line);
        }
        idx = line_end;
    }

    // Unclosed fence: don't try to interpret; treat as no frontmatter.
    (content, 1)
}

fn strip_link_spans(line: &str) -> String {
    // Replace link syntaxes with spaces to avoid matching mentions inside links.
    let mut out = String::with_capacity(line.len());
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        // wiki link or embed
        if bytes[i] == b'['
            && i + 1 < bytes.len()
            && bytes[i + 1] == b'['
            && let Some(end) = find_bytes(bytes, i + 2, b']', b']')
        {
            out.push(' ');
            i = end + 2;
            continue;
        }
        if bytes[i] == b'!'
            && i + 2 < bytes.len()
            && bytes[i + 1] == b'['
            && bytes[i + 2] == b'['
            && let Some(end) = find_bytes(bytes, i + 3, b']', b']')
        {
            out.push(' ');
            i = end + 2;
            continue;
        }

        // markdown link or embed: [text](target) or ![alt](target)
        if bytes[i] == b'[' || (bytes[i] == b'!' && i + 1 < bytes.len() && bytes[i + 1] == b'[') {
            let start = if bytes[i] == b'!' { i + 1 } else { i };
            if let Some(close_br) = bytes[start + 1..].iter().position(|b| *b == b']') {
                let j = start + 1 + close_br;
                if j + 1 < bytes.len()
                    && bytes[j + 1] == b'('
                    && let Some(close_paren) = bytes[j + 2..].iter().position(|b| *b == b')')
                {
                    out.push(' ');
                    i = j + 2 + close_paren + 1;
                    continue;
                }
            }
        }

        // autolink <...>
        if bytes[i] == b'<'
            && let Some(off) = bytes[i + 1..].iter().position(|b| *b == b'>')
        {
            out.push(' ');
            i = i + 1 + off + 1;
            continue;
        }

        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn find_bytes(bytes: &[u8], from: usize, a: u8, b: u8) -> Option<usize> {
    let mut i = from;
    while i + 1 < bytes.len() {
        if bytes[i] == a && bytes[i + 1] == b {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn find_wordish(hay: &str, needle: &str) -> bool {
    let mut start = 0usize;
    while let Some(pos) = hay[start..].find(needle) {
        let i = start + pos;
        let j = i + needle.len();
        if has_word_boundary(hay, i, j, needle) {
            return true;
        }
        start = i + 1;
    }
    false
}

fn has_word_boundary(hay: &str, i: usize, j: usize, needle: &str) -> bool {
    let bytes = hay.as_bytes();
    let nbytes = needle.as_bytes();
    let first = nbytes.first().copied().unwrap_or(b' ');
    let last = nbytes.last().copied().unwrap_or(b' ');
    let left_ok = if is_word_byte(first) {
        i == 0 || !is_word_byte(bytes[i.saturating_sub(1)])
    } else {
        true
    };
    let right_ok = if is_word_byte(last) {
        j >= bytes.len() || !is_word_byte(bytes.get(j).copied().unwrap_or(b' '))
    } else {
        true
    };
    left_ok && right_ok
}

fn is_word_byte(b: u8) -> bool {
    (b as char).is_ascii_alphanumeric() || b == b'_'
}
