use std::path::PathBuf;

use clap::Parser;
use oxidian::{Vault, VaultPath, VaultService};

/// Find plain-text (unlinked) mentions of a target note.
///
/// Examples:
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --example mentions -- --note notes/Target.md
#[derive(Debug, Parser)]
struct Args {
    /// Path to the Obsidian vault.
    #[arg(long, env = "OBSIDIAN_VAULT")]
    vault: PathBuf,

    /// Target note path (relative to vault).
    #[arg(long)]
    note: PathBuf,

    /// Maximum number of results.
    #[arg(long, default_value_t = 100)]
    limit: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let vault = Vault::open(args.vault)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;

    let target = VaultPath::try_from(args.note.as_path())?;
    let mentions = service.unlinked_mentions(&target, args.limit).await?;
    println!("mentions: {}", mentions.len());
    for m in mentions {
        println!(
            "- {}:{}\tterm={:?}\t{}",
            m.source.as_str_lossy(),
            m.line,
            m.term,
            m.line_text.trim()
        );
    }

    Ok(())
}
