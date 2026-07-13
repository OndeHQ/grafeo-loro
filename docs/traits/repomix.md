# repomix.md

`repomix` flattens a codebase into a single LLM-readable file for read-only context.

## Usage in this repo

```bash
# From repo root (where repomix.config.json lives):
repomix
```

This produces `repomix-output.xml` (default) containing every file matched by
`include` in `repomix.config.json`, with comments removed and content compressed.

## Config (already set in `repomix.config.json`)

```json
{
  "compress": true,
  "removeComments": true,
  "include": ["**/*.rs"]
}
```

- `compress=true`: tree-sitter based compression (signatures + structure).
- `removeComments=true`: strip `//` and `/* */` comments to save tokens.
- `include=["**/*.rs"]`: only Rust source files (no Cargo.toml, no docs, no tests `.rs` is included — `**/*.rs` matches them too).

## When to re-run

Run `repomix` at the start of every sub-agent task to refresh context. Then read
`repomix-output.xml` (or `rg -n` against it) instead of opening individual files.

## Read-only

`repomix` does not edit the codebase. It only writes `repomix-output.xml` (and
the file is gitignored — never commit it).

## Prefer `rg -n` over file reads

For targeted lookups ("where is `SyncEngine` defined?"), use:
```bash
rg -n "struct SyncEngine" repomix-output.xml
```
This is faster and uses less context than opening the source file directly.
Only open the source file when full implementation closure is needed.
