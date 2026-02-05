#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LinkKind {
    Wiki,
    Markdown,
    AutoUrl,
    ObsidianUri,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LinkTarget {
    Internal { reference: String },
    ExternalUrl(String),
    ObsidianUri { raw: String },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Subpath {
    Heading(String),
    Block(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LinkLocation {
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Link {
    pub kind: LinkKind,
    pub embed: bool,
    pub display: Option<String>,
    pub target: LinkTarget,
    pub subpath: Option<Subpath>,
    pub location: LinkLocation,
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkIssueReason {
    MissingTarget,
    AmbiguousTarget { candidates: Vec<crate::VaultPath> },
    MissingHeading { heading: String },
    MissingBlock { block: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkIssue {
    pub source: crate::VaultPath,
    pub link: Link,
    pub reason: LinkIssueReason,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinkHealthReport {
    pub total_internal_occurrences: usize,
    pub ok: usize,
    pub broken: Vec<LinkIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backlink {
    pub source: crate::VaultPath,
    pub link: Link,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BacklinksIndex {
    pub unresolved: usize,
    pub ambiguous: usize,
    pub(crate) inbound: std::collections::HashMap<crate::VaultPath, Vec<Backlink>>,
}

impl BacklinksIndex {
    pub fn backlinks(&self, target: &crate::VaultPath) -> &[Backlink] {
        self.inbound
            .get(target)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn targets(&self) -> impl Iterator<Item = &crate::VaultPath> {
        self.inbound.keys()
    }
}
