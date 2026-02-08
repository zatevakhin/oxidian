use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::header;
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, broadcast};
use tracing::{info, warn};

use oxidian::{FileKind, GraphIndex, Vault, VaultIndex, VaultPath, VaultService};
#[cfg(feature = "similarity")]
use oxidian::{NoteSimilarityHit, SimilaritySettings};

const INDEX_HTML: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/web-ui/index.html"
));
const APP_JS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/web-ui/app.js"));

const DEFAULT_SIMILARITY_MIN_SCORE: f32 = 0.6;
const DEFAULT_SIMILARITY_TOP_K: usize = 8;

#[derive(Clone)]
struct AppState {
    service: Arc<Mutex<VaultService>>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ClientMessage {
    #[serde(rename = "similarity_settings")]
    SimilaritySettings {
        enabled: bool,
        min_score: f32,
        top_k: usize,
    },
}

#[derive(Debug, Serialize)]
struct GraphPayload {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
    similarity: SimilarityMeta,
}

#[derive(Debug, Serialize)]
struct GraphNode {
    id: String,
    label: String,
    kind: String,
    size: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    cluster_id: Option<u32>,
}

#[derive(Debug, Serialize)]
struct GraphEdge {
    id: String,
    source: String,
    target: String,
}

#[derive(Debug, Serialize, Clone, Copy)]
struct SimilarityMeta {
    available: bool,
    enabled: bool,
    min_score: f32,
    top_k: usize,
}

#[derive(Debug, Clone, Copy)]
struct SimilarityConfig {
    enabled: bool,
    min_score: f32,
    top_k: usize,
}

impl Default for SimilarityConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_score: DEFAULT_SIMILARITY_MIN_SCORE,
            top_k: DEFAULT_SIMILARITY_TOP_K,
        }
    }
}

pub async fn run(vault_path: PathBuf, bind: SocketAddr) -> anyhow::Result<()> {
    let vault = Vault::open(&vault_path)?;
    let mut service = VaultService::new(vault)?;
    info!(path = %vault_path.display(), "building index for web ui");
    service.build_index().await?;
    info!(path = %vault_path.display(), "starting watcher for web ui");
    service.start_watching().await?;

    let state = AppState {
        service: Arc::new(Mutex::new(service)),
    };

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/app.js", get(app_js_handler))
        .route("/ws", get(ws_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind).await?;
    info!(%bind, "web ui listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index_handler() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn app_js_handler() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        APP_JS,
    )
}

async fn ws_handler(State(state): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| ws_loop(socket, state))
}

async fn ws_loop(mut socket: WebSocket, state: AppState) {
    info!("web ui client connected");
    let mut events = {
        let service = state.service.lock().await;
        service.subscribe()
    };

    let mut similarity_settings = SimilarityConfig::default();

    if send_snapshot(&mut socket, &state, similarity_settings)
        .await
        .is_err()
    {
        return;
    }

    loop {
        tokio::select! {
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Text(text))) => {
                        if let Some(updated) = handle_client_message(&text) {
                            similarity_settings = updated;
                            if send_snapshot(&mut socket, &state, similarity_settings).await.is_err() {
                                break;
                            }
                        }
                    }
                    Some(Ok(_)) => {},
                    Some(Err(_)) => break,
                }
            }
            event = events.recv() => {
                match event {
                    Ok(_) => {
                        if send_snapshot(&mut socket, &state, similarity_settings).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        continue;
                    }
                    Err(_) => break,
                }
            }
        }
    }

    info!("web ui client disconnected");
}

fn handle_client_message(text: &str) -> Option<SimilarityConfig> {
    let message: ClientMessage = serde_json::from_str(text).ok()?;
    match message {
        ClientMessage::SimilaritySettings {
            enabled,
            min_score,
            top_k,
        } => Some(normalize_similarity_settings(SimilarityConfig {
            enabled,
            min_score,
            top_k,
        })),
    }
}

fn normalize_similarity_settings(settings: SimilarityConfig) -> SimilarityConfig {
    let min_score = settings.min_score.clamp(0.0, 1.0);
    let top_k = settings.top_k.clamp(1, 50);
    SimilarityConfig {
        enabled: settings.enabled,
        min_score,
        top_k,
    }
}

async fn send_snapshot(
    socket: &mut WebSocket,
    state: &AppState,
    similarity_settings: SimilarityConfig,
) -> Result<(), ()> {
    let payload = {
        let service = state.service.lock().await;
        match graph_payload(&service, similarity_settings) {
            Ok(payload) => payload,
            Err(err) => {
                warn!(error = %err, "failed to build graph payload");
                return Ok(());
            }
        }
    };

    let text = match serde_json::to_string(&payload) {
        Ok(text) => text,
        Err(err) => {
            warn!(error = %err, "failed to serialize graph payload");
            return Ok(());
        }
    };

    socket.send(Message::Text(text)).await.map_err(|_| ())
}

