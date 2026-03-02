# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
[0.1.3]: https://github.com/greysquirr3l/coraline/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/greysquirr3l/coraline/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/greysquirr3l/coraline/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/greysquirr3l/coraline/releases/tag/v0.1.0
