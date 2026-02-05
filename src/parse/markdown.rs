use std::collections::BTreeSet;

use crate::{Link, LinkKind, LinkLocation, LinkTarget, Subpath, Tag, VaultPath};

#[derive(Debug, Clone)]
pub(crate) struct ParsedNote {
    pub title: String,
    pub tags: BTreeSet<Tag>,
    pub links: BTreeSet<LinkTarget>,
    pub link_occurrences: Vec<Link>,
    pub frontmatter: FrontmatterParse,
    pub inline_fields: Vec<(String, String)>,
    pub tasks: Vec<ParsedTask>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedTask {
    pub line: u32,
    pub status: crate::TaskStatus,
    pub text: String,
}

#[derive(Debug, Clone)]
pub(crate) enum FrontmatterParse {
    None,
    Valid(serde_yaml::Value),
    Broken { error: String },
}

pub(crate) fn parse_markdown_note(path: &VaultPath, content: &str) -> ParsedNote {
    let (frontmatter, body, body_start_line) = split_frontmatter(content);
    let mut tags = BTreeSet::new();
    if let FrontmatterParse::Valid(fm) = &frontmatter {
        tags.extend(extract_frontmatter_tags(fm));
    }
    let (inline_tags, links, link_occurrences, inline_fields, tasks) =
        extract_inline_tags_links_fields(body, body_start_line);
    tags.extend(inline_tags);

    let title = extract_title(
        path,
        match &frontmatter {
            FrontmatterParse::Valid(v) => Some(v),
            _ => None,
        },
        body,
    );

    ParsedNote {
        title,
        tags,
        links,
        link_occurrences,
        frontmatter,
        inline_fields,
        tasks,
    }
}

fn split_frontmatter(content: &str) -> (FrontmatterParse, &str, u32) {
    let trimmed = content;
    let Some(rest) = trimmed
        .strip_prefix("---\n")
        .or_else(|| trimmed.strip_prefix("---\r\n"))
    else {
        return (FrontmatterParse::None, content, 1);
    };

    // Find a closing fence on its own line.
    // We accept either "---\n" or "---\r\n".
    let mut idx = 0usize;
    let bytes = rest.as_bytes();
    while idx < bytes.len() {
        // Find end of line
        let line_end = match memchr::memchr(b'\n', &bytes[idx..]) {
            Some(off) => idx + off + 1,
            None => bytes.len(),
        };
        let line = &rest[idx..line_end];
        let line_trim = line.trim_end_matches(['\r', '\n']);
        if line_trim == "---" {
            let fm_text = &rest[..idx];
            let body = &rest[line_end..];
            let start_line = 1 + count_newlines(&content[..content.len() - body.len()]) as u32;
            match serde_yaml::from_str::<serde_yaml::Value>(fm_text) {
                Ok(v) => return (FrontmatterParse::Valid(v), body, start_line),
                Err(err) => {
                    return (
                        FrontmatterParse::Broken {
                            error: err.to_string(),
                        },
                        body,
                        start_line,
                    );
                }
            }
        }
        idx = line_end;
    }

    (
        FrontmatterParse::Broken {
            error: "frontmatter fence not closed".to_string(),
        },
        content,
        1,
    )
}

fn extract_frontmatter_tags(fm: &serde_yaml::Value) -> BTreeSet<Tag> {
    let mut out = BTreeSet::new();
    let Some(map) = fm.as_mapping() else {
        return out;
    };

    for key in ["tags", "tag"] {
        let Some(v) = map.get(serde_yaml::Value::String(key.into())) else {
            continue;
        };
        out.extend(extract_tags_from_yaml_value(v));
    }

    out
}

fn extract_tags_from_yaml_value(v: &serde_yaml::Value) -> BTreeSet<Tag> {
    let mut out = BTreeSet::new();
    match v {
        serde_yaml::Value::Sequence(seq) => {
            for item in seq {
                if let Some(s) = item.as_str() {
                    if let Some(tag) = normalize_tag(s) {
                        out.insert(tag);
                    }
                }
            }
        }
        serde_yaml::Value::String(s) => {
            for part in s
                .split(|c: char| c.is_whitespace() || c == ',')
                .filter(|p| !p.is_empty())
            {
                if let Some(tag) = normalize_tag(part) {
                    out.insert(tag);
                }
            }
        }
        _ => {}
    }
    out
}

fn extract_title(path: &VaultPath, fm: Option<&serde_yaml::Value>, body: &str) -> String {
    if let Some(fm) = fm {
        if let Some(map) = fm.as_mapping() {
            if let Some(v) = map.get(serde_yaml::Value::String("title".into())) {
                if let Some(s) = v.as_str() {
                    let s = s.trim();
                    if !s.is_empty() {
                        return s.to_string();
                    }
                }
            }
        }
    }

    // First H1.
    let mut in_fenced = false;
    for line in body.lines() {
        if is_fence(line) {
            in_fenced = !in_fenced;
            continue;
        }
        if in_fenced {
            continue;
        }
        if let Some(h) = line.strip_prefix("# ") {
            let h = h.trim();
            if !h.is_empty() {
                return h.to_string();
            }
        }
    }

    // Fallback: filename stem.
    path.as_path()
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("untitled")
        .to_string()
}

fn extract_inline_tags_links_fields(
    body: &str,
    body_start_line: u32,
) -> (
    BTreeSet<Tag>,
    BTreeSet<LinkTarget>,
    Vec<Link>,
    Vec<(String, String)>,
    Vec<ParsedTask>,
) {
    let mut tags = BTreeSet::new();
    let mut links = BTreeSet::new();
    let mut link_occurrences = Vec::new();
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut tasks: Vec<ParsedTask> = Vec::new();
    let mut in_fenced = false;

    for (line_ix, line) in body.lines().enumerate() {
        if is_fence(line) {
            in_fenced = !in_fenced;
            continue;
        }
        if in_fenced {
            continue;
        }

        tags.extend(extract_inline_tags_from_line(line));
        let (targets, occs) = extract_links_from_line(line, body_start_line + line_ix as u32);
        links.extend(targets);
        link_occurrences.extend(occs);
        fields.extend(extract_inline_fields_from_line(line));

        if let Some((status, text)) = parse_task_line(line) {
            tasks.push(ParsedTask {
                line: body_start_line + line_ix as u32,
                status,
                text,
            });
        }
    }

    (tags, links, link_occurrences, fields, tasks)
}

fn parse_task_line(line: &str) -> Option<(crate::TaskStatus, String)> {
    let s = line.trim_start();
    let mut rest = s;

    // Unordered list
    if let Some(r) = rest
        .strip_prefix("- ")
        .or_else(|| rest.strip_prefix("* "))
        .or_else(|| rest.strip_prefix("+ "))
    {
        rest = r;
    } else {
        // Ordered list: "1. " or "1) "
        let bytes = rest.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
            i += 1;
        }
        if i == 0 {
            return None;
        }
        if i + 1 >= bytes.len() {
            return None;
        }
        let punct = bytes[i] as char;
        if punct != '.' && punct != ')' {
            return None;
        }
        if bytes[i + 1] != b' ' {
            return None;
        }
        rest = &rest[i + 2..];
    }

