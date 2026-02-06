use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::time::SystemTime;

use nucleo::{
    pattern::{CaseMatching, Normalization, Pattern},
    Matcher, Utf32Str,
};

use crate::fields::{
    extract_top_level_frontmatter_fields, inline_value_to_field_value, merge_field,
    normalize_field_key, FieldMap,
};
use crate::parse::{parse_markdown_note, FrontmatterParse};
use crate::{BacklinksIndex, Error, Query, QueryHit, Result, Vault, VaultPath};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Markdown,
    Canvas,
    Attachment,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tag(pub String);

pub use crate::links::LinkTarget;

#[derive(Debug, Clone)]
pub struct FileMeta {
    pub path: VaultPath,
    pub kind: FileKind,
    pub mtime: SystemTime,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct NoteMeta {
    pub file: FileMeta,
    pub title: String,
    pub aliases: BTreeSet<String>,
    pub tags: BTreeSet<Tag>,
    pub links: BTreeSet<LinkTarget>,
    pub link_occurrences: Vec<crate::Link>,
    pub frontmatter: FrontmatterStatus,
    pub fields: FieldMap,
    pub tasks: Vec<Task>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TaskStatus {
    Todo,
    Done,
    InProgress,
    Cancelled,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub path: VaultPath,
    /// 1-based line number in the file.
    pub line: u32,
    pub status: TaskStatus,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrontmatterStatus {
    None,
    Valid,
    Broken { error: String },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrontmatterReport {
    pub none: usize,
    pub valid: usize,
    pub broken: usize,
}

#[derive(Debug, Clone, Default)]
pub struct VaultIndex {
    files: HashMap<VaultPath, FileMeta>,
    notes: HashMap<VaultPath, NoteMeta>,
    file_tags: HashMap<VaultPath, BTreeSet<Tag>>,
    file_links: HashMap<VaultPath, BTreeSet<LinkTarget>>,
    tags: HashMap<Tag, BTreeSet<VaultPath>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndexDelta {
    pub added_tags: BTreeSet<Tag>,
    pub removed_tags: BTreeSet<Tag>,
    pub added_links: BTreeSet<LinkTarget>,
    pub removed_links: BTreeSet<LinkTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub path: VaultPath,
    pub score: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentSearchHit {
    pub path: VaultPath,
    pub score: u32,
    /// 1-based line number.
    pub line: u32,
    pub line_text: String,
}

impl VaultIndex {
    pub fn build(vault: &Vault) -> Result<Self> {
        let mut idx = Self::default();
        for entry in walkdir::WalkDir::new(vault.root())
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let abs = entry.path();
            let rel = match vault.to_rel(abs) {
                Ok(r) => r,
                Err(_) => continue,
            };
            if !vault.is_indexable_rel(rel.as_path()) {
                continue;
            }
            idx.upsert_path(vault, rel)?;
        }
        Ok(idx)
    }

    pub fn upsert_path(&mut self, vault: &Vault, rel: VaultPath) -> Result<IndexDelta> {
        if !vault.is_indexable_rel(rel.as_path()) {
            return Ok(IndexDelta::default());
        }

        let abs = vault.to_abs(&rel);
        let meta = std::fs::metadata(&abs).map_err(|e| Error::io(&abs, e))?;
        if !meta.is_file() {
            return Ok(IndexDelta::default());
        }
        let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let size = meta.len();
        let kind = file_kind_from_path(vault, rel.as_path());

        let file = FileMeta {
            path: rel.clone(),
            kind,
            mtime,
            size,
        };
        self.files.insert(rel.clone(), file.clone());

        let (new_tags, new_links, note_meta) = match kind {
            FileKind::Markdown | FileKind::Canvas => {
                let content = std::fs::read_to_string(&abs).map_err(|e| Error::io(&abs, e))?;
                let parsed = parse_markdown_note(&rel, &content);
                let mut fields = FieldMap::new();
                let mut aliases = BTreeSet::new();
                let frontmatter = match &parsed.frontmatter {
                    FrontmatterParse::None => FrontmatterStatus::None,
                    FrontmatterParse::Valid(fm) => {
                        if let Ok(fm_fields) = extract_top_level_frontmatter_fields(fm) {
                            for (k, v) in fm_fields {
                                merge_field(&mut fields, k, v);
                            }
                        }
                        aliases = extract_frontmatter_aliases(fm);
                        FrontmatterStatus::Valid
                    }
                    FrontmatterParse::Broken { error } => FrontmatterStatus::Broken {
                        error: error.clone(),
                    },
                };

                for (k_raw, v_raw) in parsed.inline_fields {
                    let Some(k) = normalize_field_key(&k_raw) else {
                        continue;
                    };
                    let v = inline_value_to_field_value(&v_raw);
                    merge_field(&mut fields, k, v);
                }

                let tasks = parsed
                    .tasks
                    .into_iter()
                    .map(|t| Task {
                        path: rel.clone(),
                        line: t.line,
                        status: t.status,
                        text: t.text,
                    })
                    .collect();

                let note_meta = NoteMeta {
                    file,
                    title: parsed.title,
                    aliases,
                    tags: parsed.tags.clone(),
                    links: parsed.links.clone(),
                    link_occurrences: parsed.link_occurrences,
                    frontmatter,
                    fields,
                    tasks,
                };
                (parsed.tags, parsed.links, Some(note_meta))
            }
            _ => (BTreeSet::new(), BTreeSet::new(), None),
        };

        let old_tags = self.file_tags.insert(rel.clone(), new_tags.clone());
        let old_links = self.file_links.insert(rel.clone(), new_links.clone());

        if let Some(note) = note_meta {
            self.notes.insert(rel.clone(), note);
        } else {
            self.notes.remove(&rel);
        }

        let delta = self.reconcile_tag_index(&rel, old_tags, &new_tags);
        let delta = self.reconcile_link_index(delta, &rel, old_links, &new_links);
        Ok(delta)
    }

    pub fn remove_path(&mut self, rel: &VaultPath) -> IndexDelta {
        self.files.remove(rel);
        self.notes.remove(rel);

        let old_tags = self.file_tags.remove(rel).unwrap_or_default();
        let old_links = self.file_links.remove(rel).unwrap_or_default();

        for tag in &old_tags {
            if let Some(set) = self.tags.get_mut(tag) {
                set.remove(rel);
                if set.is_empty() {
                    self.tags.remove(tag);
                }
            }
        }

        IndexDelta {
            added_tags: BTreeSet::new(),
            removed_tags: old_tags,
            added_links: BTreeSet::new(),
            removed_links: old_links,
        }
    }

    pub fn note(&self, path: &VaultPath) -> Option<&NoteMeta> {
        self.notes.get(path)
    }

    pub fn file(&self, path: &VaultPath) -> Option<&FileMeta> {
        self.files.get(path)
    }

    pub fn all_files(&self) -> impl Iterator<Item = &FileMeta> {
        self.files.values()
    }

    pub fn all_tags(&self) -> impl Iterator<Item = &Tag> {
        self.tags.keys()
    }

    pub fn files_with_tag(&self, tag: &Tag) -> impl Iterator<Item = &VaultPath> {
        self.tags.get(tag).into_iter().flat_map(|s| s.iter())
    }

    pub fn outgoing_links(&self, from: &VaultPath) -> impl Iterator<Item = &LinkTarget> {
        self.file_links.get(from).into_iter().flat_map(|s| s.iter())
    }

    pub fn query(&self, q: &Query) -> Vec<QueryHit> {
        q.execute(self)
    }

    pub fn query_tasks(&self, q: &crate::TaskQuery) -> Vec<crate::TaskHit> {
        q.execute(self)
    }

    pub(crate) fn notes_iter_paths(&self) -> impl Iterator<Item = &VaultPath> {
        self.notes.keys()
    }

    pub(crate) fn notes_iter(&self) -> impl Iterator<Item = (&VaultPath, &NoteMeta)> {
        self.notes.iter()
    }

    pub fn notes_with_frontmatter(&self) -> impl Iterator<Item = &VaultPath> {
        self.notes
            .iter()
            .filter(|(_, n)| !matches!(n.frontmatter, FrontmatterStatus::None))
            .map(|(p, _)| p)
    }

    pub fn notes_without_frontmatter(&self) -> impl Iterator<Item = &VaultPath> {
        self.notes
            .iter()
            .filter(|(_, n)| matches!(n.frontmatter, FrontmatterStatus::None))
            .map(|(p, _)| p)
    }

    pub fn notes_with_broken_frontmatter(&self) -> impl Iterator<Item = (&VaultPath, &str)> {
        self.notes.iter().filter_map(|(p, n)| match &n.frontmatter {
            FrontmatterStatus::Broken { error } => Some((p, error.as_str())),
            _ => None,
        })
    }

    pub fn frontmatter_report(&self) -> FrontmatterReport {
        let mut r = FrontmatterReport::default();
        for note in self.notes.values() {
            match &note.frontmatter {
                FrontmatterStatus::None => r.none += 1,
                FrontmatterStatus::Valid => r.valid += 1,
                FrontmatterStatus::Broken { .. } => r.broken += 1,
            }
        }
        r
    }

    pub fn note_tasks(&self, path: &VaultPath) -> Option<&[Task]> {
        self.notes.get(path).map(|n| n.tasks.as_slice())
    }

    pub fn all_tasks(&self) -> impl Iterator<Item = &Task> {
        self.notes.values().flat_map(|n| n.tasks.iter())
    }

    pub fn link_health_report(&self, vault: &Vault) -> Result<crate::LinkHealthReport> {
        crate::link_health::link_health_report(self, vault)
    }

    pub fn link_resolver(&self) -> crate::LinkResolver {
        crate::LinkResolver::new(self)
    }

    pub fn build_backlinks(&self, _vault: &Vault) -> Result<BacklinksIndex> {
        Ok(self.build_graph(_vault)?.backlinks)
    }

    pub fn build_graph(&self, _vault: &Vault) -> Result<crate::GraphIndex> {
        Ok(crate::graph::build_graph(self))
    }

    #[cfg(feature = "similarity")]
    pub fn note_similarity_report(&self, vault: &Vault) -> Result<crate::NoteSimilarityReport> {
        crate::similarity::note_similarity_report(self, vault)
    }

    #[cfg(feature = "similarity")]
    pub fn note_similarity_for(
        &self,
        vault: &Vault,
        source: &VaultPath,
    ) -> Result<Vec<crate::NoteSimilarityHit>> {
        crate::similarity::note_similarity_for(self, vault, source)
    }

    pub fn resolved_outgoing_internal_links(
        &self,
        source: &VaultPath,
    ) -> Vec<crate::ResolvedInternalLink> {
        let Some(note) = self.note(source) else {
            return Vec::new();
        };
        let resolver = self.link_resolver();
        note.link_occurrences
            .iter()
            .filter(|l| matches!(l.target, crate::LinkTarget::Internal { .. }))
            .map(|l| crate::ResolvedInternalLink {
                source: source.clone(),
                link: l.clone(),
                resolution: resolver.resolve_link_target(&l.target, source),
            })
            .collect()
    }

    /// Fuzzy-search by relative path (including directories in the string).
    pub fn search_filenames_fuzzy(&self, query: &str, limit: usize) -> Vec<SearchHit> {
        let q = query.trim();
        if q.is_empty() || limit == 0 {
            return Vec::new();
        }

        let pattern = Pattern::parse(q, CaseMatching::Smart, Normalization::Smart);
        let mut matcher = Matcher::new(nucleo::Config::DEFAULT);
        let mut utf32_buf = Vec::new();
        let mut hits = Vec::new();

        for p in self.files.keys() {
            let s = p.as_str_lossy();
            if let Some(score) = pattern.score(Utf32Str::new(&s, &mut utf32_buf), &mut matcher) {
                hits.push(SearchHit {
                    path: p.clone(),
                    score,
                });
            }
        }

        hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
        hits.truncate(limit);
        hits
    }

    /// Fuzzy-search note content by scanning non-empty lines and taking the best match per note.
    ///
    /// This reads note files from disk and can be expensive; prefer calling it from a
    /// `spawn_blocking` context.
    pub fn search_content_fuzzy(
        &self,
        vault: &Vault,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ContentSearchHit>> {
        let q = query.trim();
        if q.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let pattern = Pattern::parse(q, CaseMatching::Smart, Normalization::Smart);
        let mut matcher = Matcher::new(nucleo::Config::DEFAULT);
        let mut utf32_buf = Vec::new();
        let mut hits: Vec<ContentSearchHit> = Vec::new();

        for (path, file) in &self.files {
            if !matches!(file.kind, FileKind::Markdown | FileKind::Canvas) {
                continue;
            }

            let abs = vault.to_abs(path);
            let text = std::fs::read_to_string(&abs).map_err(|e| Error::io(&abs, e))?;
            let mut best: Option<(u32, u32, String)> = None;
            for (ix, line) in text.lines().enumerate() {
                let lt = line.trim();
                if lt.is_empty() {
                    continue;
                }
                if let Some(score) = pattern.score(Utf32Str::new(lt, &mut utf32_buf), &mut matcher)
                {
                    let line_no = (ix + 1) as u32;
                    match &best {
                        None => best = Some((score, line_no, line.to_string())),
                        Some((b, _, _)) if score > *b => {
                            best = Some((score, line_no, line.to_string()))
                        }
                        _ => {}
                    }
                }
            }

            if let Some((score, line_no, line_text)) = best {
                hits.push(ContentSearchHit {
                    path: path.clone(),
                    score,
                    line: line_no,
                    line_text,
                });
            }
        }

        hits.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.path.cmp(&b.path))
                .then_with(|| a.line.cmp(&b.line))
        });
        hits.truncate(limit);
        Ok(hits)
    }

    fn reconcile_tag_index(
        &mut self,
        rel: &VaultPath,
        old: Option<BTreeSet<Tag>>,
        new: &BTreeSet<Tag>,
    ) -> IndexDelta {
        let old = old.unwrap_or_default();
        let old_set: HashSet<_> = old.iter().cloned().collect();
        let new_set: HashSet<_> = new.iter().cloned().collect();

        let added: BTreeSet<_> = new_set.difference(&old_set).cloned().collect();
        let removed: BTreeSet<_> = old_set.difference(&new_set).cloned().collect();

        for tag in &added {
            self.tags
                .entry(tag.clone())
                .or_default()
                .insert(rel.clone());
        }
        for tag in &removed {
            if let Some(set) = self.tags.get_mut(tag) {
                set.remove(rel);
                if set.is_empty() {
                    self.tags.remove(tag);
                }
            }
        }

        IndexDelta {
            added_tags: added,
            removed_tags: removed,
            ..Default::default()
        }
    }

    fn reconcile_link_index(
        &mut self,
        mut delta: IndexDelta,
        rel: &VaultPath,
        old: Option<BTreeSet<LinkTarget>>,
        new: &BTreeSet<LinkTarget>,
    ) -> IndexDelta {
        let old = old.unwrap_or_default();
        let old_set: HashSet<_> = old.iter().cloned().collect();
        let new_set: HashSet<_> = new.iter().cloned().collect();

        delta.added_links = new_set.difference(&old_set).cloned().collect();
        delta.removed_links = old_set.difference(&new_set).cloned().collect();
        if delta.added_links.is_empty() && delta.removed_links.is_empty() {
            // avoid unused params warnings; keep signature consistent
            let _ = rel;
        }
        delta
    }
}

fn file_kind_from_path(vault: &Vault, rel: &Path) -> FileKind {
    let ext = rel.extension().and_then(|s| s.to_str()).unwrap_or("");
    let ext = ext.to_lowercase();

    if vault
        .config()
        .note_extensions
        .iter()
        .any(|e| e.eq_ignore_ascii_case(&ext))
    {
        return match ext.as_str() {
            "md" => FileKind::Markdown,
            "canvas" => FileKind::Canvas,
            _ => FileKind::Other,
        };
    }

    if vault
        .config()
        .attachment_extensions
        .iter()
        .any(|e| e.eq_ignore_ascii_case(&ext))
    {
        return FileKind::Attachment;
    }

    FileKind::Other
}

fn extract_frontmatter_aliases(fm: &serde_yaml::Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Some(map) = fm.as_mapping() else {
        return out;
    };

    for key in ["aliases", "alias"] {
        let Some(v) = map.get(serde_yaml::Value::String(key.into())) else {
            continue;
        };
        match v {
            serde_yaml::Value::Sequence(seq) => {
                for item in seq {
                    if let Some(s) = item.as_str() {
                        let s = s.trim();
                        if s.is_empty() {
                            continue;
                        }
                        out.insert(s.to_lowercase());
                    }
                }
            }
            serde_yaml::Value::String(s) => {
                let s = s.trim();
                if !s.is_empty() {
                    out.insert(s.to_lowercase());
                }
            }
            _ => {}
        }
    }

    out
}
