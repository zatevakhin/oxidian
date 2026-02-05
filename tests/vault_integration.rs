use std::path::PathBuf;
use std::time::Duration;

use oxidian::{Tag, Vault, VaultConfig, VaultEvent, VaultService};

#[tokio::test]
async fn vault_service_indexes_and_reindexes_on_change() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let vault_root = temp.path().join("vault");
    std::fs::create_dir_all(&vault_root)?;

    let note_path = vault_root.join("notes/a.md");
    std::fs::create_dir_all(note_path.parent().unwrap())?;
    std::fs::write(
        &note_path,
        "---\ntags: [Foo]\n---\n\n# A\nBody #bar and [[Link]].\n",
    )?;

    let mut cfg = VaultConfig::default();
    cfg.watch_debounce = Duration::from_millis(100);
    let vault = Vault::with_config(&vault_root, cfg)?;

    let mut service = VaultService::new(vault)?;
    service.build_index().await?;

    // Initial index state.
    service.with_index(|idx| {
        let foo = Tag("foo".into());
        let bar = Tag("bar".into());

        let files_with_foo: Vec<_> = idx.files_with_tag(&foo).collect();
        let files_with_bar: Vec<_> = idx.files_with_tag(&bar).collect();
        assert_eq!(files_with_foo.len(), 1);
        assert_eq!(files_with_bar.len(), 1);

        let rel = idx
            .files_with_tag(&foo)
            .next()
            .expect("expected a file tagged foo");
        let note = idx.note(rel).expect("expected note meta");
        assert!(note.links.contains(&oxidian::LinkTarget::Internal {
            reference: "Link".into(),
        }));
        assert!(matches!(
            note.frontmatter,
            oxidian::FrontmatterStatus::Valid
        ));
    });

    // Start watch + subscribe.
    let mut rx = service.subscribe();
    service.start_watching().await?;

    // Update tags in-place.
    std::fs::write(
        &note_path,
        "---\ntags: [Foo]\n---\n\n# A\nBody #baz and [[Link]].\n",
    )?;

    let rel_path = PathBuf::from("notes/a.md");
    let mut saw_reindex = false;

    let deadline = Duration::from_secs(5);
    let mut remaining = deadline;
    while remaining.as_millis() > 0 {
        let start = std::time::Instant::now();
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(ev)) => match ev {
                VaultEvent::Indexed { path, .. } if path.as_path() == rel_path.as_path() => {
                    saw_reindex = true;
                    break;
                }
                _ => {}
            },
            _ => break,
        }
        remaining = remaining.saturating_sub(start.elapsed());
    }

    assert!(saw_reindex, "expected a reindex event for notes/a.md");

    // Updated index state.
    service.with_index(|idx| {
        let foo = Tag("foo".into());
        let bar = Tag("bar".into());
        let baz = Tag("baz".into());

        assert_eq!(idx.files_with_tag(&foo).count(), 1);
        assert_eq!(idx.files_with_tag(&bar).count(), 0);
        assert_eq!(idx.files_with_tag(&baz).count(), 1);
    });

    service.shutdown().await;
    Ok(())
}
