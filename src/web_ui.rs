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
use serde::Serialize;
use tokio::sync::{Mutex, broadcast};
use tracing::{info, warn};

use oxidian::{FileKind, GraphIndex, Vault, VaultIndex, VaultPath, VaultService};

const INDEX_HTML: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/web-ui/index.html"
));
const APP_JS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/web-ui/app.js"));

#[derive(Clone)]
struct AppState {
    service: Arc<Mutex<VaultService>>,
}

#[derive(Debug, Serialize)]
struct GraphPayload {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
}

#[derive(Debug, Serialize)]
struct GraphNode {
    id: String,
    label: String,
    kind: String,
    size: f32,
}

#[derive(Debug, Serialize)]
struct GraphEdge {
    id: String,
    source: String,
    target: String,
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

    if send_snapshot(&mut socket, &state).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {},
                    Some(Err(_)) => break,
                }
            }
            event = events.recv() => {
                match event {
                    Ok(_) => {
                        if send_snapshot(&mut socket, &state).await.is_err() {
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

async fn send_snapshot(socket: &mut WebSocket, state: &AppState) -> Result<(), ()> {
    let payload = {
        let service = state.service.lock().await;
        match graph_payload(&service) {
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

fn graph_payload(service: &VaultService) -> anyhow::Result<GraphPayload> {
    let snapshot = service.index_snapshot();
    let graph = snapshot.build_graph(service.vault())?;
    Ok(build_graph_payload(&snapshot, &graph))
}

fn build_graph_payload(snapshot: &VaultIndex, graph: &GraphIndex) -> GraphPayload {
    let mut nodes: BTreeMap<String, GraphNode> = BTreeMap::new();
    let mut edges = Vec::new();
    let mut edge_keys: BTreeSet<String> = BTreeSet::new();
    for file in snapshot.all_files() {
        if !matches!(file.kind, FileKind::Markdown | FileKind::Canvas) {
            continue;
        }
        insert_node(snapshot, &file.path, &mut nodes);

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
        insert_node(snapshot, target, &mut nodes);

        for backlink in graph.backlinks.backlinks(target) {
            let source_id = backlink.source.as_str_lossy().to_string();
            insert_node(snapshot, &backlink.source, &mut nodes);
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

    GraphPayload { nodes, edges }
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

fn insert_node(snapshot: &VaultIndex, path: &VaultPath, nodes: &mut BTreeMap<String, GraphNode>) {
    let id = path.as_str_lossy().to_string();
    if nodes.contains_key(&id) {
        return;
    }

    let kind = snapshot
        .file(path)
        .map(|meta| file_kind_label(meta.kind))
        .unwrap_or("other");

    nodes.insert(
        id.clone(),
        GraphNode {
            id: id.clone(),
            label: id,
            kind: kind.to_string(),
            size: 1.0,
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
        let payload = build_graph_payload(&snapshot, &graph);

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
}
