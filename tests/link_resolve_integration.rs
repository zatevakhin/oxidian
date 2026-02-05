use oxidian::{LinkResolver, ResolveResult, Vault, VaultPath, VaultService};

#[tokio::test]
async fn resolver_prefers_same_folder_and_supports_aliases() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let vault_root = temp.path().join("vault");
    std::fs::create_dir_all(vault_root.join("notes"))?;
    std::fs::create_dir_all(vault_root.join("other"))?;

    // Two dup notes in different folders.
    std::fs::write(vault_root.join("notes/dup.md"), "# dup\n")?;
    std::fs::write(vault_root.join("other/dup.md"), "# dup\n")?;

    // Aliased note.
    std::fs::write(
        vault_root.join("notes/Target.md"),
        "---\naliases: [AltName]\n---\n\n# Target\n",
    )?;

    // Source in notes folder.
    std::fs::write(vault_root.join("notes/source.md"), "[[dup]] [[AltName]]\n")?;

    let vault = Vault::open(&vault_root)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;
    let idx = service.index_snapshot();
    let resolver: LinkResolver = idx.link_resolver();

    let source = VaultPath::try_from(std::path::Path::new("notes/source.md"))?;
    match resolver.resolve_internal("dup", &source) {
        ResolveResult::Resolved(p) => assert_eq!(p.as_str_lossy(), "notes/dup.md"),
        other => anyhow::bail!("expected resolved dup; got {other:?}"),
    }

    match resolver.resolve_internal("AltName", &source) {
        ResolveResult::Resolved(p) => assert_eq!(p.as_str_lossy(), "notes/Target.md"),
        other => anyhow::bail!("expected resolved alias; got {other:?}"),
    }

    Ok(())
}
