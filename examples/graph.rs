use std::path::PathBuf;

use clap::Parser;
use oxidian::{ResolveResult, Vault, VaultPath, VaultService};

/// Build a resolved link graph and inspect it.
///
/// Examples:
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --example graph -- --note notes/source.md
#[derive(Debug, Parser)]
struct Args {
    /// Path to the Obsidian vault.
    #[arg(long, env = "OBSIDIAN_VAULT")]
    vault: PathBuf,

    /// Source note path (relative to vault) to show outgoing internal links.
    #[arg(long)]
    note: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let vault = Vault::open(args.vault)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;

    let graph = service.build_graph()?;
    println!(
        "unresolved_internal_occurrences: {}",
        graph.backlinks.unresolved
    );
    println!(
        "ambiguous_internal_occurrences: {}",
        graph.backlinks.ambiguous
    );
    println!("issue_count: {}", graph.issues.len());

    if let Some(note) = args.note {
        let source = VaultPath::try_from(note.as_path())?;
        let idx = service.index_snapshot();
        let outgoing = idx.resolved_outgoing_internal_links(&source);
        println!("\nsource: {}", source.as_str_lossy());
        for o in outgoing {
            match &o.resolution {
                ResolveResult::Resolved(p) => {
                    println!(
                        "- {}:{}\tresolved\t{}\traw={:?}",
                        o.source.as_str_lossy(),
                        o.link.location.line,
                        p.as_str_lossy(),
                        o.link.raw
                    );
                }
                ResolveResult::Missing => {
                    println!(
                        "- {}:{}\tmissing\t{:?}\traw={:?}",
                        o.source.as_str_lossy(),
                        o.link.location.line,
                        o.link.target,
                        o.link.raw
                    );
                }
                ResolveResult::Ambiguous(cands) => {
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

    Ok(())
}
