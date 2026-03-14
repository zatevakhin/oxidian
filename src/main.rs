use std::collections::BTreeMap;
use std::fs;
#[cfg(feature = "web-ui")]
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
#[cfg(feature = "web-ui")]
use std::sync::Once;

use clap::{Parser, Subcommand, ValueEnum};
use oxidian::{
    FileKind, LayoutRule, Link, LinkIssueKind, LinkIssueReason, LinkKind, NodeSchema,
    NodeTypeSchema, PredicateDef, PredicatesSchema, Query, Schema, SchemaSeverity, ScopeResolution,
    SortDir, Tag, TaskQuery, TaskStatus, UnmatchedBehavior, Vault, VaultPath, VaultSchema,
    VaultScope, VaultService,
};

#[cfg(feature = "similarity")]
use oxidian::VaultConfig;

// ---------------------------------------------------------------------------
// Output helpers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

/// Unified envelope for JSON output.
#[derive(serde::Serialize)]
struct JsonEnvelope<T: serde::Serialize> {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonError>,
}

#[derive(serde::Serialize)]
struct JsonError {
    code: String,
    message: String,
}

fn emit_json<T: serde::Serialize>(data: &T) {
    let envelope = JsonEnvelope::<&T> {
        ok: true,
        data: Some(data),
        error: None,
    };
    println!(
        "{}",
        serde_json::to_string(&envelope).expect("json serialization")
    );
}

fn emit_json_error(code: &str, message: &str) {
    let envelope = JsonEnvelope::<()> {
        ok: false,
        data: None,
        error: Some(JsonError {
            code: code.to_string(),
            message: message.to_string(),
        }),
    };
    println!(
        "{}",
        serde_json::to_string(&envelope).expect("json serialization")
    );
}

fn format_schema_status(status: &oxidian::SchemaStatus) -> String {
    match status {
        oxidian::SchemaStatus::Disabled => "disabled".to_string(),
        oxidian::SchemaStatus::Loaded { version, .. } => format!("loaded (version {version})"),
        oxidian::SchemaStatus::Error { .. } => "invalid schema".to_string(),
    }
}

/// Emit a progress message to stderr, unless `--quiet` is set.
fn progress(quiet: bool, msg: &str) {
    if !quiet {
        eprintln!("{msg}");
    }
}

// ---------------------------------------------------------------------------
// CLI argument types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, ValueEnum)]
enum LinkKindArg {
    Wiki,
    Markdown,
    Autourl,
    ObsidianUri,
}

