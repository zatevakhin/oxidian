use std::path::PathBuf;

use clap::Parser;

#[cfg(feature = "similarity")]
use oxidian::{Vault, VaultConfig, VaultPath, VaultService};

/// Note similarity report and per-note neighbors.
///
/// Examples:
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --example similarity -- --top-k 5
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --example similarity -- --note "notes/a.md"
#[derive(Debug, Parser)]
struct Args {
    /// Path to the Obsidian vault.
    #[arg(long, env = "OBSIDIAN_VAULT")]
    vault: PathBuf,

    /// Relative note path to list neighbors for.
    #[arg(long)]
    note: Option<PathBuf>,

    /// Minimum similarity score.
    #[arg(long)]
    min_score: Option<f32>,

    /// Maximum neighbors per note.
    #[arg(long)]
    top_k: Option<usize>,
}

#[cfg(feature = "similarity")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("oxidian=info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let mut cfg = VaultConfig::default();
    if let Some(score) = args.min_score {
        cfg.similarity_min_score = score;
    }
    if let Some(top_k) = args.top_k {
        cfg.similarity_top_k = top_k;
    }

    let vault = Vault::with_config(args.vault, cfg)?;
    let service = VaultService::new(vault)?;
    eprintln!("building index...");
    service.build_index().await?;
    eprintln!("index ready");

    if let Some(note) = args.note {
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
    } else {
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

    Ok(())
}

#[cfg(not(feature = "similarity"))]
fn main() {
    eprintln!(
        "example requires the similarity feature: cargo run -p oxidian --example similarity --features similarity"
    );
}
