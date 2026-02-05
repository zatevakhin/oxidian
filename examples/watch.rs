use std::path::PathBuf;

use clap::Parser;
use oxidian::{Vault, VaultService};

/// Watch a vault and print indexing events.
///
/// Examples:
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --example watch
///   cargo run -p oxidian --example watch -- --vault /path/to/vault
#[derive(Debug, Parser)]
struct Args {
    /// Path to the Obsidian vault.
    #[arg(long, env = "OBSIDIAN_VAULT")]
    vault: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let vault = Vault::open(args.vault)?;
    let mut service = VaultService::new(vault)?;
    service.build_index().await?;
    let mut rx = service.subscribe();

    service.start_watching().await?;
    println!("watching... (Ctrl-C to stop)");

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            ev = rx.recv() => {
                match ev {
                    Ok(ev) => println!("{ev:?}"),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        eprintln!("(lagged {n} events)");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    service.shutdown().await;
    Ok(())
}
