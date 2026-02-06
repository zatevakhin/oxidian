use std::collections::{HashMap, HashSet};

use crate::link_resolve::{ResolveResult, Resolver};
use crate::{
    Error, FileKind, LinkHealthReport, LinkIssue, LinkIssueReason, LinkTarget, Subpath, Vault,
    VaultIndex, VaultPath,
};

pub(crate) fn link_health_report(
    index: &VaultIndex,
    vault: &Vault,
) -> crate::Result<LinkHealthReport> {
    let resolver = Resolver::new(index);
    let mut cache: HashMap<VaultPath, TargetCache> = HashMap::new();

    let mut report = LinkHealthReport::default();

    for (source_path, note) in index.notes_iter() {
        for link in &note.link_occurrences {
            let LinkTarget::Internal { reference } = &link.target else {
                continue;
            };
            report.total_internal_occurrences += 1;

            let resolved_path = match resolver.resolve_internal_with_source(reference, source_path)
            {
                ResolveResult::Missing => {
                    report.broken.push(LinkIssue {
                        source: source_path.clone(),
                        link: link.clone(),
                        reason: LinkIssueReason::MissingTarget,
                    });
                    continue;
                }
                ResolveResult::Ambiguous(candidates) => {
                    report.broken.push(LinkIssue {
                        source: source_path.clone(),
                        link: link.clone(),
                        reason: LinkIssueReason::AmbiguousTarget { candidates },
                    });
                    continue;
                }
                ResolveResult::Resolved(p) => p,
            };

            if let Some(subpath) = &link.subpath {
                match validate_subpath(vault, index, &mut cache, &resolved_path, subpath)? {
                    SubpathCheck::Ok => {}
                    SubpathCheck::MissingHeading(h) => {
                        report.broken.push(LinkIssue {
                            source: source_path.clone(),
                            link: link.clone(),
                            reason: LinkIssueReason::MissingHeading { heading: h },
                        });
                        continue;
                    }
                    SubpathCheck::MissingBlock(b) => {
                        report.broken.push(LinkIssue {
                            source: source_path.clone(),
                            link: link.clone(),
                            reason: LinkIssueReason::MissingBlock { block: b },
                        });
                        continue;
                    }
                }
            }

            report.ok += 1;
        }
    }

    Ok(report)
}

#[derive(Debug, Clone)]
struct TargetCache {
    headings: HashSet<String>,
    heading_slugs: HashSet<String>,
    blocks: HashSet<String>,
}

enum SubpathCheck {
    Ok,
    MissingHeading(String),
    MissingBlock(String),
}

fn validate_subpath(
    vault: &Vault,
    index: &VaultIndex,
    cache: &mut HashMap<VaultPath, TargetCache>,
    target: &VaultPath,
    subpath: &Subpath,
) -> crate::Result<SubpathCheck> {
    let Some(file) = index.file(target) else {
        return Ok(SubpathCheck::Ok);
    };
    if !matches!(file.kind, FileKind::Markdown | FileKind::Canvas) {
        return Ok(SubpathCheck::Ok);
    }

    if !cache.contains_key(target) {
        let abs = vault.to_abs(target);
        let text = std::fs::read_to_string(&abs).map_err(|e| Error::io(&abs, e))?;
        let (headings, heading_slugs, blocks) = index_targets(&text);
        cache.insert(
            target.clone(),
            TargetCache {
                headings,
                heading_slugs,
                blocks,
            },
        );
    }
    let t = cache.get(target).expect("cached");

    match subpath {
        Subpath::Heading(h) => {
            let want = h.trim();
            if want.is_empty() {
                return Ok(SubpathCheck::Ok);
            }
            let want_l = want.to_lowercase();
            if t.headings.contains(&want_l) || t.heading_slugs.contains(&slugify(want)) {
                Ok(SubpathCheck::Ok)
            } else {
                Ok(SubpathCheck::MissingHeading(want.to_string()))
            }
        }
        Subpath::Block(b) => {
            let want = b.trim();
            if want.is_empty() {
                return Ok(SubpathCheck::Ok);
            }
            if t.blocks.contains(want) {
                Ok(SubpathCheck::Ok)
            } else {
                Ok(SubpathCheck::MissingBlock(want.to_string()))
            }
        }
    }
}

fn index_targets(text: &str) -> (HashSet<String>, HashSet<String>, HashSet<String>) {
    let mut headings = HashSet::new();
    let mut heading_slugs = HashSet::new();
    let mut blocks = HashSet::new();

    let mut in_fenced = false;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("```") {
            in_fenced = !in_fenced;
            continue;
        }
        if in_fenced {
            continue;
        }

        if let Some(h) = parse_heading(t) {
            let hl = h.to_lowercase();
            headings.insert(hl.clone());
            heading_slugs.insert(slugify(&hl));
        }
        if let Some(b) = parse_block_id(t) {
            blocks.insert(b);
        }
    }

    (headings, heading_slugs, blocks)
}

fn parse_heading(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() && bytes[i] == b'#' {
        i += 1;
    }
    if i == 0 || i > 6 {
        return None;
    }
    if i >= bytes.len() || bytes[i] != b' ' {
        return None;
    }
    let title = line[i + 1..].trim();
    if title.is_empty() {
        None
    } else {
        Some(title.to_string())
    }
}

fn parse_block_id(line: &str) -> Option<String> {
    let idx = line.rfind('^')?;
    if idx + 1 >= line.len() {
        return None;
    }
    if idx > 0 {
        let prev = line[..idx].chars().last().unwrap_or(' ');
        if !prev.is_whitespace() {
            return None;
        }
    }
    let rest = &line[idx + 1..];
    let id: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if id.is_empty() { None } else { Some(id) }
}

fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for c in s.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
            last_dash = false;
        } else if (c.is_whitespace() || matches!(c, '-' | '_' | '/'))
            && !out.is_empty()
            && !last_dash
        {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}
