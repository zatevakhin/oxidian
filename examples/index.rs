use std::path::PathBuf;

use clap::Parser;
use oxidian::{FileKind, Tag, Vault, VaultService};

/// Build an index and print basic stats.
///
/// Examples:
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --example index
///   cargo run -p oxidian --example index -- --vault /path/to/vault
///   cargo run -p oxidian --example index -- --vault /path/to/vault --tag project/x
#[derive(Debug, Parser)]
struct Args {
    /// Path to the Obsidian vault.
    #[arg(long, env = "OBSIDIAN_VAULT")]
    vault: PathBuf,

    /// Optional tag to query for matching files.
    #[arg(long)]
    tag: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let vault = Vault::open(args.vault)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;
    let snapshot = service.index_snapshot();

    let file_count = snapshot.all_files().count();
    let note_count = snapshot
        .all_files()
        .filter(|f| matches!(f.kind, FileKind::Markdown | FileKind::Canvas))
        .count();
    let tag_count = snapshot.all_tags().count();

    println!("files: {file_count}");
    println!("notes: {note_count}");
    println!("tags: {tag_count}");

    if let Some(tag) = args.tag {
        let tag = normalize_tag_for_query(&tag)?;
        println!("\nfiles with tag #{tag}:");
        for p in snapshot.files_with_tag(&Tag(tag.clone())) {
            println!("- {}", p.as_str_lossy());
        }
    }

    Ok(())
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
