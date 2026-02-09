use std::fs;

use oxidian::{SchemaStatus, Vault, VaultIndex};

const SCHEMA_TOML: &str = r#"
version = 1

[node]
types = ["concept", "doc"]

[node.type.docs]
concept = "Concepts"

[predicates.aliases]
requires = "depends_on"

[predicates.depends_on]
description = "A requires B."
domain = ["concept"]
range = ["concept"]
severity = "error"

[vault.layout]
allow_other_dirs = true

[[vault.layout.dirs]]
path = "people"
required = true

[[vault.layout.rules]]
id = "people_notes"
dir = "people"
match = "relpath"
pattern = "^people/[^/]+\\.md$"
severity = "warn"
allow_extensions = ["md"]
"#;

fn write_schema(root: &std::path::Path) {
    let dir = root.join(".obsidian/oxidian");
    fs::create_dir_all(&dir).expect("create schema dir");
    fs::write(dir.join("schema.toml"), SCHEMA_TOML).expect("write schema");
}

fn write_note(root: &std::path::Path, rel: &str, content: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create dir");
    }
    fs::write(path, content).expect("write note");
}

#[test]
fn schema_missing_is_disabled() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("vault");
    fs::create_dir_all(&root).expect("create vault");
    let vault = Vault::open(&root).expect("open vault");

    let index = VaultIndex::build(&vault).expect("build index");
    assert!(matches!(index.schema_status(), SchemaStatus::Disabled));
}

#[test]
fn invalid_node_type_is_reported() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("vault");
    fs::create_dir_all(&root).expect("create vault");
    write_schema(&root);

    fs::create_dir_all(root.join("people")).expect("create people dir");
    write_note(
        &root,
        "notes/a.md",
        "---\n\
         type: unknown\n\
         ---\n\
         body\n",
    );

    let vault = Vault::open(&root).expect("open vault");
    let index = VaultIndex::build(&vault).expect("build index");
    let report = index.schema_report();

    assert!(report.errors > 0);
    assert!(
        report
            .violations
            .iter()
            .any(|v| v.violation.code == "node_type_unknown")
    );
}

#[test]
fn layout_rule_mismatch_is_reported() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("vault");
    fs::create_dir_all(&root).expect("create vault");
    write_schema(&root);

    write_note(&root, "people/nested/a.md", "hello");

    let vault = Vault::open(&root).expect("open vault");
    let index = VaultIndex::build(&vault).expect("build index");
    let report = index.schema_report();

    assert!(report.warnings > 0);
    assert!(
        report
            .violations
            .iter()
            .any(|v| v.violation.code == "layout_rule_mismatch")
    );
}

#[test]
fn unknown_predicate_link_is_reported_as_warning() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("vault");
    fs::create_dir_all(&root).expect("create vault");
    write_schema(&root);

    write_note(
        &root,
        "notes/a.md",
        "---\n\
         type: concept\n\
         ---\n\
         depends_on:: [[Target]]\n\
         unknown_predicate:: [[Target]]\n",
    );

    let vault = Vault::open(&root).expect("open vault");
    let index = VaultIndex::build(&vault).expect("build index");
    let report = index.schema_report();

    assert!(report.warnings > 0);
    assert!(
        report
            .violations
            .iter()
            .any(|v| v.violation.code == "predicate_unknown")
    );
    assert!(
        report
            .violations
            .iter()
            .any(|v| v.violation.code == "predicate_domain")
            == false
    );
}
