use oxidian::{LinkTarget, Vault, VaultService};

#[tokio::test]
async fn backlinks_are_built_from_resolved_internal_links() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let vault_root = temp.path().join("vault");
    std::fs::create_dir_all(vault_root.join("notes"))?;

    std::fs::write(vault_root.join("notes/B.md"), "# B\n")?;
    std::fs::write(
        vault_root.join("notes/A.md"),
        "[[B]]\n[md](B.md)\n![[B]]\n[[Missing]]\n",
    )?;

    let vault = Vault::open(&vault_root)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;

    let idx = service.index_snapshot();
    let b_path = idx
        .all_files()
        .find(|f| f.path.as_str_lossy() == "notes/B.md")
        .map(|f| f.path.clone())
        .expect("B present");

    let backlinks = service.build_backlinks()?;
    let items = backlinks.backlinks(&b_path);
    assert_eq!(items.len(), 3);
    assert!(
        items
            .iter()
            .all(|b| b.source.as_str_lossy() == "notes/A.md")
    );
    assert!(
        items
            .iter()
            .any(|b| matches!(b.link.target, LinkTarget::Internal { .. }))
    );

    Ok(())
}
