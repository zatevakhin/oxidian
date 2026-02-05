use std::path::PathBuf;

use clap::Parser;
use oxidian::{Tag, Vault, VaultService};

/// Print tags with file counts.
///
/// Examples:
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --example tags
///   cargo run -p oxidian --example tags -- --vault /path/to/vault --top 100
#[derive(Debug, Parser)]
struct Args {
    /// Path to the Obsidian vault.
    #[arg(long, env = "OBSIDIAN_VAULT")]
    vault: PathBuf,

    /// How many tags to print.
    #[arg(long, default_value_t = 50)]
    top: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let vault = Vault::open(args.vault)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;
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

    for (tag, n) in rows.into_iter().take(args.top) {
        println!("{n}\t#{tag}", tag = tag.0);
    }

    Ok(())
}
