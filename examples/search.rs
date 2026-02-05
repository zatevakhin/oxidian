use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use oxidian::{Vault, VaultService};

/// Fuzzy search by filename or note content.
///
/// Examples:
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --example search -- --mode filename --query pref
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --example search -- --mode content --query blueberry
///   cargo run -p oxidian --example search -- --vault /path/to/vault --mode content --query "meeting notes"
#[derive(Debug, Clone, Copy, ValueEnum)]
enum Mode {
    Filename,
    Content,
}

#[derive(Debug, Parser)]
struct Args {
    /// Path to the Obsidian vault.
    #[arg(long, env = "OBSIDIAN_VAULT")]
    vault: PathBuf,

    /// Search mode.
    #[arg(long, value_enum, default_value_t = Mode::Filename)]
    mode: Mode,

    /// Query string.
    #[arg(long)]
    query: String,

    /// Maximum number of results.
    #[arg(long, default_value_t = 20)]
    limit: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let vault = Vault::open(args.vault)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;

    match args.mode {
        Mode::Filename => {
            let hits = service.search_filenames_fuzzy(&args.query, args.limit);
            for hit in hits {
                println!("{}\t{}", hit.score, hit.path.as_str_lossy());
            }
        }
        Mode::Content => {
            let hits = service
                .search_content_fuzzy(&args.query, args.limit)
                .await?;
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
    }

    Ok(())
}
