use std::path::PathBuf;

use clap::Parser;
use oxidian::{Vault, VaultPath, VaultService};

/// Show resolved inbound links (backlinks).
///
/// Examples:
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --example backlinks -- --note notes/Target.md
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --example backlinks -- --note Target
#[derive(Debug, Parser)]
struct Args {
    /// Path to the Obsidian vault.
    #[arg(long, env = "OBSIDIAN_VAULT")]
    vault: PathBuf,

    /// Target note path (relative) or name to resolve.
    #[arg(long)]
    note: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let vault = Vault::open(args.vault)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;

    let backlinks = service.build_backlinks()?;

    // Target resolution for the example:
    // - if input looks like a relative path, use it
    // - else match by note stem (case-insensitive); error on ambiguity
    let idx = service.index_snapshot();
    let target: VaultPath = if args.note.contains('/') || args.note.contains('.') {
        VaultPath::try_from(std::path::Path::new(&args.note))?
    } else {
        let needle = args.note.to_lowercase();
        let mut matches = Vec::new();
        for f in idx.all_files() {
            let Some(_note) = idx.note(&f.path) else {
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
            0 => return Err(anyhow::anyhow!("could not resolve target: {}", args.note)),
            1 => matches.remove(0),
            _ => {
                return Err(anyhow::anyhow!(
                    "ambiguous target '{}': {:?}",
                    args.note,
                    matches
                ));
            }
        }
    };

    println!("target: {}", target.as_str_lossy());
    let items = backlinks.backlinks(&target);
    println!("backlinks: {}", items.len());
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
        "\nunresolved_internal_occurrences: {}",
        backlinks.unresolved
    );
    println!("ambiguous_internal_occurrences: {}", backlinks.ambiguous);

    Ok(())
}
