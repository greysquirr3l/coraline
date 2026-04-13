# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Symbol name disambiguation for MCP graph tools** — `coraline_callers`, `coraline_callees`, `coraline_impact`, `coraline_find_references`, `coraline_node`, `coraline_dependencies`, `coraline_dependents`, and `coraline_path` now accept `name` (+ optional `file`) as an alternative to `node_id`, with clear disambiguation errors when multiple symbols share the same name
- **`file` filter on search tools** — `coraline_search` and `coraline_find_symbol` accept an optional `file` parameter to scope results to a specific file path
- **`coraline_find_file` MCP tool** — glob-based file search (`*.rs`, `test_*`, `[Cc]argo.toml`) that walks the project tree, skipping common build/hidden directories

### Fixed

- **Release workflow not triggered by auto-tag** — tags pushed by `github-actions[bot]` via `GITHUB_TOKEN` don't fire `push` events on other workflows; auto-tag now explicitly triggers the release workflow via `workflow_dispatch`

## [0.7.0] - 2026-04-13

### Added

- **MCP background auto-sync** — the MCP server now spawns a background thread that periodically checks index freshness and performs incremental sync when files have changed, keeping the knowledge graph current without manual intervention
- **Automatic incremental embedding** — after each background sync, newly-added nodes are automatically embedded when the `embeddings` feature is enabled and ONNX model files are present on disk
- **`auto_sync_interval_secs` config** — new `[sync]` setting in `config.toml` controls the background check interval (default: 120 seconds, set to 0 to disable)
- **`coraline update` CLI command** — checks crates.io for newer published versions and prints upgrade instructions
- **`get_unembedded_nodes` database query** — efficient LEFT JOIN query to find nodes missing vector embeddings, enabling incremental embedding instead of re-embedding everything
- **Repository logo asset** — added `assets/img/coraline_logo.png` and wired it into the README header for consistent project branding

### Changed

- **`ureq` is now a non-optional dependency** — always available for update checking (previously gated behind the `embeddings` feature)
- **`tree-sitter-dart` updated to 0.1.0** — migrated from deprecated `language()` function to the new `LANGUAGE` constant API

### Dependencies

