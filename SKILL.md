# oxi — Obsidian Vault CLI for AI Agents

Indexes an Obsidian vault. Queries notes, tags, tasks, links, frontmatter, and schema.

`OBSIDIAN_VAULT` env var is set. Always invoke: `oxi -o json -q <COMMAND>`

## JSON envelope

Every response: `{"ok": true, "data": ...}` or `{"ok": false, "error": {"code": "...", "message": "..."}}`. Non-zero exit on error.

## Discovery

```sh
oxi -o json -q stats                   # {files, notes, tags}
oxi -o json -q stats --tag rust        # adds tag_filter, tagged_files[]
oxi -o json -q tags --top 20           # [{tag, count}]
```

## Search

Modes via `--mode`: `files` (default), `content`, `semantic`.

```sh
oxi -o json -q search "query"                           # [{path, score}]
oxi -o json -q search "query" --mode content --limit 5  # [{path, score, line, line_text}]
oxi -o json -q search "query" --mode semantic            # [{path, score}] (requires similarity feature)
```

## Query

Filter notes by tags, frontmatter fields, path prefix. All flags optional and composable.

```sh
oxi -o json -q query --tag rust
oxi -o json -q query --prefix "projects/" --eq "status=active" --exists priority
oxi -o json -q query --contains "title=machine" --gt "priority=3" --sort priority --desc --limit 10
```

Response: `[{path}]`. Filters: `--prefix`, `--tag`, `--exists FIELD`, `--eq K=V`, `--contains K=V`, `--gt K=V` (all repeatable), `--sort FIELD`, `--desc`, `--limit N`.

## Tasks

```sh
oxi -o json -q tasks                                    # [{path, line, status, text}]
oxi -o json -q tasks --status todo --contains "deploy"
oxi -o json -q tasks --prefix "projects/" --limit 10
```

Status values: `todo`, `done`, `in-progress`, `cancelled`, `blocked`.

## Per-note inspection

All take a **positional** note arg (vault-relative path). `backlinks` also accepts a bare name.

```sh
oxi -o json -q links notes/hello.md
# {note, unique_targets, occurrences, links[{kind, embed, target, subpath, location{line,column}, raw}]}
oxi -o json -q backlinks notes/other.md   # by path — or: backlinks other-note (by name)
# {target, count, backlinks[{source, link{kind, target, location, raw}}]}
oxi -o json -q mentions notes/hello.md --limit 20
# {count, mentions[{source, target, line, term, line_text}]}
oxi -o json -q neighbors notes/hello.md --min-score 0.7 --top-k 5
# [{source, target, score}]  (requires similarity feature)
```

`links` filter flags: `--kind wiki|markdown|autourl|obsidian-uri`, `--only-embeds`.

## Auditing — always returns full details

```sh
oxi -o json -q check links --limit 50
# {internal_occurrences, ok, broken_count, broken[{source, link, reason}]}
# reason: "missing_target" | {ambiguous_target:{candidates}} | {missing_heading:{heading}} | {missing_block:{block}}
oxi -o json -q check frontmatter
# {notes_without_frontmatter, notes_with_frontmatter_valid, notes_with_frontmatter_broken, missing[], broken[{path, error}]}
oxi -o json -q check schema --severity error --limit 20
# {status, errors, warnings, total_violations, violations[{path, violation{severity, code, message}}]}
```

Schema status is `"disabled"` when no schema file exists.

## Graph

```sh
oxi -o json -q graph                        # {unresolved_internal_occurrences, ambiguous_internal_occurrences, issue_count}
oxi -o json -q graph --note notes/hello.md  # adds source, outgoing[{source, link, resolution}]
```

Resolution values: `{"resolved": "path"}`, `"missing"`, `{"ambiguous": ["path1", "path2"]}`.

## Recipes

```sh
oxi -o json -q stats                                                  # vault overview
oxi -o json -q search "machine learning" --mode content --limit 10    # find notes about a topic
oxi -o json -q query --tag project --eq "status=active"               # active project notes
oxi -o json -q backlinks my-note                                      # what links to this note?
oxi -o json -q check links                                            # find broken links
oxi -o json -q tasks --status todo                                    # outstanding tasks
```

## Rules

- Always use `-o json -q`. Never parse text output.
- Note paths are vault-relative. Never absolute.
- `watch` and `persist` are long-running processes — do not use for one-shot queries.
- `schema init --template para|kg|kg-memory` generates a schema file — one-time setup, writes to disk.
