use oxidian::{TaskQuery, TaskStatus, Vault, VaultService};

#[tokio::test]
async fn tasks_are_indexed_and_queryable() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let vault_root = temp.path().join("vault");
    std::fs::create_dir_all(vault_root.join("notes"))?;

    std::fs::write(
        vault_root.join("notes/a.md"),
        "- [ ] buy milk\n- [x] paid rent\n- [>] writing\n- [-] canceled plan\n- [?] blocked by something\n",
    )?;
    std::fs::write(vault_root.join("notes/b.md"), "no tasks here\n")?;

    let vault = Vault::open(&vault_root)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;

    let q = TaskQuery::all().status(TaskStatus::Todo);
    let hits = service.query_tasks(&q);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path.as_str_lossy(), "notes/a.md");
    assert_eq!(hits[0].text, "buy milk");

    let q = TaskQuery::all().contains_text("rent");
    let hits = service.query_tasks(&q);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].status, TaskStatus::Done);

    let q = TaskQuery::all().status(TaskStatus::Blocked);
    let hits = service.query_tasks(&q);
    assert_eq!(hits.len(), 1);
    assert!(hits[0].text.contains("blocked"));

    Ok(())
}
