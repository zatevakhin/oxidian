use oxidian::{FieldValue, Query, SortDir, Vault, VaultService};

#[tokio::test]
async fn dataview_like_fields_are_indexed_and_queryable() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let vault_root = temp.path().join("vault");
    std::fs::create_dir_all(vault_root.join("notes"))?;

    std::fs::write(
        vault_root.join("notes/a.md"),
        "---\nstatus: done\npriority: 3\n---\n\nproject:: alpha\n",
    )?;
    std::fs::write(
        vault_root.join("notes/b.md"),
        "- [status:: todo]\npriority::1\n",
    )?;
    std::fs::write(vault_root.join("notes/c.md"), "---\nstatus: [\n---\n")?;

    let vault = Vault::open(&vault_root)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;
    let idx = service.index_snapshot();

    let a = idx
        .notes_without_frontmatter()
        .find(|p| p.as_str_lossy() == "notes/b.md");
    assert!(a.is_some(), "expected b.md to have no frontmatter");

    let q = Query::notes().where_field("status").eq("done");
    let hits = idx.query(&q);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path.as_str_lossy(), "notes/a.md");

    let q = Query::notes().where_field("project").eq("alpha");
    let hits = idx.query(&q);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path.as_str_lossy(), "notes/a.md");

    let q = Query::notes().where_field("priority").gt(1.5);
    let hits = idx.query(&q);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path.as_str_lossy(), "notes/a.md");

    let q = Query::notes()
        .sort_by_field("priority", SortDir::Desc)
        .limit(2);
    let hits = idx.query(&q);
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].path.as_str_lossy(), "notes/a.md");
    assert_eq!(hits[1].path.as_str_lossy(), "notes/b.md");

    // Spot-check field values.
    let note_a = idx.note(&hits[0].path).expect("note a present");
    assert_eq!(
        note_a.fields.get("status"),
        Some(&FieldValue::String("done".into()))
    );

    Ok(())
}
