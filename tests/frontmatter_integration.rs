use oxidian::{FrontmatterStatus, Vault, VaultService};

#[tokio::test]
async fn frontmatter_audit_detects_none_valid_broken() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let vault_root = temp.path().join("vault");
    std::fs::create_dir_all(vault_root.join("notes"))?;

    std::fs::write(vault_root.join("notes/none.md"), "# No frontmatter\nbody\n")?;

    std::fs::write(
        vault_root.join("notes/valid.md"),
        "---\ntags: [a]\n---\n\n# Valid\n",
    )?;

    std::fs::write(
        vault_root.join("notes/broken.md"),
        "---\ntags: [a\n---\n\n# Broken\n",
    )?;

    let vault = Vault::open(&vault_root)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;
    let snapshot = service.index_snapshot();

    let report = snapshot.frontmatter_report();
    assert_eq!(report.none, 1);
    assert_eq!(report.valid, 1);
    assert_eq!(report.broken, 1);

    let broken: Vec<_> = snapshot
        .notes_with_broken_frontmatter()
        .map(|(p, _)| p.as_str_lossy())
        .collect();
    assert_eq!(broken, vec!["notes/broken.md"]);

    let valid_note = snapshot
        .notes_with_frontmatter()
        .find(|p| p.as_str_lossy() == "notes/valid.md")
        .and_then(|p| snapshot.note(p))
        .expect("valid note present");
    assert!(matches!(valid_note.frontmatter, FrontmatterStatus::Valid));

    Ok(())
}
