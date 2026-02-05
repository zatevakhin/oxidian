use std::path::PathBuf;

use clap::Parser;
use oxidian::{LinkIssueReason, Vault, VaultService};

/// Audit internal links for missing/ambiguous targets and missing subpaths.
///
/// Examples:
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --example link_audit
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --example link_audit -- --show-broken
#[derive(Debug, Parser)]
struct Args {
    /// Path to the Obsidian vault.
    #[arg(long, env = "OBSIDIAN_VAULT")]
    vault: PathBuf,

    /// Print broken links.
    #[arg(long)]
    show_broken: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let vault = Vault::open(args.vault)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;

    let report = service.link_health_report()?;

    println!(
        "internal_occurrences: {}",
        report.total_internal_occurrences
    );
    println!("ok: {}", report.ok);
    println!("broken: {}", report.broken.len());

    if args.show_broken {
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

    Ok(())
}
