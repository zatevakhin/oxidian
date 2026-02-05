use crate::{Backlink, BacklinksIndex, Link, LinkTarget, ResolveResult, VaultIndex, VaultPath};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedInternalLink {
    pub source: VaultPath,
    pub link: Link,
    pub resolution: ResolveResult,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GraphIndex {
    pub backlinks: BacklinksIndex,
    pub issues: Vec<ResolvedInternalLink>,
}

impl GraphIndex {
    pub fn unresolved(&self) -> impl Iterator<Item = &ResolvedInternalLink> {
        self.issues
            .iter()
            .filter(|i| matches!(i.resolution, ResolveResult::Missing))
    }

    pub fn ambiguous(&self) -> impl Iterator<Item = &ResolvedInternalLink> {
        self.issues
            .iter()
            .filter(|i| matches!(i.resolution, ResolveResult::Ambiguous(_)))
    }

    pub fn backlinks(&self, target: &VaultPath) -> &[Backlink] {
        self.backlinks.backlinks(target)
    }
}

pub(crate) fn build_graph(index: &VaultIndex) -> GraphIndex {
    let resolver = index.link_resolver();
    let mut out = GraphIndex::default();

    for (source, note) in index.notes_iter() {
        for link in &note.link_occurrences {
            if !matches!(link.target, LinkTarget::Internal { .. }) {
                continue;
            }
            let resolution = resolver.resolve_link_target(&link.target, source);
            match &resolution {
                ResolveResult::Resolved(target) => {
                    out.backlinks
                        .inbound
                        .entry(target.clone())
                        .or_default()
                        .push(Backlink {
                            source: source.clone(),
                            link: link.clone(),
                        });
                }
                ResolveResult::Missing => {
                    out.backlinks.unresolved += 1;
                    out.issues.push(ResolvedInternalLink {
                        source: source.clone(),
                        link: link.clone(),
                        resolution,
                    });
                }
                ResolveResult::Ambiguous(_) => {
                    out.backlinks.ambiguous += 1;
                    out.issues.push(ResolvedInternalLink {
                        source: source.clone(),
                        link: link.clone(),
                        resolution,
                    });
                }
            }
        }
    }

    for v in out.backlinks.inbound.values_mut() {
        v.sort_by(|a, b| {
            a.source
                .cmp(&b.source)
                .then_with(|| a.link.location.cmp(&b.link.location))
        });
    }
    out.issues.sort_by(|a, b| {
        a.source
            .cmp(&b.source)
            .then_with(|| a.link.location.cmp(&b.link.location))
    });
    out
}
