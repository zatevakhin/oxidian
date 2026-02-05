use std::path::PathBuf;

use clap::Parser;
use oxidian::{FrontmatterStatus, Vault, VaultService};

/// Audit frontmatter across the vault.
///
/// Examples:
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --example frontmatter_audit
///   cargo run -p oxidian --example frontmatter_audit -- --vault /path/to/vault
#[derive(Debug, Parser)]
struct Args {
    /// Path to the Obsidian vault.
    #[arg(long, env = "OBSIDIAN_VAULT")]
    vault: PathBuf,

    /// Print paths for notes without frontmatter.
    #[arg(long)]
    show_missing: bool,

    /// Print paths for notes with broken frontmatter.
    #[arg(long)]
    show_broken: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let vault = Vault::open(args.vault)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;
    let snapshot = service.index_snapshot();

    let report = snapshot.frontmatter_report();
    println!("notes_without_frontmatter: {}", report.none);
    println!("notes_with_frontmatter_valid: {}", report.valid);
    println!("notes_with_frontmatter_broken: {}", report.broken);

    if args.show_missing {
        println!("\nmissing:");
        for p in snapshot.notes_without_frontmatter() {
            println!("- {}", p.as_str_lossy());
        }
    }

    if args.show_broken {
        println!("\nbroken:");
        for (p, err) in snapshot.notes_with_broken_frontmatter() {
            println!("- {}\t{}", p.as_str_lossy(), err);
        }
    }

    // Example of per-note inspection:
    let _ = snapshot
        .all_files()
        .filter_map(|f| snapshot.note(&f.path))
        .find(|n| matches!(n.frontmatter, FrontmatterStatus::Broken { .. }));

    Ok(())
}
