mod config;
mod error;
mod fields;
mod graph;
mod index;
#[cfg(feature = "similarity")]
mod embeddings;
#[cfg(feature = "similarity")]
mod similarity;
mod link_health;
mod link_resolve;
mod links;
mod mentions;
mod parse;
mod query;
mod service;
#[cfg(feature = "sqlite")]
mod sqlite;
mod vault;

pub use crate::config::VaultConfig;
pub use crate::error::{Error, Result};
pub use crate::fields::{FieldMap, FieldValue};
pub use crate::graph::{GraphIndex, ResolvedInternalLink};
pub use crate::index::{
    ContentSearchHit, FileKind, FileMeta, FrontmatterReport, FrontmatterStatus, IndexDelta,
    NoteMeta, SearchHit, Tag, Task, TaskStatus, VaultIndex,
};
pub use crate::link_resolve::{LinkResolver, ResolveResult};
pub use crate::links::{
    Backlink, BacklinksIndex, Link, LinkHealthReport, LinkIssue, LinkIssueReason, LinkKind,
    LinkLocation, LinkTarget, Subpath,
};
pub use crate::mentions::UnlinkedMention;
pub use crate::query::{CmpOp, Query, QueryHit, SortDir, SortKey, TaskHit, TaskQuery};
pub use crate::service::{ReindexCause, VaultEvent, VaultService, WatchKind};
#[cfg(feature = "similarity")]
pub use crate::similarity::{NoteSimilarityHit, NoteSimilarityReport};
#[cfg(feature = "sqlite")]
pub use crate::sqlite::SqliteIndexStore;
pub use crate::vault::{Vault, VaultPath};