    let bytes = rest.as_bytes();
    if bytes.len() < 3 || bytes[0] != b'[' || bytes[2] != b']' {
        return None;
    }
    let mark = bytes[1] as char;
    let status = match mark {
        ' ' => crate::TaskStatus::Todo,
        'x' | 'X' => crate::TaskStatus::Done,
        '>' => crate::TaskStatus::InProgress,
        '-' => crate::TaskStatus::Cancelled,
        '?' => crate::TaskStatus::Blocked,
        _ => return None,
    };
    let text = rest[3..].trim_start();
    Some((status, text.to_string()))
}

fn count_newlines(s: &str) -> usize {
    s.bytes().filter(|b| *b == b'\n').count()
}

fn extract_inline_fields_from_line(line: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    out.extend(extract_bracketed_fields(line));
    out.extend(extract_bare_fields(line));
    out
}

fn extract_bracketed_fields(line: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'[' {
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut j = start;
        while j < bytes.len() && bytes[j] != b']' {
            j += 1;
        }
        if j >= bytes.len() {
            break;
        }
        let inner = &line[start..j];
        if let Some((k, v)) = parse_field_kv(inner) {
            out.push((k, v));
        }
        i = j + 1;
    }
    out
}

fn extract_bare_fields(line: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();

    // Avoid extracting fields from bracketed segments: skip any :: that falls inside [..].
    let bracket_ranges = bracket_ranges(line);

    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] != b':' || bytes[i + 1] != b':' {
            i += 1;
            continue;
        }
        if bracket_ranges.iter().any(|(s, e)| i >= *s && i < *e) {
            i += 2;
            continue;
        }

        let key_end = i;
        let mut ks = key_end;
        while ks > 0 {
            let c = bytes[ks - 1] as char;
            if is_field_key_char(c) {
                ks -= 1;
            } else {
                break;
            }
        }
        if ks == key_end {
            i += 2;
            continue;
        }

        let key = line[ks..key_end].trim();
        if key.is_empty() {
            i += 2;
            continue;
        }

        let value = line[i + 2..].trim();
        if value.is_empty() {
            i += 2;
            continue;
        }

        out.push((key.to_string(), value.to_string()));
        break;
    }

    out
}

