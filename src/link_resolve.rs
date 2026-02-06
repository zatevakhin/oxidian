use std::collections::{BTreeSet, HashMap};

use crate::{FileKind, LinkTarget, VaultIndex, VaultPath};

#[derive(Debug, Clone)]
pub(crate) struct Resolver {
    by_rel: HashMap<String, VaultPath>,
    by_rel_lower: HashMap<String, VaultPath>,
    by_filename: HashMap<String, Vec<VaultPath>>,
    by_filename_lower: HashMap<String, Vec<VaultPath>>,
    by_stem: HashMap<String, Vec<VaultPath>>,
    by_stem_lower: HashMap<String, Vec<VaultPath>>,
    by_alias: HashMap<String, Vec<VaultPath>>,
    note_exts: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct LinkResolver {
    inner: Resolver,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveResult {
    Resolved(VaultPath),
    Ambiguous(Vec<VaultPath>),
    Missing,
}

impl Resolver {
    pub(crate) fn new(index: &VaultIndex) -> Self {
        let mut by_rel = HashMap::new();
        let mut by_rel_lower = HashMap::new();
        let mut by_filename: HashMap<String, Vec<VaultPath>> = HashMap::new();
        let mut by_filename_lower: HashMap<String, Vec<VaultPath>> = HashMap::new();
        let mut by_stem: HashMap<String, Vec<VaultPath>> = HashMap::new();
        let mut by_stem_lower: HashMap<String, Vec<VaultPath>> = HashMap::new();
        let mut by_alias: HashMap<String, Vec<VaultPath>> = HashMap::new();
        let mut note_exts: BTreeSet<String> = BTreeSet::new();

        for f in index.all_files() {
            let rel = f.path.as_str_lossy();
            by_rel.insert(rel.clone(), f.path.clone());
            by_rel_lower.insert(rel.to_lowercase(), f.path.clone());

            if let Some(name) = f.path.as_path().file_name().and_then(|s| s.to_str()) {
                by_filename
                    .entry(name.to_string())
                    .or_default()
                    .push(f.path.clone());
                by_filename_lower
                    .entry(name.to_lowercase())
                    .or_default()
                    .push(f.path.clone());
            }

            if matches!(f.kind, FileKind::Markdown | FileKind::Canvas) {
                if let Some(stem) = f.path.as_path().file_stem().and_then(|s| s.to_str()) {
                    by_stem
                        .entry(stem.to_string())
                        .or_default()
                        .push(f.path.clone());
                    by_stem_lower
                        .entry(stem.to_lowercase())
                        .or_default()
                        .push(f.path.clone());
                }
                if let Some(ext) = f.path.as_path().extension().and_then(|s| s.to_str()) {
                    note_exts.insert(ext.to_lowercase());
                }
            }
        }

        for (path, note) in index.notes_iter() {
            for a in &note.aliases {
                by_alias
                    .entry(a.to_lowercase())
                    .or_default()
                    .push(path.clone());
            }
        }

        Self {
            by_rel,
            by_rel_lower,
            by_filename,
            by_filename_lower,
            by_stem,
            by_stem_lower,
            by_alias,
            note_exts: note_exts.into_iter().collect(),
        }
    }

    pub(crate) fn resolve_link_target(
        &self,
        target: &LinkTarget,
        source: &VaultPath,
    ) -> ResolveResult {
        let LinkTarget::Internal { reference } = target else {
            return ResolveResult::Missing;
        };
        self.resolve_internal_with_source(reference, source)
    }

    pub(crate) fn resolve_internal_with_source(
        &self,
        reference: &str,
        source: &VaultPath,
    ) -> ResolveResult {
        let r0 = percent_decode(reference).unwrap_or_else(|| reference.to_string());
        let r = r0.trim();
        if r.is_empty() {
            return ResolveResult::Missing;
        }

        // Path-ish: contains a slash.
        if r.contains('/') {
            if let Some(p) = self.by_rel.get(r) {
                return ResolveResult::Resolved(p.clone());
            }
            if let Some(p) = self.by_rel_lower.get(&r.to_lowercase()) {
                return ResolveResult::Resolved(p.clone());
            }

            if !has_extension(r) {
                let mut candidates = Vec::new();
                for ext in &self.note_exts {
                    let cand = format!("{r}.{ext}");
                    if let Some(p) = self.by_rel.get(&cand) {
                        candidates.push(p.clone());
                    } else if let Some(p) = self.by_rel_lower.get(&cand.to_lowercase()) {
                        candidates.push(p.clone());
                    }
                }
                return pick(candidates);
            }

            return ResolveResult::Missing;
        }

        // If reference includes extension, treat it as a filename.
        if has_extension(r) {
            if let Some(v) = self.by_filename.get(r) {
                return pick_prefer_source(v.clone(), source);
            }
            if let Some(v) = self.by_filename_lower.get(&r.to_lowercase()) {
                return pick_prefer_source(v.clone(), source);
            }
            return ResolveResult::Missing;
        }

        // Otherwise: resolve by note stem or alias.
        let mut candidates: Vec<VaultPath> = Vec::new();
        if let Some(v) = self.by_stem.get(r) {
            candidates.extend(v.iter().cloned());
        }
        if let Some(v) = self.by_stem_lower.get(&r.to_lowercase()) {
            candidates.extend(v.iter().cloned());
        }
        if let Some(v) = self.by_alias.get(&r.to_lowercase()) {
            candidates.extend(v.iter().cloned());
        }
        if !candidates.is_empty() {
            return pick_prefer_source(candidates, source);
        }

        // Last resort: exact rel path match without extension.
        if let Some(p) = self.by_rel.get(r) {
            return ResolveResult::Resolved(p.clone());
        }
        if let Some(p) = self.by_rel_lower.get(&r.to_lowercase()) {
            return ResolveResult::Resolved(p.clone());
        }

        ResolveResult::Missing
    }
}

impl LinkResolver {
    pub fn new(index: &VaultIndex) -> Self {
        Self {
            inner: Resolver::new(index),
        }
    }

    pub fn resolve_internal(&self, reference: &str, source: &VaultPath) -> ResolveResult {
        self.inner.resolve_internal_with_source(reference, source)
    }

    pub fn resolve_link_target(&self, target: &LinkTarget, source: &VaultPath) -> ResolveResult {
        self.inner.resolve_link_target(target, source)
    }
}

fn pick(mut candidates: Vec<VaultPath>) -> ResolveResult {
    candidates.sort();
    candidates.dedup();
    match candidates.len() {
        0 => ResolveResult::Missing,
        1 => ResolveResult::Resolved(candidates.remove(0)),
        _ => ResolveResult::Ambiguous(candidates),
    }
}

fn pick_prefer_source(mut candidates: Vec<VaultPath>, source: &VaultPath) -> ResolveResult {
    candidates.sort();
    candidates.dedup();
    if candidates.len() <= 1 {
        return pick(candidates);
    }

    let src_dir = source
        .as_path()
        .parent()
        .unwrap_or(std::path::Path::new(""));
    let mut same_dir = Vec::new();
    for c in &candidates {
        let c_dir = c.as_path().parent().unwrap_or(std::path::Path::new(""));
        if c_dir == src_dir {
            same_dir.push(c.clone());
        }
    }
    if !same_dir.is_empty() {
        return pick_shortest_or_ambiguous(same_dir);
    }

    pick_shortest_or_ambiguous(candidates)
}

fn pick_shortest_or_ambiguous(mut candidates: Vec<VaultPath>) -> ResolveResult {
    candidates.sort();
    candidates.dedup();
    if candidates.is_empty() {
        return ResolveResult::Missing;
    }
    if candidates.len() == 1 {
        return ResolveResult::Resolved(candidates.remove(0));
    }

    let mut best_len: Option<usize> = None;
    let mut best: Vec<VaultPath> = Vec::new();
    for c in candidates {
        let len = c.as_str_lossy().len();
        match best_len {
            None => {
                best_len = Some(len);
                best.push(c);
            }
            Some(bl) if len < bl => {
                best_len = Some(len);
                best.clear();
                best.push(c);
            }
            Some(bl) if len == bl => best.push(c),
            _ => {}
        }
    }

    match best.len() {
        1 => ResolveResult::Resolved(best.remove(0)),
        _ => ResolveResult::Ambiguous(best),
    }
}

fn has_extension(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, ext)| !ext.is_empty())
}

fn percent_decode(s: &str) -> Option<String> {
    if !s.contains('%') && !s.contains('\\') {
        return None;
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let h1 = bytes[i + 1];
            let h2 = bytes[i + 2];
            if let (Some(a), Some(b)) = (from_hex(h1), from_hex(h2)) {
                out.push((a << 4) | b);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'\\' {
            out.push(b'/');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8(out).ok()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(10 + (b - b'a')),
        b'A'..=b'F' => Some(10 + (b - b'A')),
        _ => None,
    }
}
