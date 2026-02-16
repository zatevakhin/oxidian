#![cfg(feature = "similarity")]

use oxidian::{Vault, VaultConfig, VaultPath, VaultService};

#[tokio::test]
async fn similarity_reports_identical_notes_as_top_hit() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let vault_root = temp.path().join("vault");
    std::fs::create_dir_all(vault_root.join("notes"))?;

    std::fs::write(
        vault_root.join("notes/a.md"),
        "# Test\nApple banana orange.\n",
    )?;
    std::fs::write(
        vault_root.join("notes/b.md"),
        "# Test\nApple banana orange.\n",
    )?;
    std::fs::write(
        vault_root.join("notes/c.md"),
        "# Different\nZebra yurt quantum.\n",
    )?;

    let mut cfg = VaultConfig::default();
    cfg.similarity_min_score = 0.8;
    cfg.similarity_top_k = 3;
    cfg.similarity_max_notes = 100;
    cfg.embedding_cache_dir = temp.path().join("embeddings");

    let vault = Vault::with_config(&vault_root, cfg)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;

    let a_path = VaultPath::try_from(std::path::Path::new("notes/a.md"))?;
    let hits = service.note_similarity_for(&a_path)?;
    assert!(!hits.is_empty());
    assert_eq!(hits[0].target.as_str_lossy(), "notes/b.md");
    assert!(hits[0].score >= 0.8);

    Ok(())
}

#[tokio::test]
async fn semantic_search_ranks_related_notes_first() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let vault_root = temp.path().join("vault");
    std::fs::create_dir_all(vault_root.join("notes"))?;

    std::fs::write(
        vault_root.join("notes/a.md"),
        "# Test\nApple banana orange.\n",
    )?;
    std::fs::write(
        vault_root.join("notes/b.md"),
        "# Test\nApple banana orange.\n",
    )?;
    std::fs::write(
        vault_root.join("notes/c.md"),
        "# Different\nZebra yurt quantum.\n",
    )?;

    let mut cfg = VaultConfig::default();
    cfg.similarity_min_score = 0.0;
    cfg.similarity_top_k = 3;
    cfg.similarity_max_notes = 100;
    cfg.embedding_cache_dir = temp.path().join("embeddings");

    let vault = Vault::with_config(&vault_root, cfg)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;

    let hits = service
        .search_content_semantic("apple banana orange", 3)
        .await?;
    assert!(!hits.is_empty());

    let top = hits[0].path.as_str_lossy();
    assert!(top == "notes/a.md" || top == "notes/b.md");

    for pair in hits.windows(2) {
        assert!(pair[0].score >= pair[1].score);
    }
    for hit in &hits {
        assert!((0.0..=1.0).contains(&hit.score));
    }

    Ok(())
}
