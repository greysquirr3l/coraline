# CLAUDE.md

This file provides guidance to Claude when working with this repository.

## Project Overview

Coraline is a local-first code intelligence system that builds a semantic knowledge graph from any codebase. It provides structural understanding of code relationships using tree-sitter for AST parsing and SQLite for storage, exposed via an MCP server and CLI.

**Key characteristics:**
- Rust binary + library (`crates/coraline`)
- Per-project data stored in `.coraline/` directory
- Database: `.coraline/coraline.db` (SQLite)
- Memories: `.coraline/memories/` (Markdown files)
- Deterministic extraction from AST, not AI-generated summaries
- MCP server for AI assistant integration
- 28+ language support via tree-sitter

## Build and Development Commands

```bash
# Build (development)
cargo build --all-features

# Build (release / install)
cargo install --path crates/coraline --force

# Run tests
cargo test --all-features

# Lint
cargo clippy --all-features -- -D warnings

# Format
cargo fmt
```

## Running a Single Test

```bash
cargo test test_name                        # Run a specific test by name
cargo test --test extraction_test           # Run a specific integration test file
cargo test --all-features 2>&1 | tail -20   # Run all and see summary
```

## CLI Usage

```bash
coraline init [path]        # Initialize + create .coraline/
coraline init -i [path]     # Initialize and immediately index
coraline index [path]       # Full reindex
coraline sync [path]        # Incremental update (git-diff based)
coraline status [path]      # Show index statistics
coraline query <pattern>    # Search symbols by name
coraline context <task>     # Build context for an AI task
coraline serve --mcp        # Start MCP server (stdio)
coraline hooks install      # Install git post-commit auto-sync hook
```

## Architecture

```
crates/coraline/src/
├── bin/coraline.rs     # CLI entry point (clap)
├── lib.rs              # Public API surface
├── types.rs            # NodeKind, EdgeKind, all shared types
├── db.rs               # SQLite layer + schema (coraline.db)
├── extraction.rs       # Tree-sitter AST parsing + indexing
├── graph.rs            # Graph traversal and queries
├── resolution.rs       # Cross-file reference resolution
├── context.rs          # Context builder (Markdown/JSON output)
├── vectors.rs          # Vector storage + cosine similarity (ONNX pending)
├── memory.rs           # Project memory CRUD (.coraline/memories/)
├── config.rs           # Configuration loading
├── sync.rs             # Incremental sync + git hook management
├── mcp.rs              # MCP server (tool dispatch)
└── tools/
    ├── mod.rs          # Tool trait + ToolRegistry
    ├── graph_tools.rs  # search, callers, callees, impact
    ├── context_tools.rs# build_context
    └── memory_tools.rs # write/read/list/delete/edit memory
```

## Key Types

**NodeKind**: `file`, `module`, `class`, `struct`, `interface`, `trait`, `function`, `method`, `field`, `variable`, `constant`, `enum`, `enum_member`, `type_alias`, `parameter`, `import`, `component`

**EdgeKind**: `contains`, `calls`, `imports`, `exports`, `extends`, `implements`, `references`, `type_of`, `returns`, `instantiates`

## MCP Tools

| Tool | Description |
|------|-------------|
| `coraline_search` | Find symbols by name/pattern |
| `coraline_context` | Build context for a task description |
| `coraline_callers` | Find what calls a symbol |
| `coraline_callees` | Find what a symbol calls |
| `coraline_impact` | Impact radius analysis |
| `coraline_stats` | Detailed graph statistics (by language, kind, edge kind) |
| `coraline_read_memory` | Read a project memory |
| `coraline_list_memories` | List all memories |
| `coraline_delete_memory` | Delete a memory |
| `coraline_edit_memory` | Edit memory via literal or regex replace |

## Test Structure

```
crates/coraline/
├── src/
│   ├── tools/mod.rs        # Unit tests: tool registry (3 tests)
│   ├── tools/memory_tools.rs # Unit tests: MCP memory tools (5 tests)
│   ├── memory.rs           # Unit tests: memory CRUD (11 tests)
│   └── vectors.rs          # Unit tests: cosine similarity (5 tests)
└── tests/
    ├── extraction_test.rs  # Integration: AST parsing (4 tests)
    ├── graph_test.rs       # Integration: graph traversal (4 tests)
    └── context_test.rs     # Integration: context building (5 tests)
```

**Current status:** 37/37 passing, 0 ignored

## Supported Languages

Rust, TypeScript, JavaScript, TSX, JSX, Python, Go, Java, C, C++, C#, PHP, Ruby, Swift, Kotlin, Markdown, TOML, YAML, Zig, Scala, Haskell, Lua, Julia, MATLAB, R, Erlang, Elixir, Groovy, Bash, PowerShell, Nix, Dart, Fortran, Elm, Perl, Blazor (custom grammar)

## Notes

- The `reference_projects/` directory contains the original TypeScript CodeGraph and Serena — used for reference/inspiration, not part of the build
- `mcp.rs.backup` is a historical artifact, can be ignored
- Vector embeddings (`vectors.rs`) have storage/search infrastructure; ONNX embedding generation is deferred pending stable `ort` 2.0 API
