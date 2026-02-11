use std::fs;

use oxidian::{SchemaStatus, Vault, VaultIndex};

fn write_schema(root: &std::path::Path, schema: &str) {
    let dir = root.join(".obsidian/oxidian");
    fs::create_dir_all(&dir).expect("create schema dir");
    fs::write(dir.join("schema.toml"), schema).expect("write schema");
}

fn write_note(root: &std::path::Path, rel: &str, content: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create dir");
    }
    fs::write(path, content).expect("write note");
}

fn base_schema() -> String {
    r#"
version = 1

[node]
types = ["concept", "journal", "memory", "event", "quote", "decision", "fact", "preference"]

[node.type.docs]
concept = "Concepts"
journal = "Journal entries"
memory = "Memory entries"
event = "Memory event"
quote = "Memory quote"
decision = "Memory decision"
fact = "Memory fact"
preference = "Memory preference"

[predicates.aliases]
requires = "depends_on"

[predicates.depends_on]
description = "A requires B."
domain = ["concept"]
severity = "error"

[vault]
scope_resolution = "most_specific"
unscoped = "allow"
"#
    .to_string()
}

#[test]
fn scope_unmatched_is_reported() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("vault");
    fs::create_dir_all(&root).expect("create vault");

    let schema = format!(
        "{}\n\n[[vault.scopes]]\n\
         id = \"people\"\n\
         path = \"people\"\n\
         required = true\n\
         unmatched_files = \"warn\"\n\
         \n\
         [[vault.scopes.allow]]\n\
         id = \"people_md\"\n\
         glob = \"**/*.md\"\n",
        base_schema()
    );

    write_schema(&root, &schema);
    write_note(&root, "people/readme.txt", "hello");

    let vault = Vault::open(&root).expect("open vault");
    let index = VaultIndex::build(&vault).expect("build index");
    let report = index.schema_report();

    assert!(report.warnings > 0);
    assert!(
        report
            .violations
            .iter()
            .any(|v| v.violation.code == "layout_unmatched")
    );
}

#[test]
fn deny_rule_is_reported() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("vault");
    fs::create_dir_all(&root).expect("create vault");

    let schema = format!(
        "{}\n\n[[vault.scopes]]\n\
         id = \"projects\"\n\
         path = \"projects\"\n\
         required = true\n\
         unmatched_files = \"warn\"\n\
         \n\
         [[vault.scopes.allow]]\n\
         id = \"projects_md\"\n\
         glob = \"**/*.md\"\n\
         \n\
         [[vault.scopes.deny]]\n\
         id = \"blocked\"\n\
         glob = \"secret.md\"\n\
         severity = \"error\"\n",
        base_schema()
    );

    write_schema(&root, &schema);
    write_note(&root, "projects/secret.md", "body");

    let vault = Vault::open(&root).expect("open vault");
    let index = VaultIndex::build(&vault).expect("build index");
    let report = index.schema_report();

    assert!(report.errors > 0);
    assert!(
        report
            .violations
            .iter()
            .any(|v| v.violation.code == "layout_denied")
    );
}

#[test]
fn inherit_allow_applies_to_nested_scope() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("vault");
    fs::create_dir_all(&root).expect("create vault");

    let schema = format!(
        "{}\n\n[[vault.scopes]]\n\
         id = \"kg\"\n\
         path = \"kg\"\n\
         required = true\n\
         unmatched_files = \"warn\"\n\
         \n\
         [[vault.scopes.allow]]\n\
         id = \"kg_md\"\n\
         glob = \"**/*.md\"\n\
         \n\
         [[vault.scopes]]\n\
         id = \"kg_concepts\"\n\
         path = \"kg/concepts\"\n\
         required = true\n\
         inherit_allow = true\n\
         unmatched_files = \"error\"\n",
        base_schema()
    );

    write_schema(&root, &schema);
    write_note(&root, "kg/concepts/a.md", "body");

    let vault = Vault::open(&root).expect("open vault");
    let index = VaultIndex::build(&vault).expect("build index");
    let report = index.schema_report();

    assert!(report.violations.is_empty());
}

#[test]
fn template_match_enforces_repeated_vars() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("vault");
    fs::create_dir_all(&root).expect("create vault");

    let schema = format!(
        r#"{}

[[vault.scopes]]
id = "journal"
path = "journal"
required = true
unmatched_files = "error"

[[vault.scopes.allow]]
id = "journal_entry"
template = "{{year:yyyy}}/{{year:yyyy}}-{{month:mm}}-{{day:dd}}.md"
"#,
        base_schema()
    );

    write_schema(&root, &schema);
    write_note(&root, "journal/2026/2027-02-09.md", "body");

    let vault = Vault::open(&root).expect("open vault");
    let index = VaultIndex::build(&vault).expect("build index");
    let report = index.schema_report();

    assert!(report.errors > 0);
    assert!(
        report
            .violations
            .iter()
            .any(|v| v.violation.code == "layout_unmatched")
    );
}

