use std::fs;
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;

fn cmd() -> Command {
    Command::cargo_bin("oxi").expect("binary exists")
}

fn create_vault(root: &Path) {
    fs::create_dir_all(root.join("notes")).unwrap();
    fs::write(
        root.join("notes/hello.md"),
        "---\ntags: [project, rust]\ntype: doc\ntitle: Hello World\n---\n\n# Hello\n\nSome content about [[other-note]] and #rust.\n\n- [ ] Buy groceries\n- [x] Fix login bug\n",
    )
    .unwrap();
    fs::write(
        root.join("notes/other-note.md"),
        "---\ntags: [rust]\ntitle: Other Note\n---\n\nThis links back to [[hello]].\n",
    )
    .unwrap();
    fs::write(
        root.join("notes/no-frontmatter.md"),
        "# No frontmatter\n\nJust plain content.\n",
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// Help / basic arg parsing
// ---------------------------------------------------------------------------

#[test]
fn help_flag_prints_usage() {
    cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Obsidian vault indexing"))
        .stdout(predicate::str::contains("search"))
        .stdout(predicate::str::contains("query"))
        .stdout(predicate::str::contains("tags"))
        .stdout(predicate::str::contains("tasks"))
        .stdout(predicate::str::contains("links"))
        .stdout(predicate::str::contains("backlinks"))
        .stdout(predicate::str::contains("mentions"))
        .stdout(predicate::str::contains("stats"))
        .stdout(predicate::str::contains("graph"))
        .stdout(predicate::str::contains("check"))
        .stdout(predicate::str::contains("watch"))
        .stdout(predicate::str::contains("persist"))
        .stdout(predicate::str::contains("schema"));
}

#[test]
fn missing_vault_errors() {
    cmd()
        .arg("stats")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--vault is required"));
}

#[test]
fn missing_vault_errors_json() {
    cmd()
        .args(["--output", "json", "stats"])
        .assert()
        .failure()
        .stdout(predicate::str::contains(r#""ok": false"#))
        .stdout(predicate::str::contains("--vault is required"));
}

// ---------------------------------------------------------------------------
// stats
// ---------------------------------------------------------------------------

#[test]
fn stats_text_output() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    cmd()
        .args(["--vault", vault.to_str().unwrap(), "stats"])
        .assert()
        .success()
        .stdout(predicate::str::contains("files:"))
        .stdout(predicate::str::contains("notes:"))
        .stdout(predicate::str::contains("tags:"));
}

#[test]
fn stats_json_output() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    let output = cmd()
        .args([
            "--vault",
            vault.to_str().unwrap(),
            "--output",
            "json",
            "stats",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert!(json["data"]["files"].as_u64().unwrap() >= 3);
    assert!(json["data"]["notes"].as_u64().unwrap() >= 3);
    assert!(json["data"]["tags"].as_u64().unwrap() >= 1);
}

#[test]
fn stats_with_tag_filter_json() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    let output = cmd()
        .args([
            "--vault",
            vault.to_str().unwrap(),
            "-o",
            "json",
            "stats",
            "--tag",
            "rust",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["tag_filter"], "rust");
    assert!(json["data"]["tagged_files"].as_array().unwrap().len() >= 2);
}

// ---------------------------------------------------------------------------
// tags
// ---------------------------------------------------------------------------

#[test]
fn tags_text_output() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    cmd()
        .args(["--vault", vault.to_str().unwrap(), "tags"])
        .assert()
        .success()
        .stdout(predicate::str::contains("#rust"));
}

#[test]
fn tags_json_output() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    let output = cmd()
        .args(["--vault", vault.to_str().unwrap(), "-o", "json", "tags"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    let tags = json["data"].as_array().unwrap();
    assert!(!tags.is_empty());
    // Each tag entry should have tag and count fields
    assert!(tags[0]["tag"].is_string());
    assert!(tags[0]["count"].is_u64());
}

// ---------------------------------------------------------------------------
// tasks
// ---------------------------------------------------------------------------

#[test]
fn tasks_text_output() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    cmd()
        .args(["--vault", vault.to_str().unwrap(), "tasks"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Buy groceries"))
        .stdout(predicate::str::contains("Fix login bug"));
}

#[test]
fn tasks_json_output() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    let output = cmd()
        .args(["--vault", vault.to_str().unwrap(), "-o", "json", "tasks"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    let tasks = json["data"].as_array().unwrap();
    assert!(tasks.len() >= 2);
    // Verify task structure
    assert!(tasks[0]["path"].is_string());
    assert!(tasks[0]["status"].is_string());
    assert!(tasks[0]["text"].is_string());
    assert!(tasks[0]["line"].is_u64());
}

#[test]
fn tasks_filter_by_status_json() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    let output = cmd()
        .args([
            "--vault",
            vault.to_str().unwrap(),
            "-o",
            "json",
            "tasks",
            "--status",
            "todo",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let tasks = json["data"].as_array().unwrap();
    for task in tasks {
        assert_eq!(task["status"], "todo");
    }
}

// ---------------------------------------------------------------------------
// search
// ---------------------------------------------------------------------------

#[test]
fn search_files_text_output() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    cmd()
        .args(["--vault", vault.to_str().unwrap(), "search", "hello"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hello"));
}

#[test]
fn search_files_json_output() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    let output = cmd()
        .args([
            "--vault",
            vault.to_str().unwrap(),
            "-o",
            "json",
            "search",
            "hello",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    let hits = json["data"].as_array().unwrap();
    assert!(!hits.is_empty());
    assert!(hits[0]["path"].is_string());
    assert!(hits[0]["score"].is_u64());
}

#[test]
fn search_content_json_output() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    let output = cmd()
        .args([
            "--vault",
            vault.to_str().unwrap(),
            "-o",
            "json",
            "search",
            "groceries",
            "--mode",
            "content",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    let hits = json["data"].as_array().unwrap();
    assert!(!hits.is_empty());
    assert!(hits[0]["path"].is_string());
    assert!(hits[0]["line"].is_u64());
    assert!(hits[0]["line_text"].is_string());
}

// ---------------------------------------------------------------------------
// links (positional note arg)
// ---------------------------------------------------------------------------

#[test]
fn links_text_output() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    cmd()
        .args([
            "--vault",
            vault.to_str().unwrap(),
            "links",
            "notes/hello.md",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("note: notes/hello.md"))
        .stdout(predicate::str::contains("unique_targets:"));
}

#[test]
fn links_json_output() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    let output = cmd()
        .args([
            "--vault",
            vault.to_str().unwrap(),
            "-o",
            "json",
            "links",
            "notes/hello.md",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["note"], "notes/hello.md");
    assert!(json["data"]["unique_targets"].is_u64());
    assert!(json["data"]["links"].is_array());
}

// ---------------------------------------------------------------------------
// backlinks (positional note arg)
// ---------------------------------------------------------------------------

#[test]
fn backlinks_text_output() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    cmd()
        .args([
            "--vault",
            vault.to_str().unwrap(),
            "backlinks",
            "notes/other-note.md",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("target: notes/other-note.md"))
        .stdout(predicate::str::contains("backlinks:"));
}

#[test]
fn backlinks_json_output() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    let output = cmd()
        .args([
            "--vault",
            vault.to_str().unwrap(),
            "-o",
            "json",
            "backlinks",
            "notes/other-note.md",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["target"], "notes/other-note.md");
    assert!(json["data"]["count"].is_u64());
    assert!(json["data"]["backlinks"].is_array());
}

// ---------------------------------------------------------------------------
// mentions (positional note arg)
// ---------------------------------------------------------------------------

#[test]
fn mentions_json_output() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    let output = cmd()
        .args([
            "--vault",
            vault.to_str().unwrap(),
            "-o",
            "json",
            "mentions",
            "notes/hello.md",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert!(json["data"]["count"].is_u64());
    assert!(json["data"]["mentions"].is_array());
}

// ---------------------------------------------------------------------------
// query
// ---------------------------------------------------------------------------

#[test]
fn query_text_output() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    cmd()
        .args(["--vault", vault.to_str().unwrap(), "query", "--tag", "rust"])
        .assert()
        .success()
        .stdout(predicate::str::contains("notes/"));
}

#[test]
fn query_json_output() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    let output = cmd()
        .args([
            "--vault",
            vault.to_str().unwrap(),
            "-o",
            "json",
            "query",
            "--tag",
            "rust",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    let hits = json["data"].as_array().unwrap();
    assert!(!hits.is_empty());
    assert!(hits[0]["path"].is_string());
}

// ---------------------------------------------------------------------------
// graph
// ---------------------------------------------------------------------------

#[test]
fn graph_json_output() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    let output = cmd()
        .args(["--vault", vault.to_str().unwrap(), "-o", "json", "graph"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert!(json["data"]["issue_count"].is_u64());
}

// ---------------------------------------------------------------------------
// check links
// ---------------------------------------------------------------------------

#[test]
fn check_links_json_output() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    let output = cmd()
        .args([
            "--vault",
            vault.to_str().unwrap(),
            "-o",
            "json",
            "check",
            "links",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert!(json["data"]["internal_occurrences"].is_u64());
    assert!(json["data"]["ok"].is_u64());
    assert!(json["data"]["broken"].is_array());
}

// ---------------------------------------------------------------------------
// check frontmatter
// ---------------------------------------------------------------------------

#[test]
fn check_frontmatter_json_output() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    let output = cmd()
        .args([
            "--vault",
            vault.to_str().unwrap(),
            "-o",
            "json",
            "check",
            "frontmatter",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert!(json["data"]["notes_without_frontmatter"].is_u64());
    assert!(json["data"]["missing"].is_array());
    assert!(json["data"]["broken"].is_array());
}

// ---------------------------------------------------------------------------
// check schema
// ---------------------------------------------------------------------------

#[test]
fn check_schema_json_output() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    let output = cmd()
        .args([
            "--vault",
            vault.to_str().unwrap(),
            "-o",
            "json",
            "check",
            "schema",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert!(json["data"]["errors"].is_u64());
    assert!(json["data"]["warnings"].is_u64());
    assert!(json["data"]["violations"].is_array());
}

// ---------------------------------------------------------------------------
// check frontmatter shows details by default (no --show-broken needed)
// ---------------------------------------------------------------------------

#[test]
fn check_frontmatter_always_shows_missing_details() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    // In the new CLI, check frontmatter ALWAYS shows missing/broken details
    cmd()
        .args(["--vault", vault.to_str().unwrap(), "check", "frontmatter"])
        .assert()
        .success()
        .stdout(predicate::str::contains("missing:"))
        .stdout(predicate::str::contains("no-frontmatter.md"));
}

// ---------------------------------------------------------------------------
// check links always shows broken details
// ---------------------------------------------------------------------------

#[test]
fn check_links_always_shows_broken_details() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    // hello.md links to [[other-note]] which should resolve, but let's check the format
    cmd()
        .args(["--vault", vault.to_str().unwrap(), "check", "links"])
        .assert()
        .success()
        .stdout(predicate::str::contains("internal_occurrences:"));
}

// ---------------------------------------------------------------------------
// --quiet suppresses stderr
// ---------------------------------------------------------------------------

#[test]
fn quiet_flag_suppresses_stderr() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    let output = cmd()
        .args(["--vault", vault.to_str().unwrap(), "--quiet", "stats"])
        .output()
        .unwrap();

    assert!(output.status.success());
    // stderr should be empty (no progress messages)
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.is_empty(),
        "expected empty stderr with --quiet, got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// schema init
// ---------------------------------------------------------------------------

#[test]
fn schema_init_creates_file() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    cmd()
        .args(["--vault", vault.to_str().unwrap(), "schema", "init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("schema written to"));

    let schema_path = vault.join(".obsidian/oxidian/schema.toml");
    assert!(schema_path.exists());
}

#[test]
fn schema_init_json_output() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    let output = cmd()
        .args([
            "--vault",
            vault.to_str().unwrap(),
            "-o",
            "json",
            "schema",
            "init",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert!(json["data"]["path"].is_string());
    assert!(json["data"]["template"].is_string());
}

// ---------------------------------------------------------------------------
// JSON error envelope
// ---------------------------------------------------------------------------

#[test]
fn nonexistent_vault_returns_json_error() {
    let output = cmd()
        .args([
            "--vault",
            "/nonexistent/path/to/vault",
            "-o",
            "json",
            "stats",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert!(json["error"]["code"].is_string());
    assert!(json["error"]["message"].is_string());
}

// ---------------------------------------------------------------------------
// Positional args work (not --note)
// ---------------------------------------------------------------------------

#[test]
fn backlinks_takes_positional_note_arg() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    // This should work: positional arg, not --note
    cmd()
        .args([
            "--vault",
            vault.to_str().unwrap(),
            "backlinks",
            "other-note",
        ])
        .assert()
        .success();
}

#[test]
fn links_takes_positional_note_arg() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    // This should work: positional arg, not --note
    cmd()
        .args([
            "--vault",
            vault.to_str().unwrap(),
            "links",
            "notes/hello.md",
        ])
        .assert()
        .success();
}

#[test]
fn search_takes_positional_query_arg() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    create_vault(&vault);

    // This should work: positional query, not --query
    cmd()
        .args(["--vault", vault.to_str().unwrap(), "search", "hello"])
        .assert()
        .success();
}