- Consolidated Dependabot PRs (#15–#19) — CI action versions (`actions/checkout@v6`, `codeql-action@v4`, `upload-artifact@v7`, `download-artifact@v8.0.1`) were already at target versions; no changes needed
- Skipped `ort` 2.0.0-rc.12 due to upstream `VitisAI` build regression — remains pinned at `=2.0.0-rc.11`

### Documentation

- **README cleanup and docs-site routing** — removed emoji-heavy formatting, normalized the logo image tag, and updated primary documentation links to point to the published site at `https://greysquirr3l.github.io/coraline/`
- **Configuration, MCP, and CLI docs updated** — documented `auto_sync_interval_secs`, background auto-sync behavior, and the `coraline update` command

## [0.6.0] - 2026-04-09

### Added

- **`--skip-sync` for `coraline embed`** — allows explicitly bypassing the pre-embed sync check when you intentionally want to embed the current indexed state
- **`SyncStatus` preflight API (`extraction::needs_sync`)** — lightweight sync-status check now returns detailed added/modified/removed counts for reuse by CLI and MCP flows

### Changed

- **`coraline embed` now preflights index freshness** — embed checks for stale index state and auto-runs incremental `sync` only when needed, with progress output that reports detected and applied changes
- **`coraline_semantic_search` now performs periodic freshness maintenance** — MCP semantic search throttles freshness checks and, when stale, auto-syncs the graph and refreshes stale/missing node embeddings before serving results

### Documentation

- **CLI, MCP, and README docs updated** — documented `embed --skip-sync`, pre-embed auto-sync behavior, and MCP semantic-search freshness metadata
- **mdBook docs site added** — introduced `docs/book/` and a GitHub Pages deployment workflow (`docs-pages.yml`) to publish documentation on the project GitHub Pages site
- **Architecture docs visual refresh** — replaced ASCII overview with GitHub-native Mermaid diagrams for cleaner rendering and maintenance

## [0.5.0] - 2026-04-08

### Added

- **MCP protocol negotiation and compatibility fallback** — server now negotiates protocol version with clients, preferring `2025-11-25` while retaining compatibility with `2024-11-05`
- **`tools/list` pagination support** — cursor-based pagination added via `cursor` request param and `nextCursor` response field

### Changed

- **MCP lifecycle enforcement tightened** — normal operations now require successful `initialize` followed by `notifications/initialized`
- **Tool error semantics aligned with MCP expectations** — unknown tool calls return protocol errors; tool execution failures continue returning `isError: true` results
- **Tool capability declaration expanded** — MCP initialize response now advertises `tools.listChanged` capability (currently `false`)
- **Core dependencies refreshed for 0.5.0** — upgraded key libraries including `toml` (1.1), `rusqlite` (0.39), `sha2` (0.11), `tokenizers` (0.22), and multiple tree-sitter parser crates; validated with full workspace tests and clippy
- **Workflow supply-chain hardening** — all CI and CodeQL GitHub Actions are now pinned to immutable commit SHAs to improve OSSF Scorecard `Pinned-Dependencies` posture
- **Strict lint command is now standardized** — added tracked `.cargo/config.toml` alias so `cargo lint` consistently enforces the project clippy policy in local and CI runs

### Fixed

- **Tool result schema field casing** — MCP tool results now serialize as `isError` (camelCase) instead of `is_error`
- **Clippy pedantic compliance in MCP server code** — removed no-effect underscore bindings and replaced potential panicking slice/index patterns with safe iterator/object mutation patterns

### Documentation

- **MCP documentation refreshed across README and docs book** — updated protocol/lifecycle notes, pagination behavior, and development examples to reflect current server behavior

## [0.4.4] - 2026-04-08

### Fixed

- **Windows cross-platform CI test failure** — `parse_project_root_accepts_file_uri` unit test now handles Windows file URI format (`file:///C:/...`) correctly by normalizing the leading slash when present; test is now platform-aware and uses appropriate URIs for each platform

## [0.4.3] - 2026-04-07

### Fixed

- **Cross-compilation builds failing on OpenSSL** — switched TLS backend from `native-tls` (OpenSSL) to `rustls` for all HTTP operations; musl and ARM cross-builds no longer require OpenSSL headers or linking
- **Root cause**: `ort` dependency had default features enabled which pulled in `tls-native` → `ureq/native-tls` → `openssl-sys`; now uses `default-features = false` with explicit `tls-rustls`

### Changed

- **`embeddings` feature now uses rustls** — pure Rust TLS for model downloads, no system OpenSSL dependency
- **`embeddings-dynamic` no longer includes any TLS stack** — users supply their own ONNX runtime, no HTTP downloads needed

### Security

- **Pinned all GitHub Actions to commit SHAs** — OSSF Scorecard `PinnedDependenciesID` compliance
- **Added Dependabot configuration** — automated dependency updates for Cargo and GitHub Actions

## [0.4.2] - 2026-04-07

### Fixed

- **MCP tools discovery without explicit `--path`** — `tools/list` now lazily initializes the tool registry when clients call it before `initialize`, so tools are returned even when `coraline serve --mcp` starts without `-p`
- **Safer MCP project-root URI parsing** — non-`file://` URIs (for example, remote client scheme URIs) are no longer treated as filesystem paths during `initialize`; server falls back to an actual local path when needed
- **Regression coverage for MCP startup flow** — added tests for pre-initialize `tools/list` behavior and URI parsing guards to prevent regressions

## [0.4.1] - 2026-04-03

### Added

- **Embedding model prompt on `coraline init`** — when stdin is a TTY, `init` now offers to download the embedding model (~137 MB) immediately after initialization; declined or non-interactive runs print a tip and continue normally with full graph functionality
- **`embeddings` is now the default feature** — `cargo install coraline` includes ONNX/semantic search support out of the box; no `--features` flag required for most users

### Fixed

- **MCP server no longer ghost-creates `.coraline/`** — `MemoryManager` previously called `create_dir_all(.coraline/memories/)` eagerly on every MCP startup, leaving a stub directory that blocked `coraline init` from running cleanly; it now returns an error if `.coraline/` doesn't exist, which the MCP tool registry handles gracefully
- **`coraline init -i` on an already-initialized project no longer prompts to overwrite** — when `--index` is present without `--force`, init detects the existing directory, skips the destructive overwrite, and runs indexing directly; use `--force` to explicitly wipe and reinitialize
- **`coraline_semantic_search` MCP tool degrades gracefully without a model** — when no ONNX model file is present the tool is not registered (all other tools remain available) and a warning is emitted to the project log

---

## [0.4.0] - 2026-03-20

### Added

- **Full multi-language symbol extraction coverage** across all supported languages, including previously missing Python internals and broad AST node-kind mappings.
- **Expanded import/export resolution across languages** for stronger cross-file graph relationships and MCP traversal behavior.
- **Broader call-expression detection and callee extraction** across language grammars to improve call graph completeness.

### Changed

- **Cross-language resolution quality** improved for callers/callees/impact tooling due to richer import/export and call mapping.

### Internal

- Strict lint compliance restored after expansion (`cargo clippy --all-features -- -D warnings`).
- Release validated with build + full test suite.

---

## [0.3.1] - 2026-03-18

### Added

- **`embeddings-dynamic` feature flag** — alternative to `embeddings` that uses `ort/load-dynamic` instead of `ort/download-binaries`, allowing users on systems with older glibc (e.g., Rocky Linux, HPC nodes) to supply their own `libonnxruntime.so` built against their local glibc ([#8](https://github.com/greysquirr3l/coraline/issues/8))
- **musl static binaries in releases** — `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` targets added to the release CI matrix, producing fully static binaries with zero glibc dependency

---

## [0.3.0] - 2026-03-07

### Added

- **Vector math optimizations** — cosine similarity and L2 normalization now use fused multiply-add for improved numerical stability

### Changed

- **Dependencies updated** — refreshed core dependencies and applied transitive updates for security and compatibility (tree-sitter, clap, tempfile, syn, and 16+ transitive deps)

### Internal

- All tests validated (37/37 passing); property tests ensure numerical accuracy

---

## [0.2.3] - 2026-03-01

### Fixed

- **`coraline init -i` on an already-initialized project** — instead of hard-failing, the CLI now prompts "Overwrite? [y/N]" when stdin is a TTY, or prints a clear error with `--force` guidance in non-interactive contexts; `--force` / `-f` flag added to `init` to skip the prompt
- **UNIQUE constraint failures on minified/single-line files** — node IDs now incorporate `start_column` in addition to `start_line`, preventing hash collisions for multiple symbols on the same line
- **Garbled progress output during `index`/`sync`** — progress lines now use `\r\x1B[K` (erase-to-end-of-line) instead of bare `\r`, and `stdout` is flushed after each update to prevent interleaving with log output

### Internal

- OSSF Scorecard CI workflow added
- Security audit workflow now also triggers on `deny.toml` changes
- `cargo fmt` style pass across `config.rs` and `resolution/mod.rs`

---

## [0.2.2] - 2026-02-21

### Fixed

- **Release pipeline: binary builds failing on `ort-sys`** — `ort`, `tokenizers`, and `ndarray` are now optional, gated behind an `embeddings` feature flag; default builds no longer require ONNX Runtime prebuilt binaries
- **Release pipeline: `coraline publish` failing when version already on crates.io** — publish steps for both `tree-sitter-blazor` and `coraline` now skip gracefully if the version already exists
- **Release pipeline: build matrix cancelling all jobs on first failure** — added `fail-fast: false` so platform builds run independently
- **Dropped `x86_64-apple-darwin` release binary** — Intel Mac is not a supported target; ONNX Runtime provides no prebuilt binaries for it

---

## [0.2.1] - 2026-02-21

### Fixed

- **`coraline init` blocked by log directory** — `logging::init()` eagerly created `.coraline/logs/` before `is_initialized()` ran, making every re-init attempt report "already initialized"
- **`sync` crash on UNIQUE constraint** — incremental sync now catches per-file store errors (warn + continue) instead of aborting the entire sync, consistent with `index_all`
- **`callers`/`callees` CLI showing incorrect results** — CLI was passing no edge-kind filter, surfacing `contains` edges as false callers; now filters to `calls` edges only, consistent with MCP tools
- **CI `actions/checkout@v6`** — updated all workflow steps to the current stable `v4`

---

## [0.2.0] - 2026-02-20

### Added

- **Vector embeddings** — full ONNX pipeline using `ort 2.0.0-rc.11` and nomic-embed-text-v1.5 (384-dim). `coraline embed` CLI command and `coraline_semantic_search` MCP tool
- **25 MCP tools** (26 with embeddings) — complete symbol, graph, file, memory, config, stats, and sync toolset
- **`coraline_stats`** — detailed graph statistics grouped by language, node kind, and edge kind
- **`coraline_dependencies` / `coraline_dependents`** — traversal tools for outgoing/incoming dependencies
- **`coraline_path`** — find shortest paths between any two nodes
- **`coraline_sync`** MCP tool — trigger incremental sync from an MCP client
- **`coraline_semantic_search`** — semantic similarity search over indexed symbols
- **`coraline_find_symbol` / `coraline_get_symbols_overview` / `coraline_node` / `coraline_find_references`** — symbol-level tools matching Serena's precision
- **`coraline_read_file` / `coraline_list_dir` / `coraline_get_file_nodes`** — file exploration tools
- **`coraline_get_config` / `coraline_update_config`** — TOML config management via MCP
- **Memory tools** — `coraline_write_memory`, `coraline_read_memory`, `coraline_list_memories`, `coraline_delete_memory`, `coraline_edit_memory` (regex + literal modes)
- **TOML configuration** — `.coraline/config.toml` with sections for indexing, context, sync, and vectors; written as a commented template on `coraline init`
- **Structured logging** — `tracing` with daily-rotating file appender to `.coraline/logs/coraline.log`; level via `CORALINE_LOG` env var
- **Framework-specific resolvers** — Rust, React, Blazor, Laravel
- **CLI commands** — `callers`, `callees`, `impact`, `config`, `stats`, `embed`; `--json` flag on all query commands
- **Criterion benchmark suite** — 9 benchmarks across indexing, search, graph traversal, and context building groups (`cargo bench --bench indexing`)
- **CI/CD** — GitHub Actions for multiplatform builds (Linux x86\_64/ARM64, macOS x86\_64/ARM64, Windows x86\_64), crates.io publishing, CodeQL scanning, daily dependency auditing
- **28+ language support** via tree-sitter: Rust, TypeScript, JavaScript, TSX, JSX, Python, Go, Java, C, C++, C#, PHP, Ruby, Swift, Kotlin, Bash, Dart, Elixir, Elm, Erlang, Fortran, Groovy, Haskell, Julia, Lua, Markdown, MATLAB, Nix, Perl, PowerShell, R, Scala, TOML, YAML, Zig, Blazor

### Fixed

- TypeScript import extraction: `import_statement` was wrongly mapped as `import_declaration` in tree-sitter AST
- `import_clause` lookup: switched from `child_by_field_name` (always `None`) to child iteration
- Cross-file import edges test: `SELECT *` placed integer `id` at column 0; changed to explicit `SELECT source, target`
- FTS multi-word search: now uses `OR` logic so partial matches are found
- Glob pattern matching: completely rewritten using `globset` crate; prior regex implementation was non-functional
- Parallel indexing: CPU-bound parse phase separated from sequential DB writes; SQLite PRAGMA tuning (`synchronous=NORMAL`, 64 MB cache, 256 MB mmap)

### Changed

- Database filename: `codegraph.db` → `coraline.db`
- Project directory: `.codegraph/` → `.coraline/`
- Post-commit git hook updated to check `.coraline/` directory

## [0.1.3] - 2026-02-15

### Added

- `coraline_stats` MCP tool — graph statistics by language, node kind, and edge kind
- TypeScript import extraction fix

### Fixed

- Cross-file import edge detection

## [0.1.2] - 2026-02-13

### Added

- PHP, Swift, Kotlin, Markdown, TOML parser support
- CI/CD infrastructure (GitHub Actions)
- `.coraline/` directory rename from `.codegraph/`

### Fixed

- Critical glob pattern matching bug (rewritten with `globset`)

## [0.1.1] - 2026-02-10

### Added

- Memory system with 5 MCP tools
- Tool abstraction layer and registry
- Integration test suite

## [0.1.0] - 2026-02-07

### Added

- Initial release
- Tree-sitter based AST extraction for Rust, TypeScript, JavaScript, Python, Go, Java, C, C++, C#, Ruby
- SQLite graph storage
- MCP server (`coraline serve --mcp`)
- Basic CLI: `init`, `index`, `sync`, `status`, `query`, `context`
- `coraline_search`, `coraline_callers`, `coraline_callees`, `coraline_impact`, `coraline_context` MCP tools
- Git post-commit hook integration

[0.2.0]: https://github.com/greysquirr3l/coraline/compare/v0.1.3...v0.2.0
[0.6.0]: https://github.com/greysquirr3l/coraline/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/greysquirr3l/coraline/compare/v0.4.4...v0.5.0
[0.4.2]: https://github.com/greysquirr3l/coraline/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/greysquirr3l/coraline/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/greysquirr3l/coraline/compare/v0.3.1...v0.4.0
[0.3.1]: https://github.com/greysquirr3l/coraline/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/greysquirr3l/coraline/compare/v0.2.3...v0.3.0
[0.2.3]: https://github.com/greysquirr3l/coraline/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/greysquirr3l/coraline/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/greysquirr3l/coraline/compare/v0.2.0...v0.2.1
[0.1.3]: https://github.com/greysquirr3l/coraline/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/greysquirr3l/coraline/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/greysquirr3l/coraline/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/greysquirr3l/coraline/releases/tag/v0.1.0
