use std::path::PathBuf;

use clap::Parser;
use oxidian::{Query, SortDir, Vault, VaultService};

/// Minimal Dataview-like querying via typed, chainable API.
///
/// Examples:
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --example dataview -- --exists status
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --example dataview -- --eq status=done --sort-field priority --desc
///   OBSIDIAN_VAULT=/path/to/vault cargo run -p oxidian --example dataview -- --contains project=alpha --limit 20
#[derive(Debug, Parser)]
struct Args {
    /// Path to the Obsidian vault.
    #[arg(long, env = "OBSIDIAN_VAULT")]
    vault: PathBuf,

    /// Limit results to paths with this prefix.
    #[arg(long)]
    prefix: Option<String>,

    /// Limit results to notes with this tag.
    #[arg(long)]
    tag: Option<String>,

    /// Require that a field exists (repeatable).
    #[arg(long)]
    exists: Vec<String>,

    /// Field equals (repeatable): key=value.
    #[arg(long)]
    eq: Vec<String>,

    /// Field contains substring (repeatable): key=value.
    #[arg(long)]
    contains: Vec<String>,

    /// Field numeric greater-than (repeatable): key=value.
    #[arg(long)]
    gt: Vec<String>,

    /// Sort by field name.
    #[arg(long)]
    sort_field: Option<String>,

    /// Sort descending.
    #[arg(long)]
    desc: bool,

    /// Maximum number of results.
    #[arg(long, default_value_t = 50)]
    limit: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let vault = Vault::open(args.vault)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;

    let mut q = Query::notes();
    if let Some(prefix) = args.prefix {
        q = q.from_path_prefix(prefix);
    }
    if let Some(tag) = args.tag {
        q = q.from_tag(tag);
    }

    for key in args.exists {
        q = q.where_field(key).exists();
    }
    for kv in args.eq {
        let Some((k, v)) = kv.split_once('=') else {
            continue;
        };
        q = q.where_field(k).eq(v);
    }
    for kv in args.contains {
        let Some((k, v)) = kv.split_once('=') else {
            continue;
        };
        q = q.where_field(k).contains(v);
    }
    for kv in args.gt {
        let Some((k, v)) = kv.split_once('=') else {
            continue;
        };
        if let Ok(n) = v.trim().parse::<f64>() {
            q = q.where_field(k).gt(n);
        }
    }

    let dir = if args.desc {
        SortDir::Desc
    } else {
        SortDir::Asc
    };
    if let Some(field) = args.sort_field {
        q = q.sort_by_field(field, dir);
    } else {
        q = q.sort_by_path(dir);
    }
    q = q.limit(args.limit);

    for hit in service.query(&q) {
        println!("{}", hit.path.as_str_lossy());
    }

    Ok(())
}
