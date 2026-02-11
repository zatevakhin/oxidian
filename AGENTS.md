# AGENTS.md

## Preferences
- If the developer corrects you and asks you to redo something you already did, extract the preference from that correction and add it here.
- Developer preferences are high priority; follow them strictly.
- Use proper logging instead of print/eprintln where applicable. If context is unclear, ask the developer.
- Backwards compatibility is not required unless the developer explicitly asks for it.
- After any Rust code changes, run `cargo check`. Run `cargo fmt` only if `cargo check` passes.
- Before developing a feature, add tests for it first (TDD).
- Add logs that could be useful to the developer, following existing repository patterns.

## Repository Overview
- Language: Rust (edition 2024)
- Async runtime: Tokio
- Primary crate: `oxidian`
- Features: `sqlite`, `similarity`
- CI: build, test, fmt check, clippy (warnings as errors)

## Build, Lint, and Test Commands

### Build
- `cargo build`
- `cargo build --release`

### Format
- `cargo fmt`
- `cargo fmt -- --check`

### Lint (Clippy)
- `cargo clippy -- -D warnings`

### Tests
- `cargo test`
- `cargo test --verbose`

### Single test (function name)
- `cargo test test_name`
- `cargo test backlinks_resolves` (example)

### Single test file (integration test)
- `cargo test --test backlinks_integration`
- `cargo test --test query_integration`

### Single test with features
- `cargo test --features sqlite --test sqlite_integration`
- `cargo test --features similarity --test similarity_integration`

## Code Style Guidelines

### Formatting
- Use `rustfmt` defaults (no custom config in repo).
- Keep code lines reasonably short; let rustfmt wrap.
- Avoid manual alignment; trust rustfmt.

### Imports
- Order imports: `std` first, then external crates, then `crate`.
- Separate these groups with a blank line.
- Prefer explicit imports over globbing in library code.

### Naming
- Types: `CamelCase` (structs, enums, traits).
- Functions/variables: `snake_case`.
- Constants: `SCREAMING_SNAKE_CASE`.
- Use descriptive names for domain types (`VaultPath`, `NoteMeta`).

### Types and Ownership
- Prefer `&str` over `String` in APIs unless ownership is required.
- Prefer `&Path` / `PathBuf` and `VaultPath` for vault-relative paths.
- Use `usize` for counts/limits; `u32` for line numbers (matches codebase).
- Avoid unnecessary `clone`; clone only when needed for async or storage.

### Error Handling
- Library code uses `crate::Result<T>` and `crate::Error` (thiserror).
- Propagate errors with `?`; avoid `unwrap` in non-test code.
- Use `Error::io(path, source)` for IO context when relevant.
- For binaries/examples/tests, `anyhow::Result` is acceptable.

### Async and Blocking Work
- Use `tokio` for async entry points and tasks.
- Use `tokio::task::spawn_blocking` for CPU/IO-heavy sync work.
- Do not block async tasks with synchronous filesystem loops.

### Collections and Ordering
- Prefer `BTreeSet`/`BTreeMap` when deterministic ordering matters.
- Use `Vec` for ordered output and keep explicit sorting where needed.

### Visibility and Public API
- Public API is re-exported in `src/lib.rs`.
- Prefer `pub(crate)` for internal structures/functions.
- Avoid expanding the public surface without strong need.

### Comments and Docs
- Keep comments factual and concise; use doc comments for public APIs.

### Tests
- Integration tests live in `tests/`.
- Use `tempfile` for filesystem isolation.
- Async tests use `#[tokio::test]`.
- Feature-gated tests include `#![cfg(feature = "...")]`.
