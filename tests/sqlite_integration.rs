#![cfg(feature = "sqlite")]

use oxidian::{SqliteIndexStore, Vault, VaultService};

#[tokio::test]
async fn sqlite_store_can_persist_full_index() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let vault_root = temp.path().join("vault");
    std::fs::create_dir_all(vault_root.join("notes"))?;
    std::fs::write(
        vault_root.join("notes/a.md"),
        "---\ntags: [a]\naliases: [Alt]\n---\n\n- [ ] task\n[[Link]]\nfield:: value\n",
    )?;

    let vault = Vault::open(&vault_root)?;
    let service = VaultService::new(vault)?;
    service.build_index().await?;

    let db_path = temp.path().join("idx.sqlite");
    let mut store = SqliteIndexStore::open_path(&db_path)?;
    store.write_full_index(service.vault(), &service.index_snapshot())?;

    let (files, notes, tags, tasks, links) = store.counts()?;
    assert!(files >= 1);
    assert!(notes >= 1);
    assert!(tags >= 1);
    assert!(tasks >= 1);
    assert!(links >= 1);

    Ok(())
}