fn graph_payload(
    service: &VaultService,
    similarity_settings: SimilarityConfig,
) -> anyhow::Result<GraphPayload> {
    let snapshot = service.index_snapshot();
    let graph = snapshot.build_graph(service.vault())?;
    let (cluster_ids, similarity_meta) =
        similarity_cluster_ids(service, &snapshot, similarity_settings)?;
    Ok(build_graph_payload(
        &snapshot,
        &graph,
        cluster_ids.as_ref(),
        similarity_meta,
    ))
}

fn similarity_meta(settings: SimilarityConfig) -> SimilarityMeta {
    SimilarityMeta {
        available: cfg!(feature = "similarity"),
        enabled: cfg!(feature = "similarity") && settings.enabled,
        min_score: settings.min_score,
        top_k: settings.top_k,
    }
}

fn similarity_cluster_ids(
    service: &VaultService,
    snapshot: &VaultIndex,
    settings: SimilarityConfig,
) -> anyhow::Result<(Option<BTreeMap<String, u32>>, SimilarityMeta)> {
    let meta = similarity_meta(settings);
    #[cfg(feature = "similarity")]
    {
        if !settings.enabled {
            return Ok((None, meta));
        }
        let settings = SimilaritySettings {
            min_score: settings.min_score,
            top_k: settings.top_k,
        };
        let report = match service.note_similarity_report_with_settings(settings) {
            Ok(report) => report,
            Err(err) => {
                warn!(error = %err, "failed to compute similarity clusters");
                return Ok((
                    None,
                    SimilarityMeta {
                        enabled: false,
                        ..meta
                    },
                ));
            }
        };
        let note_ids: Vec<String> = snapshot
            .all_files()
            .filter(|f| matches!(f.kind, FileKind::Markdown | FileKind::Canvas))
            .map(|f| f.path.as_str_lossy().to_string())
            .collect();
        let clusters = cluster_ids_from_hits(&note_ids, &report.hits);
        return Ok((Some(clusters), meta));
    }
    #[cfg(not(feature = "similarity"))]
    {
        let _ = service;
        let _ = snapshot;
        let _ = settings;
        Ok((None, meta))
    }
}

fn build_graph_payload(
    snapshot: &VaultIndex,
    graph: &GraphIndex,
    clusters: Option<&BTreeMap<String, u32>>,
    similarity: SimilarityMeta,
) -> GraphPayload {
    let mut nodes: BTreeMap<String, GraphNode> = BTreeMap::new();
    let mut edges = Vec::new();
    let mut edge_keys: BTreeSet<String> = BTreeSet::new();
    for file in snapshot.all_files() {
        if !matches!(file.kind, FileKind::Markdown | FileKind::Canvas) {
            continue;
        }
        insert_node(snapshot, &file.path, &mut nodes, clusters);

        if let Some(note) = snapshot.note(&file.path) {
            let note_id = file.path.as_str_lossy().to_string();
            for tag in &note.tags {
                let tag_id = format!("tag:{}", tag.0);
                insert_tag_node(&tag_id, &tag.0, &mut nodes);
                insert_edge(&note_id, &tag_id, "tag", &mut edges, &mut edge_keys);
            }
        }
    }

    for target in graph.backlinks.targets() {
        let target_id = target.as_str_lossy().to_string();
        insert_node(snapshot, target, &mut nodes, clusters);

        for backlink in graph.backlinks.backlinks(target) {
            let source_id = backlink.source.as_str_lossy().to_string();
            insert_node(snapshot, &backlink.source, &mut nodes, clusters);
            insert_edge(&source_id, &target_id, "link", &mut edges, &mut edge_keys);
        }
    }

    let mut degree: BTreeMap<String, usize> = BTreeMap::new();
    for edge in &edges {
        *degree.entry(edge.source.clone()).or_default() += 1;
        *degree.entry(edge.target.clone()).or_default() += 1;
    }

    let nodes = nodes
        .into_iter()
        .map(|(id, mut node)| {
            let d = degree.get(&id).copied().unwrap_or(0);
            node.size = 3.0 + d as f32;
            node
        })
        .collect();

    GraphPayload {
        nodes,
        edges,
        similarity,
    }
}

fn insert_edge(
    source_id: &str,
    target_id: &str,
    kind: &str,
    edges: &mut Vec<GraphEdge>,
    edge_keys: &mut BTreeSet<String>,
) {
    let key = format!("{kind}:{source_id}->{target_id}");
    if edge_keys.insert(key.clone()) {
        edges.push(GraphEdge {
            id: key,
            source: source_id.to_string(),
            target: target_id.to_string(),
        });
    }
}

