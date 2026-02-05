use std::path::PathBuf;

use clap::Parser;
#[cfg(feature = "sqlite")]
use oxidian::{Vault, VaultEvent, VaultService};

/// Build the index, persist it to SQLite, then watch and incrementally update.
///
/// Examples:
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --features sqlite --example sqlite_persist
///   cargo run -p oxidian --features sqlite --example sqlite_persist -- --vault /path/to/vault --db /tmp/obsidian.sqlite
#[derive(Debug, Parser)]
struct Args {
    /// Path to the Obsidian vault.
    #[arg(long, env = "OBSIDIAN_VAULT")]
    vault: PathBuf,

    /// Optional SQLite DB path.
    #[arg(long)]
    db: Option<PathBuf>,
}

#[cfg(not(feature = "sqlite"))]
fn main() {
    eprintln!("This example requires --features sqlite");
}

#[cfg(feature = "sqlite")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use oxidian::SqliteIndexStore;

    let args = Args::parse();

    let vault = Vault::open(args.vault)?;
    let mut service = VaultService::new(vault)?;
    service.build_index().await?;

    let mut store = match args.db {
        Some(p) => SqliteIndexStore::open_path(p)?,
        None => SqliteIndexStore::open_default(service.vault())?,
    };
    store.write_full_index(service.vault(), &service.index_snapshot())?;
    let (files, notes, tags, tasks, links) = store.counts()?;
    println!("persisted: files={files} notes={notes} tags={tags} tasks={tasks} links={links}");

    let mut rx = service.subscribe();
    service.start_watching().await?;
    println!("watching... (Ctrl-C to stop)");

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            ev = rx.recv() => {
                let Ok(ev) = ev else { continue; };
                match ev {
                    VaultEvent::Indexed { path, .. } => {
                        let snap = service.index_snapshot();
                        store.upsert_path(service.vault(), &snap, &path)?;
                    }
                    VaultEvent::Removed { path, .. } => {
                        store.remove_path(&path)?;
                    }
                    VaultEvent::Renamed { from, to, .. } => {
                        store.remove_path(&from)?;
                        let snap = service.index_snapshot();
                        store.upsert_path(service.vault(), &snap, &to)?;
                    }
                    VaultEvent::Error { .. } => {}
                }
            }
        }
    }

    service.shutdown().await;
    Ok(())
}
