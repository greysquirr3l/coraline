# Coraline Development Guide

This document covers building, testing, and contributing to Coraline.

---

## Prerequisites

| Tool | Version | Notes |
|---|---|---|
| Rust | ≥ 1.85 | MSRV — install via [rustup](https://rustup.rs/) |
| Git | any | Required for sync and hooks |
| SQLite | bundled | Included via `rusqlite` — no separate install needed |

---

## Getting Started

```bash
git clone https://github.com/greysquirr3l/coraline.git
cd coraline

# Build (debug)
cargo build --all-features

# Build and install to ~/.cargo/bin
cargo install --path crates/coraline --force

# Verify
coraline --version
```

---

## Common Commands

```bash
# Development build
cargo build --all-features

# Release build
cargo build --release --all-features

# Run all tests
cargo test --all-features

# Run tests with output
cargo test --all-features -- --nocapture

# Run a specific test by name
cargo test resolve_unresolved

# Run a specific integration test file
cargo test --test extraction_test

# Lint (project default clippy baseline)
cargo lint

# Format
cargo fmt

# Check formatting without modifying
cargo fmt -- --check
```

---

## Project Structure

```
coraline/
├── Cargo.toml                     # Workspace manifest
├── deny.toml                      # cargo-deny license/audit policy
├── crates/
│   ├── coraline/                  # Main crate
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── bin/coraline.rs    # CLI binary
│   │       └── ...                # Library modules
│   └── tree-sitter-blazor/        # Custom Blazor grammar
├── docs/                          # Documentation
├── reference_projects/            # Reference implementations (not built)
└── .github/workflows/             # CI/CD pipelines
```

---

## Tests

### Test Overview

| Location | Count | Type |
|---|---|---|
| `src/tools/mod.rs` | 3 | Unit: `ToolRegistry` |
| `src/tools/memory_tools.rs` | 5 | Unit: MCP memory tools |
| `src/memory.rs` | 11 | Unit: memory CRUD |
| `src/vectors.rs` | 5 | Unit: cosine similarity |
| `tests/extraction_test.rs` | 4 (1 ignored) | Integration: AST parsing |
| `tests/graph_test.rs` | 4 | Integration: graph traversal |
| `tests/context_test.rs` | 5 | Integration: context building |

**Current status:** 37/37 passing, 1 ignored (`test_cross_file_references` — import edge extraction not yet implemented).

### Running Tests

```bash
# All tests
cargo test --all-features 2>&1 | grep "test result"

# Single integration file
cargo test --test graph_test

# Single unit test
cargo test memory::tests::test_write_read

# With output (for debugging)
cargo test test_name -- --nocapture
```

### Test Fixtures

Integration tests use fixtures in `crates/coraline/tests/fixtures/`:

| Fixture | Purpose |
|---|---|
| `simple-project/` | Basic TypeScript + Rust extraction |
| `blazor-app/` | Blazor/Razor component parsing |
| `mixed-language/` | Multi-language extraction |

Fixtures are small, self-contained codebases checked into the repo.

---

## Adding a New Tree-Sitter Language

1. Add the tree-sitter crate to `crates/coraline/Cargo.toml`.
2. Add the language variant to `Language` enum in `types.rs`.
3. Add the file extension mapping in `extraction.rs` (`language_from_path`).
4. Add an extraction branch in `extraction.rs` (`extract_nodes_from_ast`).
5. Add to the `is_language_supported` list in `config.rs`.
6. Add at least one fixture file and a test case in `tests/extraction_test.rs`.

---

## Adding a New MCP Tool

1. Implement the `Tool` trait in the appropriate `src/tools/*.rs` file:
   ```rust
   pub struct MyTool { project_root: PathBuf }
   impl Tool for MyTool {
       fn name(&self) -> &'static str { "coraline_my_tool" }
       fn description(&self) -> &'static str { "..." }
       fn input_schema(&self) -> Value { json!({ ... }) }
       fn execute(&self, params: Value) -> ToolResult { ... }
   }
   ```

2. Register in `src/tools/mod.rs` inside `create_default_registry(...)`:
   ```rust
   registry.register(Box::new(MyTool::new(project_root.to_path_buf())));
   ```

3. Add unit tests in the same file or `src/tools/mod.rs`.

4. Document in `docs/MCP_TOOLS.md`.

---

## Adding a Framework Resolver

1. Create `src/resolution/frameworks/my_lang.rs` implementing `FrameworkResolver`:
   ```rust
   pub struct MyLangResolver;
   impl FrameworkResolver for MyLangResolver {
       fn name(&self) -> &'static str { "my_lang" }
       fn detect(&self, project_root: &Path) -> bool { ... }
       fn resolve_to_paths(&self, ctx: &ResolveContext<'_>) -> Vec<PathBuf> { ... }
   }
   ```

2. Add to `default_resolvers()` in `src/resolution/frameworks/mod.rs`.

3. Add `pub mod my_lang;` in `src/resolution/frameworks/mod.rs`.

---

## CI/CD

GitHub Actions workflows run on every push and pull request:

| Workflow | Triggers | What it does |
|---|---|---|
| `ci.yml` | push, PR | build, test, clippy, fmt, docs; multi-platform (Linux/macOS/Windows); MSRV check |
| `release.yml` | `v*` tags | Cross-platform binary builds + GitHub release |
| `security.yml` | daily + dep changes | `cargo-audit` (vulnerabilities) + `cargo-deny` (licenses) |
| `codeql.yml` | weekly | CodeQL analysis |

### Creating a Release

```bash
# Bump version in crates/coraline/Cargo.toml and crates/tree-sitter-blazor/Cargo.toml
# Update CHANGELOG.md

git add -A
git commit -m "chore: release v0.2.0"
git tag v0.2.0
git push origin main --tags
```

The `release.yml` workflow builds binaries for Linux x86_64, macOS x86_64/ARM64, and Windows x86_64, then creates a GitHub release with the CHANGELOG content attached.

---

## Code Style

- `#![forbid(unsafe_code)]` on all modules — no unsafe code
- `cargo lint` must pass **before** committing
- `cargo fmt` for formatting (rustfmt defaults)
- Keep functions focused; prefer extracting helpers over long function bodies
- Prefer `let Some(x) = opt else { return ...; }` over `.unwrap()` or `?` in non-Result contexts
- Use `tracing::{debug, info, warn, error}` for log output — no bare `println!` in library code

See `.coraline/memories/style_conventions.md` in your project for project-specific conventions.

---

## Useful Dev Workflows

### Dogfood Coraline on itself

```bash
cd /path/to/coraline
coraline init -i   # index itself
coraline query "resolve_unresolved"
coraline context "how does reference resolution work"
```

### Test MCP roundtrip manually

```bash
printf '%s\n%s\n%s\n' \
   '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"0.0.1"}}}' \
   '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
   '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
   | coraline serve --mcp
```

### Check all tests pass after a change

```bash
cargo lint && cargo test --all-features 2>&1 | grep "test result"
```
