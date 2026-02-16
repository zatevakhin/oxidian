use std::fs;
#[cfg(feature = "web-ui")]
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
#[cfg(feature = "web-ui")]
use std::sync::Once;

use clap::{Parser, Subcommand, ValueEnum};
use oxidian::{
    FileKind, LayoutRule, Link, LinkIssueReason, LinkKind, NodeSchema, NodeTypeSchema,
    PredicateDef, PredicatesSchema, Query, Schema, SchemaSeverity, ScopeResolution, SortDir, Tag,
    TaskQuery, TaskStatus, UnmatchedBehavior, Vault, VaultPath, VaultSchema, VaultScope,
    VaultService,
};

#[cfg(feature = "similarity")]
use oxidian::VaultConfig;

#[cfg(feature = "web-ui")]
mod web_ui;

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

#[derive(Debug, Parser)]
#[command(
    name = "oxidian",
    version,
    about = "Obsidian vault indexing + query CLI"
)]
struct Cli {
    /// Path to the Obsidian vault.
    #[arg(long, env = "OBSIDIAN_VAULT", global = true)]
    vault: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Reports and summaries.
    Report {
        #[command(subcommand)]
        command: ReportCommand,
    },
    /// Search by filename, note content, or embeddings.
    Search {
        #[command(subcommand)]
        command: SearchCommand,
    },
    /// Dataview-like querying via typed API.
    Query(QueryCommand),
    /// Watch a vault and print indexing events.
    Watch {
        #[command(subcommand)]
        command: WatchCommand,
    },
    /// Persist the index to SQLite and incrementally update.
    Sqlite {
        #[command(subcommand)]
        command: SqliteCommand,
    },
    /// Note similarity neighbors.
    Similarity {
        #[command(subcommand)]
        command: SimilarityCommand,
    },
    /// Serve a realtime graph UI over HTTP.
    #[cfg(feature = "web-ui")]
    #[command(name = "web-ui")]
    WebUi {
        /// Bind address for the web server.
        #[arg(long, default_value = "127.0.0.1:7878")]
        bind: SocketAddr,
    },
    /// Schema utilities.
    Schema {
        #[command(subcommand)]
        command: SchemaCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ReportCommand {
    /// Print file/note/tag counts.
    Stats {
        /// Optional tag to query for matching files.
        #[arg(long)]
        tag: Option<String>,
    },
    /// Print tags with file counts.
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
    /// List links and link occurrences.
    Links {
        /// Optional note path (relative to vault) to inspect.
        #[arg(long)]
        note: Option<PathBuf>,

        /// Filter by link kind.
        #[arg(long, value_enum)]
        kind: Option<LinkKindArg>,

        /// Only show embed links (e.g. ![[..]] or ![](..)).
        #[arg(long)]
        only_embeds: bool,
    },
    /// Show resolved inbound links (backlinks).
    Backlinks {
        /// Target note path (relative) or name to resolve.
        #[arg(long)]
        note: String,
    },
    /// Find plain-text (unlinked) mentions of a target note.
    Mentions {
        /// Target note path (relative to vault).
        #[arg(long)]
        note: PathBuf,

        /// Maximum number of results.
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// Graph issues and outgoing links.
    Graph {
        /// Source note path (relative to vault) to show outgoing internal links.
        #[arg(long)]
        note: Option<PathBuf>,
    },
    /// Audit internal links for missing/ambiguous targets and missing subpaths.
    LinkHealth {
        /// Print broken links.
        #[arg(long)]
        show_broken: bool,
    },
    /// Audit frontmatter across the vault.
    Frontmatter {
        /// Print paths for notes without frontmatter.
        #[arg(long)]
        show_missing: bool,

        /// Print paths for notes with broken frontmatter.
        #[arg(long)]
        show_broken: bool,
    },
    /// Full similarity report.
    Similarity {
        /// Minimum similarity score.
        #[arg(long)]
        min_score: Option<f32>,

        /// Maximum neighbors per note.
        #[arg(long)]
        top_k: Option<usize>,
    },
    /// Schema validation report.
    Schema {
        /// Print all schema violations.
        #[arg(long)]
        show_violations: bool,

        /// Filter by severity.
        #[arg(long, value_enum)]
        severity: Option<SchemaSeverityArg>,

        /// Maximum number of violations to print.
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
}

#[derive(Debug, Subcommand)]
enum SearchCommand {
    /// Search by filename.
    Files {
        /// Query string.
        #[arg(long)]
        query: String,

        /// Maximum number of results.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Search by note content.
    Content {
        /// Query string.
        #[arg(long)]
        query: String,

        /// Maximum number of results.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Semantic search by note embeddings.
    Semantic {
        /// Query string.
        #[arg(long)]
        query: String,

        /// Maximum number of results.
        #[arg(long, default_value_t = 20)]
        limit: usize,

        /// Minimum similarity score.
        #[arg(long)]
        min_score: Option<f32>,
    },
}

#[derive(Debug, Parser)]
struct QueryCommand {
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
    sort_field: Option<String>,

    /// Sort descending.
    #[arg(long)]
    desc: bool,

    /// Maximum number of results.
    #[arg(long, default_value_t = 50)]
    limit: usize,
}

#[derive(Debug, Subcommand)]
enum WatchCommand {
    /// Stream indexing events.
    Index,
}

#[derive(Debug, Subcommand)]
enum SqliteCommand {
    /// Persist the index to SQLite and incrementally update.
    Persist {
        /// Optional SQLite DB path.
        #[arg(long)]
        db: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum SimilarityCommand {
    /// Similar notes for a specific note.
    Neighbors {
        /// Relative note path to list neighbors for.
        #[arg(long)]
        note: PathBuf,

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

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SchemaTemplate {
    Para,
    Kg,
    KgMemory,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Report { command } => handle_report(cli.vault, command).await?,
        Command::Search { command } => handle_search(cli.vault, command).await?,
        Command::Query(command) => handle_query(cli.vault, command).await?,
        Command::Watch { command } => handle_watch(cli.vault, command).await?,
        Command::Sqlite { command } => handle_sqlite(cli.vault, command).await?,
        Command::Similarity { command } => handle_similarity(cli.vault, command).await?,
        Command::Schema { command } => handle_schema(cli.vault, command).await?,
        #[cfg(feature = "web-ui")]
        Command::WebUi { bind } => handle_web_ui(cli.vault, bind).await?,
    }

    Ok(())
}

async fn handle_report(vault: Option<PathBuf>, command: ReportCommand) -> anyhow::Result<()> {
    #[cfg(not(feature = "similarity"))]
    let similarity_enabled = false;
    #[cfg(feature = "similarity")]
    let similarity_enabled = true;

    let vault_path = require_vault(vault)?;
    let vault = Vault::open(&vault_path)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;
    let snapshot = service.index_snapshot();

    match command {
        ReportCommand::Stats { tag } => {
            let file_count = snapshot.all_files().count();
            let note_count = snapshot
                .all_files()
                .filter(|f| matches!(f.kind, FileKind::Markdown | FileKind::Canvas))
                .count();
            let tag_count = snapshot.all_tags().count();

            println!("stats");
            println!("  files: {file_count}");
            println!("  notes: {note_count}");
            println!("  tags: {tag_count}");

            if let Some(tag) = tag {
                let tag = normalize_tag_for_query(&tag)?;
                println!("\nfiles with tag #{tag}:");
                for p in snapshot.files_with_tag(&Tag(tag.clone())) {
                    println!("- {}", p.as_str_lossy());
                }
            }
        }
        ReportCommand::Tags { top } => {
            let mut rows: Vec<(Tag, usize)> = snapshot
                .all_tags()
                .cloned()
                .map(|t| {
                    let n = snapshot.files_with_tag(&t).count();
                    (t, n)
                })
                .collect();

            rows.sort_by(|(a_tag, a_n), (b_tag, b_n)| {
                b_n.cmp(a_n).then_with(|| a_tag.0.cmp(&b_tag.0))
            });

            for (tag, n) in rows.into_iter().take(top) {
                println!("{n}\t#{tag}", tag = tag.0);
            }
        }
        ReportCommand::Tasks {
            prefix,
            status,
            contains,
            limit,
        } => {
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

            for hit in service.query_tasks(&q) {
                println!(
                    "{:?}\t{}:{}\t{}",
                    hit.status,
                    hit.path.as_str_lossy(),
                    hit.line,
                    hit.text
                );
            }
        }
        ReportCommand::Links {
            note,
            kind,
            only_embeds,
        } => {
            if let Some(note) = note {
                let rel = VaultPath::try_from(note.as_path())?;
                let note = snapshot
                    .note(&rel)
                    .ok_or_else(|| anyhow::anyhow!("note not found: {}", rel.as_str_lossy()))?;

                println!("note: {}", rel.as_str_lossy());
                println!("summary");
                println!("  unique_targets: {}", note.links.len());
                println!("  occurrences: {}", note.link_occurrences.len());

                if !note.links.is_empty() {
                    println!("\nunique targets:");
                    for t in &note.links {
                        println!("- {t:?}");
                    }
                }

                let kind_filter = kind.map(Into::into);
                let occs = note
                    .link_occurrences
                    .iter()
                    .filter(|l| kind_filter.as_ref().is_none_or(|k| &l.kind == k))
                    .filter(|l| !only_embeds || l.embed);

                println!("\noccurrences:");
                for l in occs {
                    print_occ(l);
                }

                return Ok(());
            }

            let mut total = 0usize;
            let mut wiki = 0usize;
            let mut md = 0usize;
            let mut auto = 0usize;
            let mut obs_uri = 0usize;
            let mut embeds = 0usize;

            for f in snapshot.all_files() {
                let Some(note) = snapshot.note(&f.path) else {
                    continue;
                };
                for l in &note.link_occurrences {
                    total += 1;
                    if l.embed {
                        embeds += 1;
                    }
                    match l.kind {
                        LinkKind::Wiki => wiki += 1,
                        LinkKind::Markdown => md += 1,
                        LinkKind::AutoUrl => auto += 1,
                        LinkKind::ObsidianUri => obs_uri += 1,
                    }
                }
            }

            println!("occurrences");
            println!("  total: {total}");
            println!("  embeds: {embeds}");
            println!("  wiki: {wiki}");
            println!("  markdown: {md}");
            println!("  auto-url: {auto}");
            println!("  obsidian-uri: {obs_uri}");
        }
        ReportCommand::Backlinks { note } => {
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

            println!("target: {}", target.as_str_lossy());
            let items = backlinks.backlinks(&target);
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
        ReportCommand::Mentions { note, limit } => {
            let target = VaultPath::try_from(note.as_path())?;
            let mentions = service.unlinked_mentions(&target, limit).await?;
            println!("summary");
            println!("  mentions: {}", mentions.len());
            for m in mentions {
                println!(
                    "- {}:{}\tterm={:?}\t{}",
                    m.source.as_str_lossy(),
                    m.line,
                    m.term,
                    m.line_text.trim()
                );
            }
        }
        ReportCommand::Graph { note } => {
            let graph = service.build_graph()?;
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

            if let Some(note) = note {
                let source = VaultPath::try_from(note.as_path())?;
                let outgoing = snapshot.resolved_outgoing_internal_links(&source);
                println!("\nsource: {}", source.as_str_lossy());
                for o in outgoing {
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
        ReportCommand::LinkHealth { show_broken } => {
            let report = service.link_health_report()?;
            println!("summary");
            println!(
                "  internal_occurrences: {}",
                report.total_internal_occurrences
            );
            println!("  ok: {}", report.ok);
            println!("  broken: {}", report.broken.len());

            if show_broken {
                println!("\nbroken:");
                for issue in &report.broken {
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
        ReportCommand::Frontmatter {
            show_missing,
            show_broken,
        } => {
            let report = snapshot.frontmatter_report();
            println!("summary");
            println!("  notes_without_frontmatter: {}", report.none);
            println!("  notes_with_frontmatter_valid: {}", report.valid);
            println!("  notes_with_frontmatter_broken: {}", report.broken);

            if show_missing {
                println!("\nmissing:");
                for p in snapshot.notes_without_frontmatter() {
                    println!("- {}", p.as_str_lossy());
                }
            }

            if show_broken {
                println!("\nbroken:");
                for (p, err) in snapshot.notes_with_broken_frontmatter() {
                    println!("- {}\t{}", p.as_str_lossy(), err);
                }
            }
        }
        ReportCommand::Similarity { min_score, top_k } => {
            if !similarity_enabled {
                let _ = min_score;
                let _ = top_k;
                eprintln!("This command requires --features similarity");
                return Ok(());
            }

            #[cfg(feature = "similarity")]
            {
                let mut cfg = VaultConfig::default();
                if let Some(score) = min_score {
                    cfg.similarity_min_score = score;
                }
                if let Some(top_k) = top_k {
                    cfg.similarity_top_k = top_k;
                }

                let vault = Vault::with_config(&vault_path, cfg)?;
                let service = VaultService::new(vault)?;
                eprintln!("building index...");
                service.build_index().await?;
                eprintln!("index ready");

                eprintln!("computing similarity report...");
                let report = service.note_similarity_report()?;
                eprintln!(
                    "done: {} hits across {} notes",
                    report.hits.len(),
                    report.total_notes
                );
                println!("total_notes\t{}", report.total_notes);
                println!("pairs_checked\t{}", report.pairs_checked);
                for hit in report.hits {
                    println!(
                        "{:.3}\t{}\t{}",
                        hit.score,
                        hit.source.as_str_lossy(),
                        hit.target.as_str_lossy()
                    );
                }
            }
        }
        ReportCommand::Schema {
            show_violations,
            severity,
            limit,
        } => {
            let report = service.schema_report();
            println!("schema");
            println!("  status: {:?}", report.status);
            println!("  errors: {}", report.errors);
            println!("  warnings: {}", report.warnings);
            println!("  total_violations: {}", report.violations.len());

            if show_violations {
                let severity = severity.map(Into::into);
                println!("\nviolations:");
                for v in report
                    .violations
                    .iter()
                    .filter(|v| severity.as_ref().is_none_or(|s| &v.violation.severity == s))
                    .take(limit)
                {
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

    Ok(())
}

async fn handle_search(vault: Option<PathBuf>, command: SearchCommand) -> anyhow::Result<()> {
    #[cfg(not(feature = "similarity"))]
    if matches!(&command, SearchCommand::Semantic { .. }) {
        eprintln!("This command requires --features similarity");
        return Ok(());
    }

    let vault = Vault::open(require_vault(vault)?)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;

    match command {
        SearchCommand::Files { query, limit } => {
            let hits = service.search_filenames_fuzzy(&query, limit);
            for hit in hits {
                println!("{}\t{}", hit.score, hit.path.as_str_lossy());
            }
        }
        SearchCommand::Content { query, limit } => {
            let hits = service.search_content_fuzzy(&query, limit).await?;
            for hit in hits {
                println!(
                    "{}\t{}:{}\t{}",
                    hit.score,
                    hit.path.as_str_lossy(),
                    hit.line,
                    hit.line_text.trim()
                );
            }
        }
        SearchCommand::Semantic {
            query,
            limit,
            min_score,
        } => {
            #[cfg(feature = "similarity")]
            {
                let hits = if let Some(score) = min_score {
                    service
                        .search_content_semantic_with_min_score(&query, limit, score)
                        .await?
                } else {
                    service.search_content_semantic(&query, limit).await?
                };
                for hit in hits {
                    println!("{:.3}\t{}", hit.score, hit.path.as_str_lossy());
                }
            }

            #[cfg(not(feature = "similarity"))]
            {
                let _ = query;
                let _ = limit;
                let _ = min_score;
            }
        }
    }

    Ok(())
}

async fn handle_query(vault: Option<PathBuf>, command: QueryCommand) -> anyhow::Result<()> {
    let vault = Vault::open(require_vault(vault)?)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;

    let mut q = Query::notes();
    if let Some(prefix) = command.prefix {
        q = q.from_path_prefix(prefix);
    }
    if let Some(tag) = command.tag {
        q = q.from_tag(tag);
    }

    for key in command.exists {
        q = q.where_field(key).exists();
    }
    for kv in command.eq {
        let Some((k, v)) = kv.split_once('=') else {
            continue;
        };
        q = q.where_field(k).eq(v);
    }
    for kv in command.contains {
        let Some((k, v)) = kv.split_once('=') else {
            continue;
        };
        q = q.where_field(k).contains(v);
    }
    for kv in command.gt {
        let Some((k, v)) = kv.split_once('=') else {
            continue;
        };
        if let Ok(n) = v.trim().parse::<f64>() {
            q = q.where_field(k).gt(n);
        }
    }

    let dir = if command.desc {
        SortDir::Desc
    } else {
        SortDir::Asc
    };
    if let Some(field) = command.sort_field {
        q = q.sort_by_field(field, dir);
    } else {
        q = q.sort_by_path(dir);
    }
    q = q.limit(command.limit);

    for hit in service.query(&q) {
        println!("{}", hit.path.as_str_lossy());
    }

    Ok(())
}

async fn handle_watch(vault: Option<PathBuf>, command: WatchCommand) -> anyhow::Result<()> {
    match command {
        WatchCommand::Index => {
            let vault = Vault::open(require_vault(vault)?)?;
            let mut service = VaultService::new(vault)?;
            service.build_index().await?;
            let mut rx = service.subscribe();

            service.start_watching().await?;
            println!("watching... (Ctrl-C to stop)");

            loop {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => break,
                    ev = rx.recv() => {
                        match ev {
                            Ok(ev) => println!("{ev:?}"),
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                eprintln!("(lagged {n} events)");
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }
            }

            service.shutdown().await;
        }
    }

    Ok(())
}

async fn handle_sqlite(vault: Option<PathBuf>, command: SqliteCommand) -> anyhow::Result<()> {
    #[cfg(not(feature = "sqlite"))]
    {
        let _ = vault;
        let _ = command;
        eprintln!("This command requires --features sqlite");
        Ok(())
    }

    #[cfg(feature = "sqlite")]
    {
        use oxidian::{SqliteIndexStore, VaultEvent};

        match command {
            SqliteCommand::Persist { db } => {
                let vault = Vault::open(require_vault(vault)?)?;
                let mut service = VaultService::new(vault)?;
                service.build_index().await?;

                let mut store = match db {
                    Some(p) => SqliteIndexStore::open_path(p)?,
                    None => SqliteIndexStore::open_default(service.vault())?,
                };
                store.write_full_index(service.vault(), &service.index_snapshot())?;
                let (files, notes, tags, tasks, links) = store.counts()?;
                println!(
                    "persisted: files={files} notes={notes} tags={tags} tasks={tasks} links={links}"
                );

                let mut rx = service.subscribe();
                service.start_watching().await?;
                println!("watching... (Ctrl-C to stop)");

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
            }
        }

        Ok(())
    }
}

async fn handle_similarity(
    vault: Option<PathBuf>,
    command: SimilarityCommand,
) -> anyhow::Result<()> {
    #[cfg(not(feature = "similarity"))]
    {
        let _ = vault;
        let _ = command;
        eprintln!("This command requires --features similarity");
        Ok(())
    }

    #[cfg(feature = "similarity")]
    {
        let mut cfg = VaultConfig::default();
        let (min_score, top_k) = match &command {
            SimilarityCommand::Neighbors {
                min_score, top_k, ..
            } => (*min_score, *top_k),
        };
        if let Some(score) = min_score {
            cfg.similarity_min_score = score;
        }
        if let Some(top_k) = top_k {
            cfg.similarity_top_k = top_k;
        }

        let vault = Vault::with_config(require_vault(vault)?, cfg)?;
        let service = VaultService::new(vault)?;
        eprintln!("building index...");
        service.build_index().await?;
        eprintln!("index ready");

        match command {
            SimilarityCommand::Neighbors { note, .. } => {
                let note_path = VaultPath::try_from(note.as_path())?;
                eprintln!("computing similarity for {}...", note_path.as_str_lossy());
                let hits = service.note_similarity_for(&note_path)?;
                eprintln!("done: {} hits", hits.len());
                for hit in hits {
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

async fn handle_schema(vault: Option<PathBuf>, command: SchemaCommand) -> anyhow::Result<()> {
    match command {
        SchemaCommand::Init { template, force } => {
            let vault = require_vault(vault)?;
            let schema_path = vault.join(".obsidian/oxidian/schema.toml");
            if schema_path.exists() && !force {
                eprintln!(
                    "schema already exists at {}; use --force to overwrite",
                    schema_path.display()
                );
                anyhow::bail!("schema already exists");
            }

            if let Some(parent) = schema_path.parent() {
                fs::create_dir_all(parent)?;
            }

            let schema = generate_schema_template(template);
            let contents = toml::to_string_pretty(&schema)
                .map_err(|err| anyhow::anyhow!("failed to serialize schema: {err}"))?;
            fs::write(&schema_path, contents)?;
            println!(
                "schema written to {} (template: {:?})",
                schema_path.display(),
                template
            );
        }
    }

    Ok(())
}

#[cfg(feature = "web-ui")]
async fn handle_web_ui(vault: Option<PathBuf>, bind: SocketAddr) -> anyhow::Result<()> {
    init_web_ui_logging();
    let vault_path = require_vault(vault)?;
    web_ui::run(vault_path, bind).await
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

fn require_vault(vault: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    vault.ok_or_else(|| anyhow::anyhow!("--vault is required (or set OBSIDIAN_VAULT)"))
}

fn print_occ(l: &Link) {
    println!(
        "- {:?}\tembed={}\t{}:{}\ttarget={:?}\tsubpath={:?}\tdisplay={:?}\traw={:?}",
        l.kind, l.embed, l.location.line, l.location.column, l.target, l.subpath, l.display, l.raw
    );
}

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
                    template: Some(
                        "{year:yyyy}/{week:ww}/{year:yyyy}-{month:mm}-{day:dd}.md".into(),
                    ),
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
            template: Some("{year:yyyy}/{month:mm}/{day:dd}/{slug:slug}.md".into()),
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

fn map_str<const N: usize>(items: [(&str, &str); N]) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    for (k, v) in items {
        out.insert(k.to_string(), v.to_string());
    }
    out
}

fn map_defs<const N: usize>(
    items: [(&str, PredicateDef); N],
) -> std::collections::BTreeMap<String, PredicateDef> {
    let mut out: std::collections::BTreeMap<String, PredicateDef> =
        std::collections::BTreeMap::new();
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
        assert!(text.contains("{year:yyyy}/{month:mm}/{day:dd}/{slug:slug}.md"));
    }
}