fn insert_node(
    snapshot: &VaultIndex,
    path: &VaultPath,
    nodes: &mut BTreeMap<String, GraphNode>,
    clusters: Option<&BTreeMap<String, u32>>,
) {
    let id = path.as_str_lossy().to_string();
    if nodes.contains_key(&id) {
        return;
    }

    let kind = snapshot
        .file(path)
        .map(|meta| file_kind_label(meta.kind))
        .unwrap_or("other");

    let cluster_id = clusters.and_then(|map| map.get(&id).copied());
    nodes.insert(
        id.clone(),
        GraphNode {
            id: id.clone(),
            label: id,
            kind: kind.to_string(),
            size: 1.0,
            cluster_id,
        },
    );
}

fn insert_tag_node(tag_id: &str, tag: &str, nodes: &mut BTreeMap<String, GraphNode>) {
    if nodes.contains_key(tag_id) {
        return;
    }

    nodes.insert(
        tag_id.to_string(),
        GraphNode {
            id: tag_id.to_string(),
            label: format!("#{tag}"),
            kind: "tag".to_string(),
            size: 1.0,
            cluster_id: None,
        },
    );
}

fn file_kind_label(kind: FileKind) -> &'static str {
    match kind {
        FileKind::Markdown => "markdown",
        FileKind::Canvas => "canvas",
        FileKind::Attachment => "attachment",
        FileKind::Other => "other",
    }
}

#[cfg(feature = "similarity")]
fn cluster_ids_from_hits(note_ids: &[String], hits: &[NoteSimilarityHit]) -> BTreeMap<String, u32> {
    let mut adjacency: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for id in note_ids {
        adjacency.entry(id.clone()).or_default();
    }

    for hit in hits {
        let source = hit.source.as_str_lossy().to_string();
        let target = hit.target.as_str_lossy().to_string();
        if source == target {
            continue;
        }
        if !adjacency.contains_key(&source) || !adjacency.contains_key(&target) {
            continue;
        }
        adjacency
            .entry(source.clone())
            .or_default()
            .insert(target.clone());
        adjacency.entry(target).or_default().insert(source);
    }

    let mut cluster_ids: BTreeMap<String, u32> = BTreeMap::new();
    let mut next_id = 1u32;

    for id in note_ids {
        if cluster_ids.contains_key(id) {
            continue;
        }
        let mut stack = vec![id.clone()];
        while let Some(current) = stack.pop() {
            if cluster_ids.contains_key(&current) {
                continue;
            }
            cluster_ids.insert(current.clone(), next_id);
            if let Some(neighbors) = adjacency.get(&current) {
                for neighbor in neighbors {
                    if !cluster_ids.contains_key(neighbor) {
                        stack.push(neighbor.clone());
                    }
                }
            }
        }
        next_id += 1;
    }

    cluster_ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_payload_tracks_resolved_edges() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("alpha.md"), "#tag-a\n[[beta]]").expect("write alpha");
        std::fs::write(dir.path().join("beta.md"), "beta note").expect("write beta");

        let vault = Vault::open(dir.path()).expect("vault open");
        let service = VaultService::new(vault).expect("service");

        let rt = tokio::runtime::Runtime::new().expect("runtime");
        rt.block_on(service.build_index()).expect("build index");

        let snapshot = service.index_snapshot();
        let graph = snapshot.build_graph(service.vault()).expect("graph");
        let payload = build_graph_payload(
            &snapshot,
            &graph,
            None,
            SimilarityMeta {
                available: false,
                enabled: false,
                min_score: DEFAULT_SIMILARITY_MIN_SCORE,
                top_k: DEFAULT_SIMILARITY_TOP_K,
            },
        );

        assert_eq!(payload.nodes.len(), 3);
        assert_eq!(payload.edges.len(), 2);
        assert!(
            payload
                .nodes
                .iter()
                .any(|node| node.id == "tag:tag-a" && node.kind == "tag")
        );
        assert!(
            payload
                .edges
                .iter()
                .any(|edge| edge.source == "alpha.md" && edge.target == "beta.md")
        );
        assert!(
            payload
                .edges
                .iter()
                .any(|edge| edge.source == "alpha.md" && edge.target == "tag:tag-a")
        );
    }

    #[cfg(feature = "similarity")]
    #[test]
    fn clusters_group_connected_hits() {
        let note_ids = vec!["a.md".to_string(), "b.md".to_string(), "c.md".to_string()];
        let hits = vec![
            NoteSimilarityHit {
                source: VaultPath::try_from(std::path::Path::new("a.md")).expect("source"),
                target: VaultPath::try_from(std::path::Path::new("b.md")).expect("target"),
                score: 0.8,
            },
            NoteSimilarityHit {
                source: VaultPath::try_from(std::path::Path::new("b.md")).expect("source"),
                target: VaultPath::try_from(std::path::Path::new("a.md")).expect("target"),
                score: 0.8,
            },
        ];

        let clusters = cluster_ids_from_hits(&note_ids, &hits);
        assert_eq!(clusters.get("a.md"), clusters.get("b.md"));
        assert_ne!(clusters.get("a.md"), clusters.get("c.md"));
    }
}
