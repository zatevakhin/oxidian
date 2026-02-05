use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use oxidian::{Link, LinkKind, Vault, VaultPath, VaultService};

/// Inspect parsed links (unique targets + typed occurrences).
///
/// Examples:
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --example links
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --example links -- --note indexes/preferences.md
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --example links -- --note indexes/preferences.md --kind wiki
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --example links -- --note indexes/preferences.md --only-embeds
#[derive(Debug, Clone, Copy, ValueEnum)]
enum KindArg {
    Wiki,
    Markdown,
    Autourl,
    ObsidianUri,
}

impl From<KindArg> for LinkKind {
    fn from(value: KindArg) -> Self {
        match value {
            KindArg::Wiki => LinkKind::Wiki,
            KindArg::Markdown => LinkKind::Markdown,
            KindArg::Autourl => LinkKind::AutoUrl,
            KindArg::ObsidianUri => LinkKind::ObsidianUri,
        }
    }
}

#[derive(Debug, Parser)]
struct Args {
    /// Path to the Obsidian vault.
    #[arg(long, env = "OBSIDIAN_VAULT")]
    vault: PathBuf,

    /// Optional note path (relative to vault) to inspect.
    #[arg(long)]
    note: Option<PathBuf>,

    /// Filter by link kind.
    #[arg(long, value_enum)]
    kind: Option<KindArg>,

    /// Only show embed links (e.g. ![[..]] or ![](..)).
    #[arg(long)]
    only_embeds: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let vault = Vault::open(args.vault)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;
    let idx = service.index_snapshot();

    if let Some(note) = args.note {
        let rel = VaultPath::try_from(note.as_path())?;
        let note = idx
            .note(&rel)
            .ok_or_else(|| anyhow::anyhow!("note not found: {}", rel.as_str_lossy()))?;

        println!("note: {}", rel.as_str_lossy());
        println!("unique_targets: {}", note.links.len());
        println!("occurrences: {}", note.link_occurrences.len());

        if !note.links.is_empty() {
            println!("\nunique targets:");
            for t in &note.links {
                println!("- {t:?}");
            }
        }

        let kind_filter = args.kind.map(Into::into);
        let occs = note
            .link_occurrences
            .iter()
            .filter(|l| kind_filter.as_ref().map_or(true, |k| &l.kind == k))
            .filter(|l| !args.only_embeds || l.embed);

        println!("\noccurrences:");
        for l in occs {
            print_occ(l);
        }

        return Ok(());
    }

    // Summary across all notes.
    let mut total = 0usize;
    let mut wiki = 0usize;
    let mut md = 0usize;
    let mut auto = 0usize;
    let mut obs_uri = 0usize;
    let mut embeds = 0usize;

    for f in idx.all_files() {
        let Some(note) = idx.note(&f.path) else {
            continue;
        };
        for l in &note.link_occurrences {
            total += 1;
            if l.embed {
                embeds += 1;
            }
            match l.kind {
                LinkKind::Wiki => wiki += 1,
                LinkKind::Markdown => md += 1,
                LinkKind::AutoUrl => auto += 1,
                LinkKind::ObsidianUri => obs_uri += 1,
            }
        }
    }

    println!("occurrences_total: {total}");
    println!("occurrences_embeds: {embeds}");
    println!("occurrences_wiki: {wiki}");
    println!("occurrences_markdown: {md}");
    println!("occurrences_autourl: {auto}");
    println!("occurrences_obsidian_uri: {obs_uri}");

    Ok(())
}

fn print_occ(l: &Link) {
    println!(
        "- {:?}\tembed={}\t{}:{}\ttarget={:?}\tsubpath={:?}\tdisplay={:?}\traw={:?}",
        l.kind, l.embed, l.location.line, l.location.column, l.target, l.subpath, l.display, l.raw
    );
}