#[test]
fn scope_note_type_is_required() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("vault");
    fs::create_dir_all(&root).expect("create vault");

    let schema = format!(
        "{}\n\n[[vault.scopes]]\n\
         id = \"journal\"\n\
         path = \"journal\"\n\
         required = true\n\
         unmatched_files = \"warn\"\n\
         \n\
         [[vault.scopes.allow]]\n\
         id = \"journal_entry\"\n\
         glob = \"**/*.md\"\n\
         \n\
         [vault.scopes.notes.type]\n\
         required = true\n\
         allowed = [\"journal\"]\n\
         severity = \"error\"\n",
        base_schema()
    );

    write_schema(&root, &schema);
    write_note(&root, "journal/2026-02-09.md", "---\n---\nbody\n");

    let vault = Vault::open(&root).expect("open vault");
    let index = VaultIndex::build(&vault).expect("build index");
    let report = index.schema_report();

    assert!(report.errors > 0);
    assert!(
        report
            .violations
            .iter()
            .any(|v| v.violation.code == "note_type_missing")
    );
}

#[test]
fn memory_scope_requires_date_structure() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("vault");
    fs::create_dir_all(&root).expect("create vault");

    let schema = format!(
        r#"{}

[[vault.scopes]]
id = "memory"
path = "memory"
required = true
unmatched_files = "error"

[[vault.scopes.allow]]
id = "memory_entry"
template = "{{year:yyyy}}/{{month:mm}}/{{day:dd}}/{{slug:slug}}.md"

[vault.scopes.notes.type]
required = true
allowed = ["memory", "event", "quote", "decision", "fact", "preference"]
severity = "error"
"#,
        base_schema()
    );

    write_schema(&root, &schema);
    write_note(&root, "memory/2026/02/note.md", "---\n\n---\nbody\n");

    let vault = Vault::open(&root).expect("open vault");
    let index = VaultIndex::build(&vault).expect("build index");
    let report = index.schema_report();

    assert!(report.errors > 0);
    assert!(
        report
            .violations
            .iter()
            .any(|v| v.violation.code == "layout_unmatched")
    );
}

#[test]
fn memory_scope_requires_type() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("vault");
    fs::create_dir_all(&root).expect("create vault");

    let schema = format!(
        r#"{}

[[vault.scopes]]
id = "memory"
path = "memory"
required = true
unmatched_files = "error"

[[vault.scopes.allow]]
id = "memory_entry"
template = "{{year:yyyy}}/{{month:mm}}/{{day:dd}}/{{slug:slug}}.md"

[vault.scopes.notes.type]
required = true
allowed = ["memory", "event", "quote", "decision", "fact", "preference"]
severity = "error"
"#,
        base_schema()
    );

    write_schema(&root, &schema);
    write_note(&root, "memory/2026/02/11/remember.md", "---\n---\nbody\n");

    let vault = Vault::open(&root).expect("open vault");
    let index = VaultIndex::build(&vault).expect("build index");
    let report = index.schema_report();

    assert!(report.errors > 0);
    assert!(
        report
            .violations
            .iter()
            .any(|v| v.violation.code == "note_type_missing")
    );
}

#[test]
fn memory_scope_rejects_disallowed_type() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("vault");
    fs::create_dir_all(&root).expect("create vault");

    let schema = format!(
        r#"{}

[[vault.scopes]]
id = "memory"
path = "memory"
required = true
unmatched_files = "error"

[[vault.scopes.allow]]
id = "memory_entry"
template = "{{year:yyyy}}/{{month:mm}}/{{day:dd}}/{{slug:slug}}.md"

[vault.scopes.notes.type]
required = true
allowed = ["memory", "event", "quote", "decision", "fact", "preference"]
severity = "error"
"#,
        base_schema()
    );

    write_schema(&root, &schema);
    write_note(
        &root,
        "memory/2026/02/11/remember.md",
        "---\n\
         type: concept\n\
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
            .any(|v| v.violation.code == "note_type_mismatch")
    );
}

#[test]
fn memory_scope_requires_slug_format() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("vault");
    fs::create_dir_all(&root).expect("create vault");

    let schema = format!(
        r#"{}

[[vault.scopes]]
id = "memory"
path = "memory"
required = true
unmatched_files = "error"

[[vault.scopes.allow]]
id = "memory_entry"
template = "{{year:yyyy}}/{{month:mm}}/{{day:dd}}/{{slug:slug}}.md"

[vault.scopes.notes.type]
required = true
allowed = ["memory"]
severity = "error"
"#,
        base_schema()
    );

    write_schema(&root, &schema);
    write_note(
        &root,
        "memory/2026/02/11/BadSlug.md",
        "---\n\
         type: memory\n\
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
            .any(|v| v.violation.code == "layout_unmatched")
    );
}

