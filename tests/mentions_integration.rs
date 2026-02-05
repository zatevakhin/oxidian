use oxidian::{Vault, VaultPath, VaultService};

#[tokio::test]
async fn unlinked_mentions_exclude_links_code_and_frontmatter() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let vault_root = temp.path().join("vault");
    std::fs::create_dir_all(vault_root.join("notes"))?;

    std::fs::write(
        vault_root.join("notes/Target.md"),
        "---\naliases: [Alt]\n---\n\n# Target\n",
    )?;

    std::fs::write(
        vault_root.join("notes/Source.md"),
        "---\nTarget: should_not_count\n---\n\n\
         Here is Target in plain text.\n\
         Here is Alt too.\n\
         Here is a link [[Target]] that should not count.\n\
         And a markdown link [Target](Target.md) that should not count.\n\
         ```\nTarget in code\n```\n",
    )?;

    let vault = Vault::open(&vault_root)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;

    let target = VaultPath::try_from(std::path::Path::new("notes/Target.md"))?;
    let mentions = service.unlinked_mentions(&target, 100).await?;

    let terms: Vec<_> = mentions.iter().map(|m| m.term.as_str()).collect();
    assert!(terms.contains(&"target"));
    assert!(terms.contains(&"alt"));

    // Should not include any lines that are links.
    assert!(!mentions.iter().any(|m| m.line_text.contains("[[Target]]")));
    assert!(
        !mentions
            .iter()
            .any(|m| m.line_text.contains("[Target](Target.md)"))
    );

    Ok(())
}
