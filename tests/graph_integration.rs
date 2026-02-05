use oxidian::{ResolveResult, Vault, VaultService};

#[tokio::test]
async fn graph_build_collects_backlinks_and_issues() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let vault_root = temp.path().join("vault");
    std::fs::create_dir_all(vault_root.join("a"))?;
    std::fs::create_dir_all(vault_root.join("b"))?;
    std::fs::create_dir_all(vault_root.join("c"))?;

    std::fs::write(vault_root.join("a/dup.md"), "# A\n")?;
    std::fs::write(vault_root.join("b/dup.md"), "# B\n")?;
    std::fs::write(vault_root.join("c/Target.md"), "# Target\n")?;
    std::fs::write(
        vault_root.join("c/source.md"),
        "[[Target]]\n[[Missing]]\n[[dup]]\n",
    )?;

    let vault = Vault::open(&vault_root)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;

    let graph = service.build_graph()?;
    assert_eq!(graph.backlinks.unresolved, 1);
    assert_eq!(graph.backlinks.ambiguous, 1);

    assert_eq!(graph.unresolved().count(), 1);
    assert_eq!(graph.ambiguous().count(), 1);

    // Backlinks for Target should include source.
    let idx = service.index_snapshot();
    let target = idx
        .all_files()
        .find(|f| f.path.as_str_lossy() == "c/Target.md")
        .map(|f| f.path.clone())
        .unwrap();
    let bl = graph.backlinks(&target);
    assert_eq!(bl.len(), 1);
    assert_eq!(bl[0].source.as_str_lossy(), "c/source.md");

    // Outgoing resolutions should reflect missing/ambiguous.
    let source = oxidian::VaultPath::try_from(std::path::Path::new("c/source.md"))?;
    let outgoing = idx.resolved_outgoing_internal_links(&source);
    assert_eq!(outgoing.len(), 3);
    assert!(
        outgoing
            .iter()
            .any(|o| matches!(o.resolution, ResolveResult::Missing))
    );
    assert!(
        outgoing
            .iter()
            .any(|o| matches!(o.resolution, ResolveResult::Ambiguous(_)))
    );

    Ok(())
}
