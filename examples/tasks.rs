use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use oxidian::{TaskQuery, TaskStatus, Vault, VaultService};

/// List indexed tasks.
///
/// Examples:
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --example tasks
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --example tasks -- --status todo
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --example tasks -- --contains birthday --limit 50
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

#[derive(Debug, Parser)]
struct Args {
    /// Path to the Obsidian vault.
    #[arg(long, env = "OBSIDIAN_VAULT")]
    vault: PathBuf,

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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let vault = Vault::open(args.vault)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;

    let mut q = TaskQuery::all();
    if let Some(prefix) = args.prefix {
        q = q.from_path_prefix(prefix);
    }
    if let Some(status) = args.status {
        q = q.status(status.into());
    }
    if let Some(needle) = args.contains {
        q = q.contains_text(needle);
    }
    q = q.limit(args.limit);

    for hit in service.query_tasks(&q) {
        println!(
            "{:?}\t{}:{}\t{}",
            hit.status,
            hit.path.as_str_lossy(),
            hit.line,
            hit.text
        );
    }

    Ok(())
}