#[test]
fn memory_scope_requires_tag_or_type_match() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("vault");
    fs::create_dir_all(&root).expect("create vault");

    let schema = format!(
        r#"{}

[[vault.scopes]]
id = "memory"
path = "memory"
required = true
unmatched_files = "error"

[[vault.scopes.allow]]
id = "memory_entry"
template = "{{year:yyyy}}/{{month:mm}}/{{day:dd}}/{{slug:slug}}.md"

[vault.scopes.notes.type]
required = true
allowed = ["memory", "event", "quote", "decision", "fact", "preference"]
severity = "error"

[vault.scopes.notes.require_any]
tags = ["event", "quote", "decision", "fact", "preference"]
types = ["event", "quote", "decision", "fact", "preference"]
severity = "error"
"#,
        base_schema()
    );

    write_schema(&root, &schema);
    write_note(
        &root,
        "memory/2026/02/11/remember.md",
        "---\n\
         type: memory\n\
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
            .any(|v| v.violation.code == "note_require_any_missing")
    );
}

#[test]
fn memory_scope_accepts_tag_match() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("vault");
    fs::create_dir_all(&root).expect("create vault");

    let schema = format!(
        r#"{}

[[vault.scopes]]
id = "memory"
path = "memory"
required = true
unmatched_files = "error"

[[vault.scopes.allow]]
id = "memory_entry"
template = "{{year:yyyy}}/{{month:mm}}/{{day:dd}}/{{slug:slug}}.md"

[vault.scopes.notes.type]
required = true
allowed = ["memory", "event", "quote", "decision", "fact", "preference"]
severity = "error"

[vault.scopes.notes.require_any]
tags = ["event", "quote", "decision", "fact", "preference"]
types = ["event", "quote", "decision", "fact", "preference"]
severity = "error"
"#,
        base_schema()
    );

    write_schema(&root, &schema);
    write_note(
        &root,
        "memory/2026/02/11/remember.md",
        "---\n\
         type: memory\n\
         tags: [event]\n\
         ---\n\
         body\n",
    );

    let vault = Vault::open(&root).expect("open vault");
    let index = VaultIndex::build(&vault).expect("build index");
    let report = index.schema_report();

    assert!(report.errors == 0);
    assert!(
        report
            .violations
            .iter()
            .all(|v| v.violation.code != "note_require_any_missing")
    );
}

#[test]
fn orphaned_attachment_is_reported() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("vault");
    fs::create_dir_all(&root).expect("create vault");

    let schema = format!(
        r#"{}

[[vault.scopes]]
id = "memory_assets"
path = "memory/assets"
required = true
unmatched_files = "warn"
kinds = ["attachment"]
orphan_attachments = "warn"

[[vault.scopes.allow]]
id = "assets_any"
glob = "**/*"
"#,
        base_schema()
    );

    write_schema(&root, &schema);
    write_note(&root, "memory/assets/clip.png", "png");

    let vault = Vault::open(&root).expect("open vault");
    let index = VaultIndex::build(&vault).expect("build index");
    let report = index.schema_report();

    assert!(report.warnings > 0);
    assert!(
        report
            .violations
            .iter()
            .any(|v| v.violation.code == "attachment_orphaned")
    );
}

#[test]
fn required_scope_dir_is_reported() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("vault");
    fs::create_dir_all(&root).expect("create vault");

    let schema = format!(
        "{}\n\n[[vault.scopes]]\n\
         id = \"people\"\n\
         path = \"people\"\n\
         required = true\n\
         unmatched_files = \"warn\"\n\
         \n\
         [[vault.scopes.allow]]\n\
         id = \"people_md\"\n\
         glob = \"**/*.md\"\n",
        base_schema()
    );

    write_schema(&root, &schema);

    let vault = Vault::open(&root).expect("open vault");
    let index = VaultIndex::build(&vault).expect("build index");
    let report = index.schema_report();

    assert!(report.errors > 0);
    assert!(
        report
            .violations
            .iter()
            .any(|v| v.violation.code == "layout_dir_missing")
    );
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

    let schema = format!(
        "{}\n\n[[vault.scopes]]\n\
         id = \"notes\"\n\
         path = \"notes\"\n\
         required = true\n\
         unmatched_files = \"allow\"\n",
        base_schema()
    );
    write_schema(&root, &schema);

    fs::create_dir_all(root.join("notes")).expect("create notes dir");
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
fn unknown_predicate_link_is_reported_as_warning() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("vault");
    fs::create_dir_all(&root).expect("create vault");

    let schema = format!(
        "{}\n\n[[vault.scopes]]\n\
         id = \"notes\"\n\
         path = \"notes\"\n\
         required = true\n\
         unmatched_files = \"allow\"\n",
        base_schema()
    );
    write_schema(&root, &schema);

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
