use oxidian::{LinkIssueReason, Vault, VaultService};

#[tokio::test]
async fn link_health_report_finds_missing_ambiguous_and_subpath_issues() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let vault_root = temp.path().join("vault");
    std::fs::create_dir_all(vault_root.join("notes"))?;
    std::fs::create_dir_all(vault_root.join("a"))?;
    std::fs::create_dir_all(vault_root.join("b"))?;
    std::fs::create_dir_all(vault_root.join("other"))?;

    // Targets
    std::fs::write(
        vault_root.join("notes/Target.md"),
        "# Hello World\n\nParagraph ^blk1\n",
    )?;
    std::fs::write(vault_root.join("a/dup.md"), "# A\n")?;
    std::fs::write(vault_root.join("b/dup.md"), "# B\n")?;

    // Source
    std::fs::write(
        vault_root.join("other/source.md"),
        "Links:\n\
         [[Target]]\n\
         [[Target#Hello World]]\n\
         [[Target#Missing]]\n\
         [[Target^blk1]]\n\
         [[Target^nope]]\n\
         [[MissingNote]]\n\
         [[dup]]\n\
         [md](Target.md)\n\
         [uri](obsidian://open?vault=V&file=Target)\n\
         <https://example.com>\n",
    )?;

    let vault = Vault::open(&vault_root)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;

    let report = service.link_health_report()?;
    assert!(report.total_internal_occurrences >= 8);

    // We expect: MissingNote, dup ambiguous, missing heading, missing block.
    let mut missing_target = 0;
    let mut ambiguous = 0;
    let mut missing_heading = 0;
    let mut missing_block = 0;
    for issue in &report.broken {
        match &issue.reason {
            LinkIssueReason::MissingTarget => missing_target += 1,
            LinkIssueReason::AmbiguousTarget { .. } => ambiguous += 1,
            LinkIssueReason::MissingHeading { .. } => missing_heading += 1,
            LinkIssueReason::MissingBlock { .. } => missing_block += 1,
        }
    }

    assert_eq!(missing_target, 1);
    assert_eq!(ambiguous, 1);
    assert_eq!(missing_heading, 1);
    assert_eq!(missing_block, 1);

    Ok(())
}
