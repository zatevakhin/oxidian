use oxidian::Vault;
use oxidian::VaultService;

#[tokio::test]
async fn aliases_are_extracted_from_frontmatter() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let vault_root = temp.path().join("vault");
    std::fs::create_dir_all(vault_root.join("notes"))?;

    std::fs::write(
        vault_root.join("notes/a.md"),
        "---\naliases: [Foo, \"Bar Baz\"]\n---\n\n# A\n",
    )?;
    std::fs::write(
        vault_root.join("notes/b.md"),
        "---\nalias: Hello, World\n---\n\n# B\n",
    )?;

    let vault = Vault::open(&vault_root)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;
    let idx = service.index_snapshot();

    let a = idx
        .note(&oxidian::VaultPath::try_from(std::path::Path::new(
            "notes/a.md",
        ))?)
        .expect("a present");
    assert!(a.aliases.contains("foo"));
    assert!(a.aliases.contains("bar baz"));

    let b = idx
        .note(&oxidian::VaultPath::try_from(std::path::Path::new(
            "notes/b.md",
        ))?)
        .expect("b present");
    assert!(b.aliases.contains("hello, world"));

    Ok(())
}