fn bracket_ranges(line: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'[' {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        while i < bytes.len() && bytes[i] != b']' {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b']' {
            out.push((start, i + 1));
            i += 1;
        }
    }
    out
}

fn parse_field_kv(inner: &str) -> Option<(String, String)> {
    let (k, v) = inner.split_once("::")?;
    let k = k.trim();
    let v = v.trim();
    if k.is_empty() || v.is_empty() {
        return None;
    }
    Some((k.to_string(), v.to_string()))
}

fn is_field_key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '/' | '.')
}

fn is_fence(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```")
}

fn extract_inline_tags_from_line(line: &str) -> BTreeSet<Tag> {
    let mut out = BTreeSet::new();

    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'#' {
            i += 1;
            continue;
        }

        // Avoid headings like "# Title".
        if i + 1 < bytes.len() && bytes[i + 1] == b' ' {
            i += 1;
            continue;
        }

        // Require a boundary before '#'.
        if i > 0 {
            let prev = bytes[i - 1] as char;
            if prev.is_alphanumeric() || prev == '/' {
                i += 1;
                continue;
            }
        }

        let mut j = i + 1;
        while j < bytes.len() {
            let c = bytes[j] as char;
            if is_tag_char(c) {
                j += 1;
            } else {
                break;
            }
        }

        if j > i + 1 {
            let raw = &line[i + 1..j];
            if let Some(tag) = normalize_tag(raw) {
                out.insert(tag);
            }
        }

        i = j.max(i + 1);
    }

    out
}

fn extract_links_from_line(line: &str, line_no: u32) -> (BTreeSet<LinkTarget>, Vec<Link>) {
    let mut targets = BTreeSet::new();
    let mut occs = Vec::new();

    let (wiki_targets, wiki_occs) = extract_wikilinks_and_embeds(line, line_no);
    targets.extend(wiki_targets);
    occs.extend(wiki_occs);

    let (md_targets, md_occs) = extract_markdown_links_and_embeds(line, line_no);
    targets.extend(md_targets);
    occs.extend(md_occs);

    let (auto_targets, auto_occs) = extract_autourls(line, line_no);
    targets.extend(auto_targets);
    occs.extend(auto_occs);

    (targets, occs)
}

fn extract_wikilinks_and_embeds(line: &str, line_no: u32) -> (BTreeSet<LinkTarget>, Vec<Link>) {
    let mut targets = BTreeSet::new();
    let mut occs = Vec::new();

    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        let mut embed = false;
        let start = if bytes[i] == b'!'
            && i + 2 < bytes.len()
            && bytes[i + 1] == b'['
            && bytes[i + 2] == b'['
        {
            embed = true;
            i + 1
        } else {
            i
        };

        if bytes[start] == b'[' && bytes[start + 1] == b'[' {
            let mut j = start + 2;
            while j + 1 < bytes.len() {
                if bytes[j] == b']' && bytes[j + 1] == b']' {
                    let inner = &line[start + 2..j];
                    let raw = inner.to_string();
                    if let Some((target, subpath, display)) = normalize_wikilink_components(inner) {
                        targets.insert(target.clone());
                        occs.push(Link {
                            kind: LinkKind::Wiki,
                            embed,
                            display,
                            target,
                            subpath,
                            location: LinkLocation {
                                line: line_no,
                                column: (start + 1) as u32,
                            },
                            raw,
                        });
                    }
                    i = j + 2;
                    break;
                }
                j += 1;
            }
            if j + 1 >= bytes.len() {
                break;
            }
            continue;
        }

        i += 1;
    }

    (targets, occs)
}

fn extract_markdown_links_and_embeds(
    line: &str,
    line_no: u32,
) -> (BTreeSet<LinkTarget>, Vec<Link>) {
    let mut targets = BTreeSet::new();
    let mut occs = Vec::new();

    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let mut embed = false;
        let start = if bytes[i] == b'!' {
            embed = true;
            i + 1
        } else {
            i
        };
        if start >= bytes.len() || bytes[start] != b'[' {
            i += 1;
            continue;
        }
        // Find closing ']'
        let mut j = start + 1;
        while j < bytes.len() && bytes[j] != b']' {
            j += 1;
        }
        if j >= bytes.len() || j + 1 >= bytes.len() || bytes[j + 1] != b'(' {
            i += 1;
            continue;
        }
        let display = &line[start + 1..j];

        // Find closing ')'
        let mut k = j + 2;
        while k < bytes.len() && bytes[k] != b')' {
            k += 1;
        }
        if k >= bytes.len() {
            break;
        }
        let raw = line[j + 2..k].to_string();
        if let Some((target, subpath)) = normalize_markdown_target(&raw) {
            targets.insert(target.clone());
            occs.push(Link {
                kind: match &target {
                    LinkTarget::ObsidianUri { .. } => LinkKind::ObsidianUri,
                    _ => LinkKind::Markdown,
                },
                embed,
                display: if display.trim().is_empty() {
                    None
                } else {
                    Some(display.to_string())
                },
                target,
                subpath,
                location: LinkLocation {
                    line: line_no,
                    column: (start + 1) as u32,
                },
                raw,
            });
        }

        i = k + 1;
    }

    (targets, occs)
}

fn extract_autourls(line: &str, line_no: u32) -> (BTreeSet<LinkTarget>, Vec<Link>) {
    let mut targets = BTreeSet::new();
    let mut occs = Vec::new();

    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        let mut j = i;
        while j < bytes.len() && bytes[j] != b'>' {
            j += 1;
        }
        if j >= bytes.len() {
            break;
        }
        let inner = line[i..j].trim();
        if inner.starts_with("http://")
            || inner.starts_with("https://")
            || inner.starts_with("mailto:")
        {
            let target = LinkTarget::ExternalUrl(inner.to_string());
            targets.insert(target.clone());
            occs.push(Link {
                kind: LinkKind::AutoUrl,
                embed: false,
                display: None,
                target,
                subpath: None,
                location: LinkLocation {
                    line: line_no,
                    column: (start + 1) as u32,
                },
                raw: inner.to_string(),
            });
        }
        i = j + 1;
    }

    (targets, occs)
}

fn normalize_wikilink_components(
    raw: &str,
) -> Option<(LinkTarget, Option<Subpath>, Option<String>)> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }

    let (before_alias, display) = if let Some((left, right)) = s.split_once('|') {
        (
            left.trim(),
            Some(right.trim().to_string()).filter(|v| !v.is_empty()),
        )
    } else {
        (s, None)
    };

    // Subpath: prefer block if present, else heading.
    let (target_raw, subpath) = if let Some((left, right)) = before_alias.split_once('^') {
        (
            left.trim(),
            Some(Subpath::Block(right.trim().to_string())).filter(|sp| match sp {
                Subpath::Block(b) => !b.is_empty(),
                _ => true,
            }),
        )
    } else if let Some((left, right)) = before_alias.split_once('#') {
        (
            left.trim(),
            Some(Subpath::Heading(right.trim().to_string())).filter(|sp| match sp {
                Subpath::Heading(h) => !h.is_empty(),
                _ => true,
            }),
        )
    } else {
        (before_alias, None)
    };

    let target_raw = target_raw.trim();
    if target_raw.is_empty() {
        return None;
    }

    Some((
        LinkTarget::Internal {
            reference: target_raw.to_string(),
        },
        subpath,
        display,
    ))
}

fn normalize_markdown_target(raw: &str) -> Option<(LinkTarget, Option<Subpath>)> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }

    if s.starts_with("obsidian://") {
        return Some((LinkTarget::ObsidianUri { raw: s.to_string() }, None));
    }
    if s.starts_with("http://") || s.starts_with("https://") || s.starts_with("mailto:") {
        return Some((LinkTarget::ExternalUrl(s.to_string()), None));
    }

    // Split off heading fragment for internal paths.
    if let Some((left, right)) = s.split_once('#') {
        let left = left.trim();
        let right = right.trim();
        if left.is_empty() {
            return None;
        }
        return Some((
            LinkTarget::Internal {
                reference: left.to_string(),
            },
            Some(Subpath::Heading(right.to_string())).filter(|sp| match sp {
                Subpath::Heading(h) => !h.is_empty(),
                _ => true,
            }),
        ));
    }

    Some((
        LinkTarget::Internal {
            reference: s.to_string(),
        },
        None,
    ))
}

fn is_tag_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '/')
}

fn normalize_tag(raw: &str) -> Option<Tag> {
    let mut s = raw.trim();
    if let Some(rest) = s.strip_prefix('#') {
        s = rest;
    }
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let s = s.trim_matches('/');
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    // Lowercase for stable, case-insensitive queries.
    Some(Tag(s.to_lowercase()))
}

// We only use memchr for fast line scanning in frontmatter.
mod memchr {
    pub(crate) fn memchr(needle: u8, haystack: &[u8]) -> Option<usize> {
        haystack.iter().position(|b| *b == needle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(path: &str, content: &str) -> ParsedNote {
        let path = VaultPath::try_from(std::path::Path::new(path)).unwrap();
        parse_markdown_note(&path, content)
    }

    #[test]
    fn frontmatter_tags_and_inline_tags_are_collected() {
        let note = parse(
            "notes/a.md",
            "---\ntitle: Hello\ntags: [Foo, bar/baz]\n---\n\n# Heading\nBody #Quux\n",
        );
        let tags: Vec<_> = note.tags.iter().map(|t| t.0.as_str()).collect();
        assert!(tags.contains(&"foo"));
        assert!(tags.contains(&"bar/baz"));
        assert!(tags.contains(&"quux"));
        assert_eq!(note.title, "Hello");
    }

    #[test]
    fn fenced_code_blocks_are_ignored() {
        let note = parse(
            "notes/a.md",
            "Here is code:\n```\n#notatag\n[[notalink]]\n```\nBut here is #tag and [[link]].\n",
        );
        assert!(note.tags.contains(&Tag("tag".into())));
        assert!(!note.tags.contains(&Tag("notatag".into())));
        assert!(note.links.contains(&LinkTarget::Internal {
            reference: "link".into()
        }));
        assert!(!note.links.contains(&LinkTarget::Internal {
            reference: "notalink".into()
        }));
    }

    #[test]
    fn headings_are_not_tags() {
        let note = parse("a.md", "# Title\n## Subtitle\n#tag\n");
        assert!(!note.tags.contains(&Tag("title".into())));
        assert!(note.tags.contains(&Tag("tag".into())));
    }

    #[test]
    fn wikilink_alias_and_heading_are_stripped() {
        let note = parse("a.md", "See [[Target|Alias]] and [[Other#Section]].");
        assert!(note.links.contains(&LinkTarget::Internal {
            reference: "Target".into()
        }));
        assert!(note.links.contains(&LinkTarget::Internal {
            reference: "Other".into()
        }));
        assert!(!note.links.contains(&LinkTarget::Internal {
            reference: "Alias".into()
        }));
    }

    #[test]
    fn inline_fields_support_bare_and_bracketed_variants() {
        let note = parse(
            "a.md",
            "x::y\n\
             x:: y\n\
             - [a::b]\n\
             - [c:: d]\n",
        );
        assert!(note.inline_fields.contains(&("x".into(), "y".into())));
        assert!(note.inline_fields.contains(&("x".into(), "y".into())));
        assert!(note.inline_fields.contains(&("a".into(), "b".into())));
        assert!(note.inline_fields.contains(&("c".into(), "d".into())));
    }

    #[test]
    fn inline_fields_ignore_fenced_code_blocks() {
        let note = parse("a.md", "```\nstatus:: secret\n```\n\nstatus:: public\n");
        assert!(
            note.inline_fields
                .contains(&("status".into(), "public".into()))
        );
        assert!(
            !note
                .inline_fields
                .contains(&("status".into(), "secret".into()))
        );
    }

    #[test]
    fn tasks_support_multiple_statuses_and_line_numbers() {
        let note = parse(
            "a.md",
            "---\nkey: value\n---\n\n- [ ] todo\n- [x] done\n- [>] prog\n- [-] cancelled\n- [?] blocked\n",
        );
        assert_eq!(note.tasks.len(), 5);
        assert_eq!(note.tasks[0].line, 5);
        assert_eq!(note.tasks[0].status, crate::TaskStatus::Todo);
        assert_eq!(note.tasks[0].text, "todo");
        assert_eq!(note.tasks[2].status, crate::TaskStatus::InProgress);
        assert_eq!(note.tasks[3].status, crate::TaskStatus::Cancelled);
        assert_eq!(note.tasks[4].status, crate::TaskStatus::Blocked);
    }
}
