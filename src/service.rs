use std::path::Path;
use std::sync::{Arc, RwLock};

use notify::{EventKind, RecursiveMode, Watcher};
use tokio::sync::{broadcast, mpsc, watch};

use crate::schema::SchemaState;
use crate::{
    Error, IndexDelta, Result, Schema, SchemaSource, SchemaStatus, Vault, VaultIndex, VaultPath,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchKind {
    Create,
    Modify,
    ModifyData,
    ModifyMetadata,
    Remove,
    Rename,
    Access,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReindexCause {
    /// The user (or host application) explicitly requested an index rebuild.
    Manual,
    /// An indexing pass done as part of service startup.
    InitialBuild,
    /// A filesystem watch event triggered the reindex.
    Watch { kind: WatchKind, event_kind: String },
}

#[derive(Debug, Clone)]
pub enum VaultEvent {
    Indexed {
        path: VaultPath,
        cause: ReindexCause,
        delta: IndexDelta,
    },
    Removed {
        path: VaultPath,
        cause: ReindexCause,
        delta: IndexDelta,
    },
    Renamed {
        from: VaultPath,
        to: VaultPath,
        cause: ReindexCause,
        delta: IndexDelta,
    },
    Error {
        path: Option<VaultPath>,
        error: String,
    },
}

pub struct VaultService {
    vault: Vault,
    index: Arc<RwLock<VaultIndex>>,
    events: broadcast::Sender<VaultEvent>,
    shutdown_tx: watch::Sender<bool>,
    watcher: Option<notify::RecommendedWatcher>,
    watch_task: Option<tokio::task::JoinHandle<()>>,
}

impl VaultService {
    pub fn new(vault: Vault) -> Result<Self> {
        let (events, _) = broadcast::channel(512);
        let (shutdown_tx, _) = watch::channel(false);
        Ok(Self {
            vault,
            index: Arc::new(RwLock::new(VaultIndex::default())),
            events,
            shutdown_tx,
            watcher: None,
            watch_task: None,
        })
    }

    pub fn vault(&self) -> &Vault {
        &self.vault
    }

    pub fn subscribe(&self) -> broadcast::Receiver<VaultEvent> {
        self.events.subscribe()
    }

    pub fn with_index<R>(&self, f: impl FnOnce(&VaultIndex) -> R) -> R {
        let guard = self.index.read().unwrap_or_else(|e| e.into_inner());
        f(&guard)
    }

    pub fn index_snapshot(&self) -> VaultIndex {
        self.with_index(|idx| idx.clone())
    }

    pub async fn build_index(&self) -> Result<()> {
        let vault = self.vault.clone();
        let schema_state = self.schema_state_for_rebuild();
        let built = tokio::task::spawn_blocking(move || {
            VaultIndex::build_with_schema(&vault, schema_state)
        })
        .await
        .map_err(|e| Error::InvalidVaultPath(format!("index build task failed: {e}")))??;

        let mut guard = self.index.write().unwrap_or_else(|e| e.into_inner());
        *guard = built;
        Ok(())
    }

    pub fn search_filenames_fuzzy(&self, query: &str, limit: usize) -> Vec<crate::SearchHit> {
        self.with_index(|idx| idx.search_filenames_fuzzy(query, limit))
    }

    pub async fn search_content_fuzzy(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<crate::ContentSearchHit>> {
        let snapshot = self.index_snapshot();
        let vault = self.vault.clone();
        let q = query.to_string();
        tokio::task::spawn_blocking(move || snapshot.search_content_fuzzy(&vault, &q, limit))
            .await
            .map_err(|e| Error::InvalidVaultPath(format!("search task failed: {e}")))?
    }

    #[cfg(feature = "similarity")]
    pub async fn search_content_semantic(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<crate::SemanticSearchHit>> {
        let snapshot = self.index_snapshot();
        let vault = self.vault.clone();
        let q = query.to_string();
        tokio::task::spawn_blocking(move || snapshot.search_content_semantic(&vault, &q, limit))
            .await
            .map_err(|e| Error::InvalidVaultPath(format!("semantic search task failed: {e}")))?
    }

    #[cfg(feature = "similarity")]
    pub async fn search_content_semantic_with_min_score(
        &self,
        query: &str,
        limit: usize,
        min_score: f32,
    ) -> Result<Vec<crate::SemanticSearchHit>> {
        let snapshot = self.index_snapshot();
        let vault = self.vault.clone();
        let q = query.to_string();
        tokio::task::spawn_blocking(move || {
            snapshot.search_content_semantic_with_min_score(&vault, &q, limit, min_score)
        })
        .await
        .map_err(|e| Error::InvalidVaultPath(format!("semantic search task failed: {e}")))?
    }

    pub fn query(&self, q: &crate::Query) -> Vec<crate::QueryHit> {
        self.with_index(|idx| idx.query(q))
    }

    pub fn schema_status(&self) -> SchemaStatus {
        self.with_index(|idx| idx.schema_status().clone())
    }

    pub fn schema_report(&self) -> crate::SchemaReport {
        self.with_index(|idx| idx.schema_report())
    }

    pub fn schema_violations_for(&self, path: &VaultPath) -> Vec<crate::SchemaViolation> {
        self.with_index(|idx| idx.schema_violations_for(path))
    }

    pub fn query_tasks(&self, q: &crate::TaskQuery) -> Vec<crate::TaskHit> {
        self.with_index(|idx| idx.query_tasks(q))
    }

    pub fn link_health_report(&self) -> crate::Result<crate::LinkHealthReport> {
        let snapshot = self.index_snapshot();
        snapshot.link_health_report(self.vault())
    }

    #[cfg(feature = "similarity")]
    pub fn note_similarity_report(&self) -> crate::Result<crate::NoteSimilarityReport> {
        let snapshot = self.index_snapshot();
        snapshot.note_similarity_report(self.vault())
    }

    #[cfg(feature = "similarity")]
    pub fn note_similarity_report_with_settings(
        &self,
        settings: crate::SimilaritySettings,
    ) -> crate::Result<crate::NoteSimilarityReport> {
        let snapshot = self.index_snapshot();
        snapshot.note_similarity_report_with_settings(self.vault(), settings)
    }

    #[cfg(feature = "similarity")]
    pub fn note_similarity_for(
        &self,
        source: &VaultPath,
    ) -> crate::Result<Vec<crate::NoteSimilarityHit>> {
        let snapshot = self.index_snapshot();
        snapshot.note_similarity_for(self.vault(), source)
    }

    pub fn build_backlinks(&self) -> crate::Result<crate::BacklinksIndex> {
        let snapshot = self.index_snapshot();
        snapshot.build_backlinks(self.vault())
    }

    pub fn build_graph(&self) -> crate::Result<crate::GraphIndex> {
        let snapshot = self.index_snapshot();
        snapshot.build_graph(self.vault())
    }

    pub async fn unlinked_mentions(
        &self,
        target: &VaultPath,
        limit: usize,
    ) -> Result<Vec<crate::UnlinkedMention>> {
        let snapshot = self.index_snapshot();
        let vault = self.vault.clone();
        let target = target.clone();
        tokio::task::spawn_blocking(move || snapshot.unlinked_mentions(&vault, &target, limit))
            .await
            .map_err(|e| Error::InvalidVaultPath(format!("mentions task failed: {e}")))?
    }

    pub async fn start_watching(&mut self) -> Result<()> {
        if self.watcher.is_some() {
            return Ok(());
        }

        let (raw_tx, raw_rx) =
            mpsc::unbounded_channel::<std::result::Result<notify::Event, notify::Error>>();
        let mut watcher = notify::recommended_watcher(move |res| {
            let _ = raw_tx.send(res);
        })?;
        watcher.watch(self.vault.root(), RecursiveMode::Recursive)?;

        let vault = self.vault.clone();
        let index = Arc::clone(&self.index);
        let events = self.events.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let debounce = self.vault.config().watch_debounce;
        self.watch_task = Some(tokio::spawn(async move {
            watch_loop(vault, index, events, raw_rx, &mut shutdown_rx, debounce).await;
        }));
        self.watcher = Some(watcher);

        Ok(())
    }

    pub async fn reload_schema(&self) -> Result<()> {
        let vault = self.vault.clone();
        let schema_state = SchemaState::load(&vault);
        let built = tokio::task::spawn_blocking(move || {
            VaultIndex::build_with_schema(&vault, schema_state)
        })
        .await
        .map_err(|e| Error::InvalidVaultPath(format!("schema reload task failed: {e}")))??;

        let mut guard = self.index.write().unwrap_or_else(|e| e.into_inner());
        *guard = built;
        Ok(())
    }

    pub async fn set_schema(&self, schema: Schema) -> Result<()> {
        let vault = self.vault.clone();
        let schema_state = SchemaState::from_schema(schema);
        let built = tokio::task::spawn_blocking(move || {
            VaultIndex::build_with_schema(&vault, schema_state)
        })
        .await
        .map_err(|e| Error::InvalidVaultPath(format!("schema set task failed: {e}")))??;

        let mut guard = self.index.write().unwrap_or_else(|e| e.into_inner());
        *guard = built;
        Ok(())
    }

    pub async fn shutdown(&mut self) {
        let _ = self.shutdown_tx.send(true);
        self.watcher.take();
        if let Some(handle) = self.watch_task.take() {
            let _ = handle.await;
        }
    }
}

impl Drop for VaultService {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
    }
}

async fn watch_loop(
    vault: Vault,
    index: Arc<RwLock<VaultIndex>>,
    events: broadcast::Sender<VaultEvent>,
    mut raw_rx: mpsc::UnboundedReceiver<std::result::Result<notify::Event, notify::Error>>,
    shutdown_rx: &mut watch::Receiver<bool>,
    debounce: std::time::Duration,
) {
    let mut pending: Vec<notify::Event> = Vec::new();
    let mut debounce_armed = false;
    let debounce_timer =
        tokio::time::sleep(std::time::Duration::from_secs(60 * 60 * 24 * 365 * 10));
    tokio::pin!(debounce_timer);

    loop {
        if *shutdown_rx.borrow() {
            break;
        }

        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    break;
                }
            }

            maybe = raw_rx.recv() => {
                let Some(res) = maybe else { break; };
                match res {
                    Ok(ev) => {
                        pending.push(ev);
                        debounce_armed = true;
                        debounce_timer
                            .as_mut()
                            .reset(tokio::time::Instant::now() + debounce);
                    }
                    Err(err) => {
                        let _ = events.send(VaultEvent::Error { path: None, error: err.to_string() });
                    }
                }
            }

            _ = &mut debounce_timer, if debounce_armed => {
                if pending.is_empty() {
                    debounce_armed = false;
                    continue;
                }

                let batch = std::mem::take(&mut pending);
                debounce_armed = false;

                let vault2 = vault.clone();
                let index2 = Arc::clone(&index);
                let applied = tokio::task::spawn_blocking(move || apply_events(&vault2, &index2, batch)).await;
                let applied = match applied {
                    Ok(Ok(list)) => list,
                    Ok(Err(err)) => {
                        let _ = events.send(VaultEvent::Error { path: None, error: err.to_string() });
                        continue;
                    }
                    Err(join_err) => {
                        let _ = events.send(VaultEvent::Error { path: None, error: join_err.to_string() });
                        continue;
                    }
                };

                for ev in applied {
                    let _ = events.send(ev);
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
enum Op {
    Upsert {
        path: VaultPath,
        cause: ReindexCause,
    },
    Remove {
        path: VaultPath,
        cause: ReindexCause,
    },
    Rename {
        from: VaultPath,
        to: VaultPath,
        cause: ReindexCause,
    },
}

fn apply_events(
    vault: &Vault,
    index: &RwLock<VaultIndex>,
    batch: Vec<notify::Event>,
) -> Result<Vec<VaultEvent>> {
    let ops = events_to_ops(vault, &batch);

    let mut out = Vec::new();
    let mut guard = index.write().unwrap_or_else(|e| e.into_inner());

    for op in ops {
        match op {
            Op::Upsert { path, cause } => {
                // Avoid emitting events for directory touches.
                let abs = vault.to_abs(&path);
                match std::fs::metadata(&abs) {
                    Ok(meta) if !meta.is_file() => continue,
                    Ok(_) => {}
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                        let delta = guard.remove_path(&path);
                        out.push(VaultEvent::Removed { path, cause, delta });
                        continue;
                    }
                    Err(err) => {
                        out.push(VaultEvent::Error {
                            path: Some(path),
                            error: err.to_string(),
                        });
                        continue;
                    }
                }

                match guard.upsert_path(vault, path.clone()) {
                    Ok(delta) => out.push(VaultEvent::Indexed { path, cause, delta }),
                    Err(Error::Io { source, .. })
                        if source.kind() == std::io::ErrorKind::NotFound =>
                    {
                        let delta = guard.remove_path(&path);
                        out.push(VaultEvent::Removed { path, cause, delta });
                    }
                    Err(err) => out.push(VaultEvent::Error {
                        path: Some(path),
                        error: err.to_string(),
                    }),
                }
            }
            Op::Remove { path, cause } => {
                let delta = guard.remove_path(&path);
                out.push(VaultEvent::Removed { path, cause, delta });
            }
            Op::Rename { from, to, cause } => {
                let removed = guard.remove_path(&from);
                let added = match guard.upsert_path(vault, to.clone()) {
                    Ok(d) => d,
                    Err(err) => {
                        out.push(VaultEvent::Error {
                            path: Some(to.clone()),
                            error: err.to_string(),
                        });
                        IndexDelta::default()
                    }
                };

                let delta = IndexDelta {
                    added_tags: added.added_tags,
                    removed_tags: removed.removed_tags,
                    added_links: added.added_links,
                    removed_links: removed.removed_links,
                };

                out.push(VaultEvent::Renamed {
                    from,
                    to,
                    cause,
                    delta,
                });
            }
        }
    }

    Ok(out)
}

fn events_to_ops(vault: &Vault, batch: &[notify::Event]) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut upsert_ix: std::collections::HashMap<VaultPath, usize> =
        std::collections::HashMap::new();
    let mut remove_ix: std::collections::HashMap<VaultPath, usize> =
        std::collections::HashMap::new();

    for ev in batch {
        // Access/metadata events can be generated by simply *reading* files (including by us),
        // which creates self-trigger loops. We don't treat them as meaningful changes.
        match &ev.kind {
            EventKind::Access(_) => continue,
            EventKind::Modify(notify::event::ModifyKind::Metadata(_)) => continue,
            _ => {}
        }

        let ev_cause = cause_from_event_kind(&ev.kind);

        match &ev.kind {
            EventKind::Modify(notify::event::ModifyKind::Name(_)) if ev.paths.len() == 2 => {
                if let (Some(from), Some(to)) = (
                    to_vault_path(vault, &ev.paths[0]),
                    to_vault_path(vault, &ev.paths[1]),
                ) {
                    ops.push(Op::Rename {
                        from,
                        to,
                        cause: ReindexCause::Watch {
                            kind: WatchKind::Rename,
                            event_kind: format!("{:?}", ev.kind),
                        },
                    });
                }
            }

            EventKind::Remove(_) => {
                for p in &ev.paths {
                    if let Some(rel) = to_vault_path(vault, p) {
                        if let Some(ix) = remove_ix.get(&rel).copied() {
                            let Op::Remove { cause, .. } = &mut ops[ix] else {
                                continue;
                            };
                            *cause = merge_cause(cause.clone(), ev_cause.clone());
                        } else {
                            let ix = ops.len();
                            ops.push(Op::Remove {
                                path: rel.clone(),
                                cause: ev_cause.clone(),
                            });
                            remove_ix.insert(rel, ix);
                        }
                    }
                }
            }

            _ => {
                for p in &ev.paths {
                    // Only upsert indexable paths; still allow remove events to clean up.
                    if let Some(rel) = to_vault_path(vault, p) {
                        if !vault.is_indexable_rel(rel.as_path()) {
                            continue;
                        }

                        if let Some(ix) = upsert_ix.get(&rel).copied() {
                            let Op::Upsert { cause, .. } = &mut ops[ix] else {
                                continue;
                            };
                            *cause = merge_cause(cause.clone(), ev_cause.clone());
                        } else {
                            let ix = ops.len();
                            ops.push(Op::Upsert {
                                path: rel.clone(),
                                cause: ev_cause.clone(),
                            });
                            upsert_ix.insert(rel, ix);
                        }
                    }
                }
            }
        }
    }

    ops
}

fn cause_from_event_kind(kind: &EventKind) -> ReindexCause {
    ReindexCause::Watch {
        kind: watch_kind_from_event_kind(kind),
        event_kind: format!("{:?}", kind),
    }
}

fn watch_kind_from_event_kind(kind: &EventKind) -> WatchKind {
    match kind {
        EventKind::Create(_) => WatchKind::Create,
        EventKind::Remove(_) => WatchKind::Remove,
        EventKind::Access(_) => WatchKind::Access,
        EventKind::Modify(notify::event::ModifyKind::Name(_)) => WatchKind::Rename,
        EventKind::Modify(notify::event::ModifyKind::Metadata(_)) => WatchKind::ModifyMetadata,
        EventKind::Modify(notify::event::ModifyKind::Data(_)) => WatchKind::ModifyData,
        EventKind::Modify(_) => WatchKind::Modify,
        _ => WatchKind::Other,
    }
}

fn merge_cause(old: ReindexCause, new: ReindexCause) -> ReindexCause {
    if rank_cause(&new) >= rank_cause(&old) {
        new
    } else {
        old
    }
}

fn rank_cause(cause: &ReindexCause) -> u8 {
    match cause {
        ReindexCause::Manual => 100,
        ReindexCause::InitialBuild => 10,
        ReindexCause::Watch { kind, .. } => match kind {
            WatchKind::Remove => 90,
            WatchKind::Rename => 80,
            WatchKind::Create => 70,
            WatchKind::ModifyData => 60,
            WatchKind::Modify => 50,
            WatchKind::ModifyMetadata => 40,
            WatchKind::Access => 30,
            WatchKind::Other => 20,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("vault");
        std::fs::create_dir_all(&root).expect("create vault root");
        let vault = Vault::open(&root).expect("open vault");
        (dir, vault)
    }

    fn event(kind: EventKind, paths: Vec<std::path::PathBuf>) -> notify::Event {
        notify::Event {
            kind,
            paths,
            attrs: Default::default(),
        }
    }

    #[test]
    fn events_to_ops_ignores_access_events() {
        let (_temp, vault) = make_vault();
        let p = vault.root().join("notes/a.md");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "hi").unwrap();

        let ev = event(
            EventKind::Access(notify::event::AccessKind::Open(
                notify::event::AccessMode::Any,
            )),
            vec![p],
        );

        let ops = events_to_ops(&vault, &[ev]);
        assert!(ops.is_empty());
    }

    #[test]
    fn apply_events_skips_directories() {
        let (_temp, vault) = make_vault();
        let dir_path = vault.root().join("indexes");
        std::fs::create_dir_all(&dir_path).unwrap();

        let lock = RwLock::new(VaultIndex::default());
        let ev = event(
            EventKind::Create(notify::event::CreateKind::Folder),
            vec![dir_path],
        );

        let out = apply_events(&vault, &lock, vec![ev]).unwrap();
        assert!(out.is_empty());
    }
}

fn to_vault_path(vault: &Vault, abs: &Path) -> Option<VaultPath> {
    vault.to_rel(abs).ok()
}

impl VaultService {
    fn schema_state_for_rebuild(&self) -> SchemaState {
        self.with_index(|idx| match idx.schema_status() {
            SchemaStatus::Loaded {
                source: SchemaSource::Inline,
                ..
            } => idx.schema_state(),
            _ => SchemaState::load(self.vault()),
        })
    }
}
