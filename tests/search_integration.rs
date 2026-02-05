use std::time::Duration;

use oxidian::{Vault, VaultConfig, VaultService};

#[tokio::test]
async fn fuzzy_search_filenames_and_content_work() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let vault_root = temp.path().join("vault");
    std::fs::create_dir_all(&vault_root)?;

    std::fs::create_dir_all(vault_root.join("notes"))?;
    std::fs::write(
        vault_root.join("notes/alpha.md"),
        "this line contains blueberry pie\n",
    )?;
    std::fs::write(
        vault_root.join("notes/beta.md"),
        "this line contains strawberry jam\n",
    )?;

    let mut cfg = VaultConfig::default();
    cfg.watch_debounce = Duration::from_millis(50);
    let vault = Vault::with_config(&vault_root, cfg)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;

    let hits = service.search_filenames_fuzzy("alp", 10);
    assert!(!hits.is_empty());
    assert_eq!(hits[0].path.as_str_lossy(), "notes/alpha.md");

    let hits = service.search_content_fuzzy("blueb", 10).await?;
    assert!(!hits.is_empty());
    assert_eq!(hits[0].path.as_str_lossy(), "notes/alpha.md");
    assert!(hits[0].line >= 1);
    assert!(hits[0].line_text.to_lowercase().contains("blueberry"));

    Ok(())
}