impl From<LinkKindArg> for LinkKind {
    fn from(value: LinkKindArg) -> Self {
        match value {
            LinkKindArg::Wiki => LinkKind::Wiki,
            LinkKindArg::Markdown => LinkKind::Markdown,
            LinkKindArg::Autourl => LinkKind::AutoUrl,
            LinkKindArg::ObsidianUri => LinkKind::ObsidianUri,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum StatusArg {
    Todo,
    Done,
    InProgress,
    Cancelled,
    Blocked,
}

impl From<StatusArg> for TaskStatus {
    fn from(value: StatusArg) -> Self {
        match value {
            StatusArg::Todo => TaskStatus::Todo,
            StatusArg::Done => TaskStatus::Done,
            StatusArg::InProgress => TaskStatus::InProgress,
            StatusArg::Cancelled => TaskStatus::Cancelled,
            StatusArg::Blocked => TaskStatus::Blocked,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SchemaSeverityArg {
    Warn,
    Error,
}

impl From<SchemaSeverityArg> for SchemaSeverity {
    fn from(value: SchemaSeverityArg) -> Self {
        match value {
            SchemaSeverityArg::Warn => SchemaSeverity::Warn,
            SchemaSeverityArg::Error => SchemaSeverity::Error,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SearchMode {
    Files,
    Content,
    Semantic,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SchemaTemplate {
    Para,
    Kg,
    KgMemory,
}

// ---------------------------------------------------------------------------
// CLI structure
// ---------------------------------------------------------------------------

#[derive(Debug, Parser)]
#[command(name = "oxi", version, about = "Obsidian vault indexing + query CLI")]
struct Cli {
    /// Path to the Obsidian vault.
    #[arg(long, env = "OBSIDIAN_VAULT", global = true)]
    vault: Option<PathBuf>,

    /// Output format.
    #[arg(long, short = 'o', global = true, value_enum, default_value = "text")]
    output: OutputFormat,

    /// Suppress progress messages on stderr.
    #[arg(long, short = 'q', global = true)]
    quiet: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    // ── Querying / Reading ──────────────────────────────────
    /// Search notes by filename, content, or embeddings.
    Search {
        /// Query string.
        query: String,

        /// Search mode.
        #[arg(long, value_enum, default_value = "files")]
        mode: SearchMode,

        /// Maximum number of results.
        #[arg(long, default_value_t = 20)]
        limit: usize,

        /// Minimum similarity score (semantic mode only).
        #[arg(long)]
        min_score: Option<f32>,
    },

    /// Dataview-like querying of notes.
    Query {
        /// Limit results to paths with this prefix.
        #[arg(long)]
        prefix: Option<String>,

        /// Limit results to notes with this tag.
        #[arg(long)]
        tag: Option<String>,

        /// Require that a field exists (repeatable).
        #[arg(long)]
        exists: Vec<String>,

        /// Field equals (repeatable): key=value.
        #[arg(long)]
        eq: Vec<String>,

        /// Field contains substring (repeatable): key=value.
        #[arg(long)]
        contains: Vec<String>,

        /// Field numeric greater-than (repeatable): key=value.
        #[arg(long)]
        gt: Vec<String>,

        /// Sort by field name.
        #[arg(long)]
        sort: Option<String>,

        /// Sort descending.
        #[arg(long)]
        desc: bool,

        /// Maximum number of results.
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },

    /// List tags with file counts.
    Tags {
        /// How many tags to print.
        #[arg(long, default_value_t = 50)]
        top: usize,
    },

    /// List indexed tasks.
    Tasks {
        /// Optional path prefix.
        #[arg(long)]
        prefix: Option<String>,

        /// Filter by status.
        #[arg(long, value_enum)]
        status: Option<StatusArg>,

        /// Filter by substring on task text.
        #[arg(long)]
        contains: Option<String>,

        /// Maximum number of tasks to print.
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },

    // ── Per-note inspection ─────────────────────────────────
    /// Show outgoing links for a note.
    Links {
        /// Note path (relative to vault).
        note: PathBuf,

        /// Filter by link kind.
        #[arg(long, value_enum)]
        kind: Option<LinkKindArg>,

        /// Only show embed links (e.g. ![[..]] or ![](..)).
        #[arg(long)]
        only_embeds: bool,
    },

    /// Show inbound links (backlinks) to a note.
    Backlinks {
        /// Target note path or name.
        note: String,
    },

    /// Find plain-text (unlinked) mentions of a note.
    Mentions {
        /// Target note path (relative to vault).
        note: PathBuf,

        /// Maximum number of results.
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },

    /// Find similar notes (embedding neighbors).
    Neighbors {
        /// Note path (relative to vault).
        note: PathBuf,

        /// Minimum similarity score.
        #[arg(long)]
        min_score: Option<f32>,

        /// Maximum neighbors.
        #[arg(long)]
        top_k: Option<usize>,
    },

    // ── Vault-wide inspection ───────────────────────────────
    /// Print vault statistics (file, note, tag counts).
    Stats {
        /// Optional tag to query for matching files.
        #[arg(long)]
        tag: Option<String>,
    },

    /// Graph summary and outgoing links.
    Graph {
        /// Source note path to show outgoing internal links.
        #[arg(long)]
        note: Option<PathBuf>,
    },

    // ── Auditing / Linting ──────────────────────────────────
    /// Audit and lint the vault.
    Check {
        #[command(subcommand)]
        command: CheckCommand,
    },

    // ── Infrastructure ──────────────────────────────────────
    /// Stream vault change events.
    Watch,

    /// Persist the index to SQLite and incrementally update.
    Persist {
        /// Optional SQLite DB path.
        #[arg(long)]
        db: Option<PathBuf>,
    },

    /// Schema utilities.
    Schema {
        #[command(subcommand)]
        command: SchemaCommand,
    },

    /// Serve a realtime graph UI over HTTP.
    #[cfg(feature = "web-ui")]
    #[command(name = "web-ui")]
    WebUi {
        /// Bind address for the web server.
        #[arg(long, default_value = "127.0.0.1:7878")]
        bind: SocketAddr,
    },
}

#[derive(Debug, Subcommand)]
enum CheckCommand {
    /// Audit internal links for missing/ambiguous targets.
    Links {
        /// Maximum number of issues to print.
        #[arg(long, default_value_t = 100)]
        limit: usize,

        /// Only include issues with these reasons (repeatable).
        #[arg(long, conflicts_with = "exclude_reason")]
        reason: Vec<LinkIssueKind>,

        /// Exclude issues with these reasons (repeatable).
        #[arg(long, conflicts_with = "reason")]
        exclude_reason: Vec<LinkIssueKind>,
    },
    /// Audit frontmatter across the vault.
    Frontmatter {
        /// Maximum number of issues to print.
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// Validate vault against its schema.
    Schema {
        /// Filter by severity.
        #[arg(long, value_enum)]
        severity: Option<SchemaSeverityArg>,

        /// Maximum number of violations to print.
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// Full similarity report across the vault.
    Similarity {
        /// Minimum similarity score.
        #[arg(long)]
        min_score: Option<f32>,

        /// Maximum neighbors per note.
        #[arg(long)]
        top_k: Option<usize>,
    },
}

#[derive(Debug, Subcommand)]
enum SchemaCommand {
    /// Initialize a default schema in the vault.
    Init {
        /// Schema template name.
        #[arg(long, value_enum, default_value = "para")]
        template: SchemaTemplate,

        /// Overwrite existing schema file.
        #[arg(long)]
        force: bool,
    },
}

// ---------------------------------------------------------------------------
// JSON-serializable output structs
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
struct StatsOutput {
    files: usize,
    notes: usize,
    tags: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    tag_filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tagged_files: Option<Vec<String>>,
}

#[derive(serde::Serialize)]
struct TagCount {
    tag: String,
    count: usize,
}

#[derive(serde::Serialize)]
struct LinksOutput {
    note: String,
    unique_targets: usize,
    occurrences: usize,
    links: Vec<oxidian::Link>,
}

#[derive(serde::Serialize)]
struct BacklinksOutput {
    target: String,
    count: usize,
    backlinks: Vec<oxidian::Backlink>,
    unresolved_internal_occurrences: usize,
    ambiguous_internal_occurrences: usize,
}

#[derive(serde::Serialize)]
struct MentionsOutput {
    count: usize,
    mentions: Vec<oxidian::UnlinkedMention>,
}

#[derive(serde::Serialize)]
struct GraphOutput {
    unresolved_internal_occurrences: usize,
    ambiguous_internal_occurrences: usize,
    issue_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    outgoing: Option<Vec<oxidian::ResolvedInternalLink>>,
}

#[derive(serde::Serialize)]
struct LinkHealthOutput {
    internal_occurrences: usize,
    ok: usize,
    broken_count: usize,
    broken: Vec<oxidian::LinkIssue>,
}

#[derive(serde::Serialize)]
struct FrontmatterOutput {
    notes_without_frontmatter: usize,
    notes_with_frontmatter_valid: usize,
    notes_with_frontmatter_broken: usize,
    missing: Vec<String>,
    broken: Vec<FrontmatterBroken>,
}

#[derive(serde::Serialize)]
struct FrontmatterBroken {
    path: String,
    error: String,
}

#[derive(serde::Serialize)]
struct SchemaCheckOutput {
    status: oxidian::SchemaStatus,
    errors: usize,
    warnings: usize,
    total_violations: usize,
    violations: Vec<oxidian::SchemaViolationRecord>,
}

#[derive(serde::Serialize)]
struct SchemaInitOutput {
    path: String,
    template: String,
}

#[cfg(feature = "sqlite")]
#[derive(serde::Serialize)]
struct PersistOutput {
    files: usize,
    notes: usize,
    tags: usize,
    tasks: usize,
    links: usize,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let fmt = cli.output;

    let result = run(cli).await;
    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            if matches!(fmt, OutputFormat::Json) {
                emit_json_error("error", &e.to_string());
                std::process::exit(1);
            }
            Err(e)
        }
    }
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    let fmt = cli.output;
    let quiet = cli.quiet;

    match cli.command {
        Command::Search {
            query,
            mode,
            limit,
            min_score,
        } => handle_search(cli.vault, fmt, quiet, query, mode, limit, min_score).await?,
        Command::Query {
            prefix,
            tag,
            exists,
            eq,
            contains,
            gt,
            sort,
            desc,
            limit,
        } => {
            handle_query(
                cli.vault, fmt, prefix, tag, exists, eq, contains, gt, sort, desc, limit,
            )
            .await?
        }
        Command::Tags { top } => handle_tags(cli.vault, fmt, top).await?,
        Command::Tasks {
            prefix,
            status,
            contains,
            limit,
        } => handle_tasks(cli.vault, fmt, prefix, status, contains, limit).await?,
        Command::Links {
            note,
            kind,
            only_embeds,
        } => handle_links(cli.vault, fmt, note, kind, only_embeds).await?,
        Command::Backlinks { note } => handle_backlinks(cli.vault, fmt, note).await?,
        Command::Mentions { note, limit } => handle_mentions(cli.vault, fmt, note, limit).await?,
        Command::Neighbors {
            note,
            min_score,
            top_k,
        } => handle_neighbors(cli.vault, fmt, quiet, note, min_score, top_k).await?,
        Command::Stats { tag } => handle_stats(cli.vault, fmt, tag).await?,
        Command::Graph { note } => handle_graph(cli.vault, fmt, note).await?,
        Command::Check { command } => handle_check(cli.vault, fmt, quiet, command).await?,
        Command::Watch => handle_watch(cli.vault, fmt, quiet).await?,
        Command::Persist { db } => handle_persist(cli.vault, fmt, quiet, db).await?,
        Command::Schema { command } => handle_schema(cli.vault, fmt, command).await?,
        #[cfg(feature = "web-ui")]
        Command::WebUi { bind } => handle_web_ui(cli.vault, bind).await?,
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn require_vault(vault: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    vault.ok_or_else(|| anyhow::anyhow!("--vault is required (or set OBSIDIAN_VAULT)"))
}

async fn open_service(vault: Option<PathBuf>) -> anyhow::Result<VaultService> {
    let vault_path = require_vault(vault)?;
    let vault = Vault::open(&vault_path)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;
    Ok(service)
}

#[cfg(feature = "similarity")]
async fn open_service_with_similarity(
    vault: Option<PathBuf>,
    min_score: Option<f32>,
    top_k: Option<usize>,
) -> anyhow::Result<VaultService> {
    let vault_path = require_vault(vault)?;
    let mut cfg = VaultConfig::default();
    if let Some(score) = min_score {
        cfg.similarity_min_score = score;
    }
    if let Some(k) = top_k {
        cfg.similarity_top_k = k;
    }
    let vault = Vault::with_config(&vault_path, cfg)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;
    Ok(service)
}

fn normalize_tag_for_query(raw: &str) -> anyhow::Result<String> {
    let s = raw.trim();
    if s.is_empty() {
        anyhow::bail!("tag is empty");
    }
    let s = s.strip_prefix('#').unwrap_or(s);
    let s = s.trim_matches('/').trim();
    if s.is_empty() {
        anyhow::bail!("tag is empty");
    }
    Ok(s.to_lowercase())
}

fn print_occ(l: &Link) {
    println!(
        "- {:?}\tembed={}\t{}:{}\ttarget={:?}\tsubpath={:?}\tdisplay={:?}\traw={:?}",
        l.kind, l.embed, l.location.line, l.location.column, l.target, l.subpath, l.display, l.raw
    );
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn handle_stats(
    vault: Option<PathBuf>,
    fmt: OutputFormat,
    tag: Option<String>,
) -> anyhow::Result<()> {
    let service = open_service(vault).await?;
    let snapshot = service.index_snapshot();

    let file_count = snapshot.all_files().count();
    let note_count = snapshot
        .all_files()
        .filter(|f| matches!(f.kind, FileKind::Markdown | FileKind::Canvas))
        .count();
    let tag_count = snapshot.all_tags().count();

    let (tag_filter, tagged_files) = if let Some(ref raw_tag) = tag {
        let t = normalize_tag_for_query(raw_tag)?;
        let files: Vec<String> = snapshot
            .files_with_tag(&Tag(t.clone()))
            .map(|p| p.as_str_lossy())
            .collect();
        (Some(t), Some(files))
    } else {
        (None, None)
    };

    match fmt {
        OutputFormat::Json => {
            emit_json(&StatsOutput {
                files: file_count,
                notes: note_count,
                tags: tag_count,
                tag_filter,
                tagged_files,
            });
        }
        OutputFormat::Text => {
            println!("stats");
            println!("  files: {file_count}");
            println!("  notes: {note_count}");
            println!("  tags: {tag_count}");

            if let (Some(tag_name), Some(files)) = (&tag_filter, &tagged_files) {
                println!("\nfiles with tag #{tag_name}:");
                for p in files {
                    println!("- {p}");
                }
            }
        }
    }

    Ok(())
}

async fn handle_tags(vault: Option<PathBuf>, fmt: OutputFormat, top: usize) -> anyhow::Result<()> {
    let service = open_service(vault).await?;
    let snapshot = service.index_snapshot();

    let mut rows: Vec<(Tag, usize)> = snapshot
        .all_tags()
        .cloned()
        .map(|t| {
            let n = snapshot.files_with_tag(&t).count();
            (t, n)
        })
        .collect();

    rows.sort_by(|(a_tag, a_n), (b_tag, b_n)| b_n.cmp(a_n).then_with(|| a_tag.0.cmp(&b_tag.0)));
    let rows: Vec<(Tag, usize)> = rows.into_iter().take(top).collect();

    match fmt {
        OutputFormat::Json => {
            let items: Vec<TagCount> = rows
                .iter()
                .map(|(tag, count)| TagCount {
                    tag: tag.0.clone(),
                    count: *count,
                })
                .collect();
            emit_json(&items);
        }
        OutputFormat::Text => {
            for (tag, n) in &rows {
                println!("{n}\t#{tag}", tag = tag.0);
            }
        }
    }

    Ok(())
}

async fn handle_tasks(
    vault: Option<PathBuf>,
    fmt: OutputFormat,
    prefix: Option<String>,
    status: Option<StatusArg>,
    contains: Option<String>,
    limit: usize,
) -> anyhow::Result<()> {
    let service = open_service(vault).await?;

    let mut q = TaskQuery::all();
    if let Some(prefix) = prefix {
        q = q.from_path_prefix(prefix);
    }
    if let Some(status) = status {
        q = q.status(status.into());
    }
    if let Some(needle) = contains {
        q = q.contains_text(needle);
    }
    q = q.limit(limit);

    let hits: Vec<oxidian::TaskHit> = service.query_tasks(&q);

    match fmt {
        OutputFormat::Json => {
            emit_json(&hits);
        }
        OutputFormat::Text => {
            for hit in &hits {
                println!(
                    "{:?}\t{}:{}\t{}",
                    hit.status,
                    hit.path.as_str_lossy(),
                    hit.line,
                    hit.text
                );
            }
        }
    }

    Ok(())
}

async fn handle_links(
    vault: Option<PathBuf>,
    fmt: OutputFormat,
    note: PathBuf,
    kind: Option<LinkKindArg>,
    only_embeds: bool,
) -> anyhow::Result<()> {
    let service = open_service(vault).await?;
    let snapshot = service.index_snapshot();

    let rel = VaultPath::try_from(note.as_path())?;
    let note_meta = snapshot
        .note(&rel)
        .ok_or_else(|| anyhow::anyhow!("note not found: {}", rel.as_str_lossy()))?;

    let kind_filter = kind.map(Into::into);
    let filtered: Vec<&Link> = note_meta
        .link_occurrences
        .iter()
        .filter(|l| kind_filter.as_ref().is_none_or(|k| &l.kind == k))
        .filter(|l| !only_embeds || l.embed)
        .collect();

    match fmt {
        OutputFormat::Json => {
            emit_json(&LinksOutput {
                note: rel.as_str_lossy(),
                unique_targets: note_meta.links.len(),
                occurrences: filtered.len(),
                links: filtered.into_iter().cloned().collect(),
            });
        }
        OutputFormat::Text => {
            println!("note: {}", rel.as_str_lossy());
            println!("summary");
            println!("  unique_targets: {}", note_meta.links.len());
            println!("  occurrences: {}", filtered.len());

            if !note_meta.links.is_empty() {
                println!("\nunique targets:");
                for t in &note_meta.links {
                    println!("- {t:?}");
                }
            }

            println!("\noccurrences:");
            for l in &filtered {
                print_occ(l);
            }
        }
    }

    Ok(())
}

async fn handle_backlinks(
    vault: Option<PathBuf>,
    fmt: OutputFormat,
    note: String,
) -> anyhow::Result<()> {
    let service = open_service(vault).await?;
    let snapshot = service.index_snapshot();
    let backlinks = service.build_backlinks()?;

    let target: VaultPath = if note.contains('/') || note.contains('.') {
        VaultPath::try_from(Path::new(&note))?
    } else {
        let needle = note.to_lowercase();
        let mut matches = Vec::new();
        for f in snapshot.all_files() {
            let Some(_note) = snapshot.note(&f.path) else {
                continue;
            };
            let Some(stem) = f.path.as_path().file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if stem.to_lowercase() == needle {
                matches.push(f.path.clone());
            }
        }
        matches.sort();
        matches.dedup();
        match matches.len() {
            0 => return Err(anyhow::anyhow!("could not resolve target: {}", note)),
            1 => matches.remove(0),
            _ => {
                return Err(anyhow::anyhow!(
                    "ambiguous target '{}': {:?}",
                    note,
                    matches
                ));
            }
        }
    };

    let items = backlinks.backlinks(&target);

    match fmt {
        OutputFormat::Json => {
            emit_json(&BacklinksOutput {
                target: target.as_str_lossy(),
                count: items.len(),
                backlinks: items.to_vec(),
                unresolved_internal_occurrences: backlinks.unresolved,
                ambiguous_internal_occurrences: backlinks.ambiguous,
            });
        }
        OutputFormat::Text => {
            println!("target: {}", target.as_str_lossy());
            println!("summary");
            println!("  backlinks: {}", items.len());
            for b in items {
                println!(
                    "- {}:{}\t{:?}\tembed={}\traw={:?}",
                    b.source.as_str_lossy(),
                    b.link.location.line,
                    b.link.target,
                    b.link.embed,
                    b.link.raw
                );
            }
            println!(
                "  unresolved_internal_occurrences: {}",
                backlinks.unresolved
            );
            println!("  ambiguous_internal_occurrences: {}", backlinks.ambiguous);
        }
    }

    Ok(())
}

async fn handle_mentions(
    vault: Option<PathBuf>,
    fmt: OutputFormat,
    note: PathBuf,
    limit: usize,
) -> anyhow::Result<()> {
    let service = open_service(vault).await?;
    let target = VaultPath::try_from(note.as_path())?;
    let mentions = service.unlinked_mentions(&target, limit).await?;

    match fmt {
        OutputFormat::Json => {
            emit_json(&MentionsOutput {
                count: mentions.len(),
                mentions,
            });
        }
        OutputFormat::Text => {
            println!("summary");
            println!("  mentions: {}", mentions.len());
            for m in &mentions {
                println!(
                    "- {}:{}\tterm={:?}\t{}",
                    m.source.as_str_lossy(),
                    m.line,
                    m.term,
                    m.line_text.trim()
                );
            }
        }
    }

    Ok(())
}

async fn handle_neighbors(
    vault: Option<PathBuf>,
    fmt: OutputFormat,
    quiet: bool,
    note: PathBuf,
    min_score: Option<f32>,
    top_k: Option<usize>,
) -> anyhow::Result<()> {
    #[cfg(not(feature = "similarity"))]
    {
        let _ = (vault, fmt, quiet, note, min_score, top_k);
        anyhow::bail!("This command requires --features similarity");
    }

    #[cfg(feature = "similarity")]
    {
        progress(quiet, "building index...");
        let service = open_service_with_similarity(vault, min_score, top_k).await?;
        progress(quiet, "index ready");

        let note_path = VaultPath::try_from(note.as_path())?;
        progress(
            quiet,
            &format!("computing similarity for {}...", note_path.as_str_lossy()),
        );
        let hits = service.note_similarity_for(&note_path)?;
        progress(quiet, &format!("done: {} hits", hits.len()));

        match fmt {
            OutputFormat::Json => {
                emit_json(&hits);
            }
            OutputFormat::Text => {
                for hit in &hits {
                    println!(
                        "{:.3}\t{}\t{}",
                        hit.score,
                        hit.source.as_str_lossy(),
                        hit.target.as_str_lossy()
                    );
                }
            }
        }

        Ok(())
    }
}

async fn handle_graph(
    vault: Option<PathBuf>,
    fmt: OutputFormat,
    note: Option<PathBuf>,
) -> anyhow::Result<()> {
    let service = open_service(vault).await?;
    let snapshot = service.index_snapshot();
    let graph = service.build_graph()?;

    let (source, outgoing) = if let Some(note) = note {
        let source = VaultPath::try_from(note.as_path())?;
        let links = snapshot.resolved_outgoing_internal_links(&source);
        (Some(source.as_str_lossy()), Some(links))
    } else {
        (None, None)
    };

    match fmt {
        OutputFormat::Json => {
            emit_json(&GraphOutput {
                unresolved_internal_occurrences: graph.backlinks.unresolved,
                ambiguous_internal_occurrences: graph.backlinks.ambiguous,
                issue_count: graph.issues.len(),
                source,
                outgoing,
            });
        }
        OutputFormat::Text => {
            println!("summary");
            println!(
                "  unresolved_internal_occurrences: {}",
                graph.backlinks.unresolved
            );
            println!(
                "  ambiguous_internal_occurrences: {}",
                graph.backlinks.ambiguous
            );
            println!("  issue_count: {}", graph.issues.len());

            if let (Some(src), Some(links)) = (&source, &outgoing) {
                println!("\nsource: {src}");
                for o in links {
                    match &o.resolution {
                        oxidian::ResolveResult::Resolved(p) => {
                            println!(
                                "- {}:{}\tresolved\t{}\traw={:?}",
                                o.source.as_str_lossy(),
                                o.link.location.line,
                                p.as_str_lossy(),
                                o.link.raw
                            );
                        }
                        oxidian::ResolveResult::Missing => {
                            println!(
                                "- {}:{}\tmissing\t{:?}\traw={:?}",
                                o.source.as_str_lossy(),
                                o.link.location.line,
                                o.link.target,
                                o.link.raw
                            );
                        }
                        oxidian::ResolveResult::Ambiguous(cands) => {
                            println!(
                                "- {}:{}\tambiguous\t{:?}\tcandidates={:?}\traw={:?}",
                                o.source.as_str_lossy(),
                                o.link.location.line,
                                o.link.target,
                                cands,
                                o.link.raw
                            );
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

async fn handle_search(
    vault: Option<PathBuf>,
    fmt: OutputFormat,
    quiet: bool,
    query: String,
    mode: SearchMode,
    limit: usize,
    min_score: Option<f32>,
) -> anyhow::Result<()> {
    match mode {
        SearchMode::Files => {
            let service = open_service(vault).await?;
            let hits = service.search_filenames_fuzzy(&query, limit);
            match fmt {
                OutputFormat::Json => emit_json(&hits),
                OutputFormat::Text => {
                    for hit in &hits {
                        println!("{}\t{}", hit.score, hit.path.as_str_lossy());
                    }
                }
            }
        }
        SearchMode::Content => {
            let service = open_service(vault).await?;
            let hits = service.search_content_fuzzy(&query, limit).await?;
            match fmt {
                OutputFormat::Json => emit_json(&hits),
                OutputFormat::Text => {
                    for hit in &hits {
                        println!(
                            "{}\t{}:{}\t{}",
                            hit.score,
                            hit.path.as_str_lossy(),
                            hit.line,
                            hit.line_text.trim()
                        );
                    }
                }
            }
        }
        SearchMode::Semantic => {
            #[cfg(not(feature = "similarity"))]
            {
                let _ = (vault, fmt, quiet, query, limit, min_score);
                anyhow::bail!("This command requires --features similarity");
            }

            #[cfg(feature = "similarity")]
            {
                progress(quiet, "building index...");
                let service = open_service_with_similarity(vault, min_score, None).await?;
                progress(quiet, "index ready");

                let hits = if let Some(score) = min_score {
                    service
                        .search_content_semantic_with_min_score(&query, limit, score)
                        .await?
                } else {
                    service.search_content_semantic(&query, limit).await?
                };
                match fmt {
                    OutputFormat::Json => emit_json(&hits),
                    OutputFormat::Text => {
                        for hit in &hits {
                            println!("{:.3}\t{}", hit.score, hit.path.as_str_lossy());
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn handle_query(
    vault: Option<PathBuf>,
    fmt: OutputFormat,
    prefix: Option<String>,
    tag: Option<String>,
    exists: Vec<String>,
    eq: Vec<String>,
    contains: Vec<String>,
    gt: Vec<String>,
    sort: Option<String>,
    desc: bool,
    limit: usize,
) -> anyhow::Result<()> {
    let service = open_service(vault).await?;

    let mut q = Query::notes();
    if let Some(prefix) = prefix {
        q = q.from_path_prefix(prefix);
    }
    if let Some(tag) = tag {
        q = q.from_tag(tag);
    }

    for key in exists {
        q = q.where_field(key).exists();
    }
    for kv in eq {
        let Some((k, v)) = kv.split_once('=') else {
            continue;
        };
        q = q.where_field(k).eq(v);
    }
    for kv in contains {
        let Some((k, v)) = kv.split_once('=') else {
            continue;
        };
        q = q.where_field(k).contains(v);
    }
    for kv in gt {
        let Some((k, v)) = kv.split_once('=') else {
            continue;
        };
        if let Ok(n) = v.trim().parse::<f64>() {
            q = q.where_field(k).gt(n);
        }
    }

    let dir = if desc { SortDir::Desc } else { SortDir::Asc };
    if let Some(field) = sort {
        q = q.sort_by_field(field, dir);
    } else {
        q = q.sort_by_path(dir);
    }
    q = q.limit(limit);

    let hits: Vec<oxidian::QueryHit> = service.query(&q);

    match fmt {
        OutputFormat::Json => emit_json(&hits),
        OutputFormat::Text => {
            for hit in &hits {
                println!("{}", hit.path.as_str_lossy());
            }
        }
    }

    Ok(())
}

async fn handle_check(
    vault: Option<PathBuf>,
    fmt: OutputFormat,
    quiet: bool,
    command: CheckCommand,
) -> anyhow::Result<()> {
    match command {
        CheckCommand::Links {
            limit,
            reason,
            exclude_reason,
        } => {
            let service = open_service(vault).await?;
            let report = service.link_health_report()?;

            let broken: Vec<oxidian::LinkIssue> = report
                .broken
                .into_iter()
                .filter(|issue| {
                    let kind = issue.reason.kind();
                    if !reason.is_empty() {
                        reason.contains(&kind)
                    } else if !exclude_reason.is_empty() {
                        !exclude_reason.contains(&kind)
                    } else {
                        true
                    }
                })
                .take(limit)
                .collect();

            match fmt {
                OutputFormat::Json => {
                    emit_json(&LinkHealthOutput {
                        internal_occurrences: report.total_internal_occurrences,
                        ok: report.ok,
                        broken_count: broken.len(),
                        broken,
                    });
                }
                OutputFormat::Text => {
                    println!("summary");
                    println!(
                        "  internal_occurrences: {}",
                        report.total_internal_occurrences
                    );
                    println!("  ok: {}", report.ok);
                    println!("  broken: {}", broken.len());

                    if !broken.is_empty() {
                        println!("\nbroken:");
                        for issue in &broken {
                            let where_ = format!(
                                "{}:{}",
                                issue.source.as_str_lossy(),
                                issue.link.location.line
                            );
                            match &issue.reason {
                                LinkIssueReason::MissingTarget => {
                                    println!("- {where_}\tmissing\t{:?}", issue.link.target);
                                }
                                LinkIssueReason::AmbiguousTarget { candidates } => {
                                    println!(
                                        "- {where_}\tambiguous\t{:?}\tcandidates={:?}",
                                        issue.link.target, candidates
                                    );
                                }
                                LinkIssueReason::MissingHeading { heading } => {
                                    println!(
                                        "- {where_}\tmissing_heading\t{:?}\t#{}",
                                        issue.link.target, heading
                                    );
                                }
                                LinkIssueReason::MissingBlock { block } => {
                                    println!(
                                        "- {where_}\tmissing_block\t{:?}\t^{}",
                                        issue.link.target, block
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        CheckCommand::Frontmatter { limit } => {
            let service = open_service(vault).await?;
            let snapshot = service.index_snapshot();
            let report = snapshot.frontmatter_report();

            let missing: Vec<String> = snapshot
                .notes_without_frontmatter()
                .take(limit)
                .map(|p| p.as_str_lossy())
                .collect();

            let broken: Vec<FrontmatterBroken> = snapshot
                .notes_with_broken_frontmatter()
                .take(limit)
                .map(|(p, err)| FrontmatterBroken {
                    path: p.as_str_lossy(),
                    error: err.to_string(),
                })
                .collect();

            match fmt {
                OutputFormat::Json => {
                    emit_json(&FrontmatterOutput {
                        notes_without_frontmatter: report.none,
                        notes_with_frontmatter_valid: report.valid,
                        notes_with_frontmatter_broken: report.broken,
                        missing,
                        broken,
                    });
                }
                OutputFormat::Text => {
                    println!("summary");
                    println!("  notes_without_frontmatter: {}", report.none);
                    println!("  notes_with_frontmatter_valid: {}", report.valid);
                    println!("  notes_with_frontmatter_broken: {}", report.broken);

                    if !missing.is_empty() {
                        println!("\nmissing:");
                        for p in &missing {
                            println!("- {p}");
                        }
                    }

                    if !broken.is_empty() {
                        println!("\nbroken:");
                        for b in &broken {
                            println!("- {}\t{}", b.path, b.error);
                        }
                    }
                }
            }
        }
        CheckCommand::Schema { severity, limit } => {
            let service = open_service(vault).await?;
            let report = service.schema_report();

            let severity_filter = severity.map(Into::into);
            let violations: Vec<oxidian::SchemaViolationRecord> = report
                .violations
                .into_iter()
                .filter(|v| {
                    severity_filter
                        .as_ref()
                        .is_none_or(|s| &v.violation.severity == s)
                })
                .take(limit)
                .collect();

            match fmt {
                OutputFormat::Json => {
                    emit_json(&SchemaCheckOutput {
                        status: report.status,
                        errors: report.errors,
                        warnings: report.warnings,
                        total_violations: violations.len(),
                        violations,
                    });
                }
                OutputFormat::Text => {
                    println!("schema");
                    println!("  status: {}", format_schema_status(&report.status));
                    println!("  errors: {}", report.errors);
                    println!("  warnings: {}", report.warnings);
                    println!("  total_violations: {}", violations.len());

                    if let oxidian::SchemaStatus::Error { error, .. } = &report.status {
                        println!("\nschema_errors:");
                        println!("- {error}");
                    }

                    if !violations.is_empty() {
                        println!("\nviolations:");
                        for v in &violations {
                            let path = v
                                .path
                                .as_ref()
                                .map(|p| p.as_str_lossy())
                                .unwrap_or_else(|| "<vault>".into());
                            println!(
                                "- {}\t{:?}\t{}\t{}",
                                path, v.violation.severity, v.violation.code, v.violation.message
                            );
                        }
                    }
                }
            }
        }
        CheckCommand::Similarity { min_score, top_k } => {
            #[cfg(not(feature = "similarity"))]
            {
                let _ = (vault, fmt, quiet, min_score, top_k);
                anyhow::bail!("This command requires --features similarity");
            }

            #[cfg(feature = "similarity")]
            {
                progress(quiet, "building index...");
                let service = open_service_with_similarity(vault, min_score, top_k).await?;
                progress(quiet, "index ready");

                progress(quiet, "computing similarity report...");
                let report = service.note_similarity_report()?;
                progress(
                    quiet,
                    &format!(
                        "done: {} hits across {} notes",
                        report.hits.len(),
                        report.total_notes
                    ),
                );

                match fmt {
                    OutputFormat::Json => emit_json(&report),
                    OutputFormat::Text => {
                        println!("total_notes\t{}", report.total_notes);
                        println!("pairs_checked\t{}", report.pairs_checked);
                        for hit in &report.hits {
                            println!(
                                "{:.3}\t{}\t{}",
                                hit.score,
                                hit.source.as_str_lossy(),
                                hit.target.as_str_lossy()
                            );
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

async fn handle_watch(
    vault: Option<PathBuf>,
    fmt: OutputFormat,
    quiet: bool,
) -> anyhow::Result<()> {
    let vault = Vault::open(require_vault(vault)?)?;
    let mut service = VaultService::new(vault)?;
    service.build_index().await?;
    let mut rx = service.subscribe();

    service.start_watching().await?;
    progress(quiet, "watching... (Ctrl-C to stop)");

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            ev = rx.recv() => {
                match ev {
                    Ok(ev) => match fmt {
                        OutputFormat::Json => {
                            println!("{}", serde_json::to_string(&ev).expect("json serialization"));
                        }
                        OutputFormat::Text => println!("{ev:?}"),
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        progress(quiet, &format!("(lagged {n} events)"));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    service.shutdown().await;
    Ok(())
}

async fn handle_persist(
    vault: Option<PathBuf>,
    fmt: OutputFormat,
    quiet: bool,
    db: Option<PathBuf>,
) -> anyhow::Result<()> {
    #[cfg(not(feature = "sqlite"))]
    {
        let _ = (vault, fmt, quiet, db);
        anyhow::bail!("This command requires --features sqlite");
    }

    #[cfg(feature = "sqlite")]
    {
        use oxidian::{SqliteIndexStore, VaultEvent};

        let vault = Vault::open(require_vault(vault)?)?;
        let mut service = VaultService::new(vault)?;
        service.build_index().await?;

        let mut store = match db {
            Some(p) => SqliteIndexStore::open_path(p)?,
            None => SqliteIndexStore::open_default(service.vault())?,
        };
        store.write_full_index(service.vault(), &service.index_snapshot())?;
        let (files, notes, tags, tasks, links) = store.counts()?;

        match fmt {
            OutputFormat::Json => {
                emit_json(&PersistOutput {
                    files,
                    notes,
                    tags,
                    tasks,
                    links,
                });
            }
            OutputFormat::Text => {
                println!(
                    "persisted: files={files} notes={notes} tags={tags} tasks={tasks} links={links}"
                );
            }
        }

        let mut rx = service.subscribe();
        service.start_watching().await?;
        progress(quiet, "watching... (Ctrl-C to stop)");

        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => break,
                ev = rx.recv() => {
                    let Ok(ev) = ev else { continue; };
                    match ev {
                        VaultEvent::Indexed { path, .. } => {
                            let snap = service.index_snapshot();
                            store.upsert_path(service.vault(), &snap, &path)?;
                        }
                        VaultEvent::Removed { path, .. } => {
                            store.remove_path(&path)?;
                        }
                        VaultEvent::Renamed { from, to, .. } => {
                            store.remove_path(&from)?;
                            let snap = service.index_snapshot();
                            store.upsert_path(service.vault(), &snap, &to)?;
                        }
                        VaultEvent::Error { .. } => {}
                    }
                }
            }
        }

        service.shutdown().await;
        Ok(())
    }
}

async fn handle_schema(
    vault: Option<PathBuf>,
    fmt: OutputFormat,
    command: SchemaCommand,
) -> anyhow::Result<()> {
    match command {
        SchemaCommand::Init { template, force } => {
            let vault = require_vault(vault)?;
            let schema_path = vault.join(".obsidian/oxidian/schema.toml");
            if schema_path.exists() && !force {
                anyhow::bail!(
                    "schema already exists at {}; use --force to overwrite",
                    schema_path.display()
                );
            }

            if let Some(parent) = schema_path.parent() {
                fs::create_dir_all(parent)?;
            }

            let schema = generate_schema_template(template);
            let contents = toml::to_string_pretty(&schema)
                .map_err(|err| anyhow::anyhow!("failed to serialize schema: {err}"))?;
            fs::write(&schema_path, contents)?;

            let template_name = format!("{template:?}");
            match fmt {
                OutputFormat::Json => {
                    emit_json(&SchemaInitOutput {
                        path: schema_path.display().to_string(),
                        template: template_name,
                    });
                }
                OutputFormat::Text => {
                    println!(
                        "schema written to {} (template: {})",
                        schema_path.display(),
                        template_name
                    );
                }
            }
        }
    }

    Ok(())
}

#[cfg(feature = "web-ui")]
async fn handle_web_ui(vault: Option<PathBuf>, bind: SocketAddr) -> anyhow::Result<()> {
    init_web_ui_logging();
    let vault_path = require_vault(vault)?;
    let vault = Vault::open(&vault_path)?;
    let mut service = VaultService::new(vault)?;
    service.build_index().await?;
    service.start_watching().await?;
    let config = oxidian::web_ui::WebUiConfig { bind };
    oxidian::web_ui::run(service, config).await
}

#[cfg(feature = "web-ui")]
fn init_web_ui_logging() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .init();
    });
}

// ---------------------------------------------------------------------------
// Schema templates
// ---------------------------------------------------------------------------

fn generate_schema_template(template: SchemaTemplate) -> Schema {
    match template {
        SchemaTemplate::Para => build_para_schema(),
        SchemaTemplate::Kg => build_kg_schema(),
        SchemaTemplate::KgMemory => build_kg_memory_schema(),
    }
}

fn build_para_schema() -> Schema {
    let node = NodeSchema {
        types: vec![
            "project".into(),
            "area".into(),
            "resource".into(),
            "archive".into(),
            "person".into(),
            "concept".into(),
            "doc".into(),
            "tool".into(),
        ],
        type_def: NodeTypeSchema {
            docs: map_str([
                ("project", "Active outcomes with a deadline."),
                ("area", "Ongoing responsibility."),
                ("resource", "Reference or topic material."),
                ("archive", "Inactive items."),
                ("person", "People."),
                ("concept", "Ideas, techniques, terms."),
                ("doc", "Notes, docs, pages, specs."),
                ("tool", "Software, tools, services."),
            ]),
        },
    };

    let predicates = PredicatesSchema {
        aliases: map_str([
            ("requires", "depends_on"),
            ("required_by", "dependency_of"),
            ("relates_to", "related_to"),
            ("cite", "references"),
            ("cites", "references"),
            ("ref", "references"),
        ]),
        defs: map_defs([
            (
                "depends_on",
                PredicateDef {
                    description: "A requires B to function or proceed.".into(),
                    domain: vec!["project", "area", "resource", "doc"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    inverse: Some("dependency_of".into()),
                    symmetric: false,
                    severity: SchemaSeverity::Error,
                },
            ),
            (
                "related_to",
                PredicateDef {
                    description: "Loose association (symmetric).".into(),
                    domain: vec!["*"].into_iter().map(str::to_string).collect(),
                    inverse: None,
                    symmetric: true,
                    severity: SchemaSeverity::Warn,
                },
            ),
            (
                "references",
                PredicateDef {
                    description: "A cites/points to B (stronger than plain links).".into(),
                    domain: vec!["doc", "resource", "project", "area"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    inverse: None,
                    symmetric: false,
                    severity: SchemaSeverity::Warn,
                },
            ),
            (
                "part_of",
                PredicateDef {
                    description: "Composition: A is part of B.".into(),
                    domain: vec!["resource", "doc", "project", "area", "archive"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    inverse: None,
                    symmetric: false,
                    severity: SchemaSeverity::Warn,
                },
            ),
            (
                "supports",
                PredicateDef {
                    description: "A supports B (resource -> project/area).".into(),
                    domain: vec!["resource", "doc"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    inverse: None,
                    symmetric: false,
                    severity: SchemaSeverity::Warn,
                },
            ),
        ]),
    };

    let vault = VaultSchema {
        scope_resolution: ScopeResolution::MostSpecific,
        unscoped: UnmatchedBehavior::Allow,
        deny: Vec::new(),
        scopes: vec![
            scope_notes(
                "projects",
                "projects",
                Some("Projects (outcomes)"),
                true,
                UnmatchedBehavior::Warn,
            ),
            scope_notes(
                "areas",
                "areas",
                Some("Areas (ongoing responsibilities)"),
                true,
                UnmatchedBehavior::Warn,
            ),
            scope_notes(
                "resources",
                "resources",
                Some("Resources (topics, references)"),
                true,
                UnmatchedBehavior::Warn,
            ),
            scope_notes(
                "archives",
                "archives",
                Some("Archives (inactive)"),
                true,
                UnmatchedBehavior::Warn,
            ),
            VaultScope {
                id: "inbox".into(),
                path: "inbox".into(),
                required: true,
                description: Some("Capture".into()),
                unmatched_files: UnmatchedBehavior::Allow,
                allow: Vec::new(),
                deny: Vec::new(),
                inherit_allow: false,
                inherit_deny: false,
                inherit_notes: false,
                kinds: Vec::new(),
                extensions: Vec::new(),
                notes: None,
                orphan_attachments: None,
            },
        ],
    };

    Schema {
        version: 1,
        node,
        predicates,
        vault,
    }
}

fn build_kg_schema() -> Schema {
    let node = NodeSchema {
        types: vec![
            "concept".into(),
            "entity".into(),
            "journal".into(),
            "person".into(),
            "org".into(),
            "project".into(),
            "system".into(),
            "tool".into(),
            "document".into(),
            "claim".into(),
            "evidence".into(),
            "task".into(),
        ],
        type_def: NodeTypeSchema {
            docs: map_str([
                ("concept", "Ideas, techniques, terms, taxonomies."),
                ("entity", "Concrete named things (non-person/org)."),
                ("journal", "Daily journal entry."),
                ("person", "People."),
                ("org", "Organizations or teams."),
                ("project", "Initiatives with outcomes."),
                ("system", "Composed systems or products."),
                ("tool", "Software tools or services."),
                ("document", "Docs, papers, specs, notes."),
                ("claim", "Statements that can be supported or contradicted."),
                ("evidence", "Evidence backing claims."),
                ("task", "Tasks or benchmarks."),
            ]),
        },
    };

    let predicates = PredicatesSchema {
        aliases: map_str([
            ("relates_to", "related_to"),
            ("similar_to", "related_to"),
            ("belongs_to", "part_of"),
            ("owned", "owned_by"),
            ("author", "authored_by"),
            ("cite", "cites"),
            ("references", "cites"),
            ("ref", "cites"),
        ]),
        defs: map_defs([
            (
                "is_a",
                PredicateDef {
                    description: "Classification: A is a kind of B.".into(),
                    domain: vec!["*"].into_iter().map(str::to_string).collect(),
                    inverse: None,
                    symmetric: false,
                    severity: SchemaSeverity::Error,
                },
            ),
            (
                "instance_of",
                PredicateDef {
                    description: "Instance of a concept.".into(),
                    domain: vec![
                        "entity", "system", "tool", "project", "task", "person", "org",
                    ]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                    inverse: None,
                    symmetric: false,
                    severity: SchemaSeverity::Warn,
                },
            ),
            (
                "part_of",
                PredicateDef {
                    description: "Composition: A is part of B.".into(),
                    domain: vec!["entity", "system", "project", "document"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    inverse: None,
                    symmetric: false,
                    severity: SchemaSeverity::Warn,
                },
            ),
            (
                "related_to",
                PredicateDef {
                    description: "Loose association (symmetric).".into(),
                    domain: vec!["*"].into_iter().map(str::to_string).collect(),
                    inverse: None,
                    symmetric: true,
                    severity: SchemaSeverity::Warn,
                },
            ),
            (
                "supports",
                PredicateDef {
                    description: "Evidence supports a claim or concept.".into(),
                    domain: vec!["evidence", "document"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    inverse: None,
                    symmetric: false,
                    severity: SchemaSeverity::Warn,
                },
            ),
            (
                "contradicts",
                PredicateDef {
                    description: "Evidence contradicts a claim or concept.".into(),
                    domain: vec!["evidence", "document"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    inverse: None,
                    symmetric: false,
                    severity: SchemaSeverity::Warn,
                },
            ),
            (
                "uses",
                PredicateDef {
                    description: "A uses B in implementation or workflow.".into(),
                    domain: vec!["system", "project", "tool"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    inverse: None,
                    symmetric: false,
                    severity: SchemaSeverity::Warn,
                },
            ),
            (
                "cites",
                PredicateDef {
                    description: "A cites B (stronger than plain links).".into(),
                    domain: vec!["document", "claim", "evidence"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    inverse: None,
                    symmetric: false,
                    severity: SchemaSeverity::Warn,
                },
            ),
            (
                "owned_by",
                PredicateDef {
                    description: "A is owned/maintained by a person/org.".into(),
                    domain: vec!["project", "system", "tool"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    inverse: None,
                    symmetric: false,
                    severity: SchemaSeverity::Warn,
                },
            ),
            (
                "authored_by",
                PredicateDef {
                    description: "A was authored by a person/org.".into(),
                    domain: vec!["document", "claim", "evidence"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    inverse: None,
                    symmetric: false,
                    severity: SchemaSeverity::Warn,
                },
            ),
            (
                "implements",
                PredicateDef {
                    description: "A implements a concept or spec.".into(),
                    domain: vec!["system", "tool"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    inverse: None,
                    symmetric: false,
                    severity: SchemaSeverity::Warn,
                },
            ),
            (
                "derives_from",
                PredicateDef {
                    description: "A derives from evidence or documents.".into(),
                    domain: vec!["claim", "document"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    inverse: None,
                    symmetric: false,
                    severity: SchemaSeverity::Warn,
                },
            ),
        ]),
    };

    let vault = VaultSchema {
        scope_resolution: ScopeResolution::MostSpecific,
        unscoped: UnmatchedBehavior::Allow,
        deny: Vec::new(),
        scopes: vec![
            scope_notes(
                "kg",
                "kg",
                Some("Knowledge graph notes"),
                true,
                UnmatchedBehavior::Warn,
            ),
            scope_notes_typed("kg_concepts", "kg/concepts", true, "concept"),
            scope_notes_typed("kg_entities", "kg/entities", true, "entity"),
            scope_notes_typed("kg_people", "kg/people", true, "person"),
            scope_notes_typed("kg_orgs", "kg/orgs", true, "org"),
            scope_notes_typed("kg_projects", "kg/projects", true, "project"),
            scope_notes_typed("kg_systems", "kg/systems", true, "system"),
            scope_notes_typed("kg_tools", "kg/tools", true, "tool"),
            scope_notes_typed("kg_documents", "kg/documents", true, "document"),
            scope_notes_typed("kg_claims", "kg/claims", true, "claim"),
            scope_notes_typed("kg_evidence", "kg/evidence", true, "evidence"),
            scope_notes_typed("kg_tasks", "kg/tasks", true, "task"),
            VaultScope {
                id: "sources".into(),
                path: "sources".into(),
                required: true,
                description: Some("Raw sources and references".into()),
                unmatched_files: UnmatchedBehavior::Warn,
                allow: vec![
                    allow_glob("sources_md", "**/*.md"),
                    allow_glob("sources_pdf", "**/*.pdf"),
                    allow_glob("sources_html", "**/*.html"),
                ],
                deny: Vec::new(),
                inherit_allow: false,
                inherit_deny: false,
                inherit_notes: false,
                kinds: Vec::new(),
                extensions: Vec::new(),
                notes: None,
                orphan_attachments: None,
            },
            scope_inherit("sources_papers", "sources/papers", true),
            scope_inherit("sources_links", "sources/links", true),
            scope_inherit("sources_notes", "sources/notes", true),
            VaultScope {
                id: "journal".into(),
                path: "journal".into(),
                required: true,
                description: Some("Daily journal".into()),
                unmatched_files: UnmatchedBehavior::Warn,
                allow: vec![LayoutRule {
                    id: "journal_weekly".into(),
                    description: None,
                    glob: None,
                    regex: None,
                    template: Some("{year}/{week}/{year}-{month}-{day}.md".into()),
                    severity: SchemaSeverity::Warn,
                }],
                deny: Vec::new(),
                inherit_allow: false,
                inherit_deny: false,
                inherit_notes: false,
                kinds: Vec::new(),
                extensions: Vec::new(),
                notes: Some(oxidian::ScopeNotes {
                    r#type: Some(oxidian::ScopeNoteType {
                        required: true,
                        allowed: vec!["journal".into()],
                        severity: SchemaSeverity::Error,
                    }),
                    require_any: None,
                }),
                orphan_attachments: None,
            },
            VaultScope {
                id: "inbox".into(),
                path: "inbox".into(),
                required: true,
                description: Some("Capture".into()),
                unmatched_files: UnmatchedBehavior::Allow,
                allow: Vec::new(),
                deny: Vec::new(),
                inherit_allow: false,
                inherit_deny: false,
                inherit_notes: false,
                kinds: Vec::new(),
                extensions: Vec::new(),
                notes: None,
                orphan_attachments: None,
            },
        ],
    };

    Schema {
        version: 1,
        node,
        predicates,
        vault,
    }
}

fn build_kg_memory_schema() -> Schema {
    let mut schema = build_kg_schema();
    insert_node_type(
        &mut schema.node,
        "memory",
        "Memory entries (daily context, reflections).",
    );
    insert_node_type(&mut schema.node, "event", "Memory event.");
    insert_node_type(&mut schema.node, "quote", "Memory quote.");
    insert_node_type(&mut schema.node, "decision", "Memory decision.");
    insert_node_type(&mut schema.node, "fact", "Memory fact.");
    insert_node_type(&mut schema.node, "preference", "Memory preference.");

    schema.vault.scopes.push(VaultScope {
        id: "memory".into(),
        path: "memory".into(),
        required: true,
        description: Some("Memories".into()),
        unmatched_files: UnmatchedBehavior::Error,
        allow: vec![LayoutRule {
            id: "memory_entry".into(),
            description: None,
            glob: None,
            regex: None,
            template: Some("{year}/{month}/{day}/{slug}.md".into()),
            severity: SchemaSeverity::Error,
        }],
        deny: Vec::new(),
        inherit_allow: false,
        inherit_deny: false,
        inherit_notes: false,
        kinds: Vec::new(),
        extensions: Vec::new(),
        notes: Some(oxidian::ScopeNotes {
            r#type: Some(oxidian::ScopeNoteType {
                required: true,
                allowed: vec![
                    "memory".into(),
                    "event".into(),
                    "quote".into(),
                    "decision".into(),
                    "fact".into(),
                    "preference".into(),
                ],
                severity: SchemaSeverity::Error,
            }),
            require_any: Some(oxidian::ScopeRequireAny {
                tags: vec![
                    "event".into(),
                    "quote".into(),
                    "decision".into(),
                    "fact".into(),
                    "preference".into(),
                ],
                types: vec![
                    "event".into(),
                    "quote".into(),
                    "decision".into(),
                    "fact".into(),
                    "preference".into(),
                ],
                severity: SchemaSeverity::Error,
            }),
        }),
        orphan_attachments: None,
    });

    schema.vault.scopes.push(VaultScope {
        id: "memory_assets".into(),
        path: "memory/assets".into(),
        required: true,
        description: Some("Memory attachments".into()),
        unmatched_files: UnmatchedBehavior::Warn,
        allow: vec![allow_glob("memory_assets_any", "**/*")],
        deny: Vec::new(),
        inherit_allow: false,
        inherit_deny: false,
        inherit_notes: false,
        kinds: vec![oxidian::ScopeKind::Attachment],
        extensions: Vec::new(),
        notes: None,
        orphan_attachments: Some(SchemaSeverity::Warn),
    });

    schema
}

fn allow_glob(id: &str, glob: &str) -> LayoutRule {
    LayoutRule {
        id: id.to_string(),
        description: None,
        glob: Some(glob.to_string()),
        regex: None,
        template: None,
        severity: SchemaSeverity::Warn,
    }
}

fn scope_notes(
    id: &str,
    path: &str,
    description: Option<&str>,
    required: bool,
    unmatched_files: UnmatchedBehavior,
) -> VaultScope {
    VaultScope {
        id: id.to_string(),
        path: path.to_string(),
        required,
        description: description.map(str::to_string),
        unmatched_files,
        allow: vec![
            allow_glob("notes_md", "**/*.md"),
            allow_glob("notes_canvas", "**/*.canvas"),
        ],
        deny: Vec::new(),
        inherit_allow: false,
        inherit_deny: false,
        inherit_notes: false,
        kinds: Vec::new(),
        extensions: Vec::new(),
        notes: None,
        orphan_attachments: None,
    }
}

fn scope_notes_typed(id: &str, path: &str, required: bool, note_type: &str) -> VaultScope {
    VaultScope {
        id: id.to_string(),
        path: path.to_string(),
        required,
        description: None,
        unmatched_files: UnmatchedBehavior::Warn,
        allow: Vec::new(),
        deny: Vec::new(),
        inherit_allow: true,
        inherit_deny: false,
        inherit_notes: false,
        kinds: Vec::new(),
        extensions: Vec::new(),
        notes: Some(oxidian::ScopeNotes {
            r#type: Some(oxidian::ScopeNoteType {
                required: true,
                allowed: vec![note_type.to_string()],
                severity: SchemaSeverity::Warn,
            }),
            require_any: None,
        }),
        orphan_attachments: None,
    }
}

fn scope_inherit(id: &str, path: &str, required: bool) -> VaultScope {
    VaultScope {
        id: id.to_string(),
        path: path.to_string(),
        required,
        description: None,
        unmatched_files: UnmatchedBehavior::Warn,
        allow: Vec::new(),
        deny: Vec::new(),
        inherit_allow: true,
        inherit_deny: false,
        inherit_notes: false,
        kinds: Vec::new(),
        extensions: Vec::new(),
        notes: None,
        orphan_attachments: None,
    }
}

fn insert_node_type(node: &mut NodeSchema, key: &str, doc: &str) {
    if !node.types.iter().any(|t| t.eq_ignore_ascii_case(key)) {
        node.types.push(key.to_string());
    }
    node.type_def
        .docs
        .entry(key.to_string())
        .or_insert_with(|| doc.to_string());
}

fn map_str<const N: usize>(items: [(&str, &str); N]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (k, v) in items {
        out.insert(k.to_string(), v.to_string());
    }
    out
}

fn map_defs<const N: usize>(items: [(&str, PredicateDef); N]) -> BTreeMap<String, PredicateDef> {
    let mut out: BTreeMap<String, PredicateDef> = BTreeMap::new();
    for (k, v) in items {
        out.insert(k.to_string(), v);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{SchemaTemplate, generate_schema_template};

    #[test]
    fn para_template_contains_sections() {
        let tpl = generate_schema_template(SchemaTemplate::Para);
        let text = toml::to_string_pretty(&tpl).expect("serialize schema");
        assert!(text.contains("[node]"));
        assert!(text.contains("[predicates.aliases]"));
        assert!(text.contains("[vault]"));
        assert!(text.contains("[[vault.scopes]]"));
        assert!(text.contains("projects"));
    }

    #[test]
    fn kg_template_contains_sections() {
        let tpl = generate_schema_template(SchemaTemplate::Kg);
        let text = toml::to_string_pretty(&tpl).expect("serialize schema");
        assert!(text.contains("[node]"));
        assert!(text.contains("[predicates.aliases]"));
        assert!(text.contains("[vault]"));
        assert!(text.contains("[[vault.scopes]]"));
        assert!(text.contains("kg"));
        assert!(text.contains("journal"));
    }

    #[test]
    fn kg_memory_template_contains_sections() {
        let tpl = generate_schema_template(SchemaTemplate::KgMemory);
        let text = toml::to_string_pretty(&tpl).expect("serialize schema");
        assert!(text.contains("[node]"));
        assert!(text.contains("[predicates.aliases]"));
        assert!(text.contains("[vault]"));
        assert!(text.contains("[[vault.scopes]]"));
        assert!(text.contains("memory"));
        assert!(text.contains("{year}/{month}/{day}/{slug}.md"));
    }
}
