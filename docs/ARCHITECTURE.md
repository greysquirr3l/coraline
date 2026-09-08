# Coraline Architecture

Coraline is a local-first code intelligence system that builds a semantic knowledge graph from any codebase. It uses tree-sitter for deterministic AST-based parsing, SQLite for storage and full-text search, and exposes its capabilities via both a CLI and an MCP server.

---

## High-Level Overview

![Coraline high-level architecture](assets/architecture-overview.png)

Diagram source: `docs/diagrams/high-level-overview.mmd`

---

## Source Layout

```
crates/coraline/src/
├── bin/coraline.rs     # CLI entry point (clap)
├── lib.rs              # Public API surface
├── types.rs            # NodeKind, EdgeKind, all shared types
├── db.rs               # SQLite layer + schema + FTS
├── extraction.rs       # Tree-sitter parsing + indexing pipeline
├── graph.rs            # Graph traversal and subgraph queries
├── clustering.rs       # Louvain community detection + process tracing (Phase 5.1)
├── resolution/         # Cross-file reference resolution
│   ├── mod.rs          # Core resolver + framework fallback
│   └── frameworks/     # Language/framework-specific resolvers
│       ├── mod.rs      # FrameworkResolver trait + registry
│       ├── rust.rs     # crate::, super::, self:: resolution
│       ├── react.rs    # ./Foo imports, @/ aliases, components
│       ├── blazor.rs   # .razor file discovery, .NET types
│       └── laravel.rs  # PSR-4, blade views, facades
├── context.rs          # Context builder (Markdown/JSON output)
├── vectors.rs          # Vector storage + cosine similarity + ONNX model management
├── vec_ext.rs          # Optional sqlite-vec integration (Phase 5.3, `--features vec-ext`)
├── memory.rs           # Project memory CRUD
├── config.rs           # TOML + JSON configuration loading
├── sync.rs             # Incremental sync + git hook management
├── logging.rs          # Structured logging (tracing)
├── doctor.rs           # Self-healing probes (config, db, model, hooks, embed coverage)
├── security.rs         # Path/permission guards for MCP file tools
├── audit.rs            # Lightweight structured event log for tool invocations
├── update.rs           # Self-update helper (cargo install --force flow)
├── mcp.rs              # MCP server (JSON-RPC over stdio)
├── utils.rs            # Shared utilities
├── db/                 # Schema migrations + table-level helpers
└── tools/
    ├── mod.rs          # Tool trait + ToolRegistry
    ├── graph_tools.rs  # search, callers, callees, impact, find_symbol, ...
    ├── context_tools.rs# coraline_context
    ├── file_tools.rs   # read_file, list_dir, status, config
    └── memory_tools.rs # write/read/list/delete/edit memory
```

---

## Data Model

### Nodes

Every extracted symbol is a `Node`:

```rust
pub struct Node {
    pub id: String,              // SHA-based deterministic ID
    pub kind: NodeKind,          // function, class, method, struct, ...
    pub name: String,            // unqualified symbol name
    pub qualified_name: Option<String>,
    pub file_path: String,       // absolute path
    pub language: String,
    pub start_line: i64,
    pub end_line: i64,
    pub start_column: i64,
    pub end_column: i64,
    pub signature: Option<String>,
    pub docstring: Option<String>,
    pub visibility: Option<String>,
    pub is_exported: bool,
    pub is_async: bool,
    pub is_static: bool,
    pub is_abstract: bool,
    pub decorators: Vec<String>,
    pub type_parameters: Vec<String>,
    pub cluster_id: Option<i64>, // Phase 5.1 — Louvain community id; None for unclustered nodes
}
```

**NodeKind values:** `file`, `module`, `class`, `struct`, `interface`, `trait`, `protocol`, `function`, `method`, `property`, `field`, `variable`, `constant`, `enum`, `enum_member`, `type_alias`, `namespace`, `parameter`, `import`, `export`, `route`, `component`

### Edges

Relationships between nodes are `Edge` records:

```rust
pub struct Edge {
    pub id: String,
    pub source: String,          // source node ID
    pub target: String,          // target node ID
    pub kind: EdgeKind,
    pub line: Option<i64>,       // line where the relationship occurs
    pub metadata: Option<String>,
    pub confidence: f32,         // Phase 5.2 — in [0.0, 1.0], see table below
    pub process_id: Option<i64>, // Phase 5.1 — execution-flow trace id; None for edges not in any process trace
}
```

**EdgeKind values:** `contains`, `calls`, `imports`, `exports`, `extends`, `implements`, `references`, `type_of`, `returns`, `instantiates`

Every `Edge` carries a `confidence: f32` in `[0.0, 1.0]`:

- `1.0` — direct AST-extracted (caller wrote the symbol syntactically; no resolution was needed)
- `0.95` — strongly-typed Rust path (`crate::` / `super::` / `self::`)
- `0.5` — generic name match / framework fallback / heuristic ranker

`coraline_callers` / `coraline_callees` / `coraline_find_references` accept a `min_confidence` parameter that filters edges below a caller-supplied threshold. Default `0.0` includes every edge; a typical working value is `0.8` to suppress generic matches.

---

## Indexing Pipeline

1. **Scan** — Glob the project tree using `include_patterns`/`exclude_patterns`.
2. **Parse** — For each file, spawn the appropriate tree-sitter grammar and walk the AST.
3. **Extract** — Emit `Node` and `Edge` records from the AST visitor. Every `Edge` carries a `confidence` field (see above).
4. **Store** — Upsert nodes and edges into SQLite. A file content hash prevents re-parsing unchanged files.
5. **Resolve** — Walk `unresolved` reference edges, attempt name-based resolution in the DB; fall back to framework-specific resolvers for zero-candidate references. Resolved edges are stamped with their `confidence`.
6. **Cluster** _(Phase 5.1)_ — Run Louvain community detection over the call graph and write `nodes.cluster_id`. Detects "module-like" communities (groups of nodes that call each other more than they call outside the group).
7. **Trace processes** _(Phase 5.1)_ — For each exported `Function`/`Method` with no incoming `calls` edge (an "entry point"), DFS forward through the call graph with cycle protection and a depth cap, writing `edges.process_id`. Every edge that participates in some process trace gets a `process_id`.

Steps 6 and 7 run as part of every `coraline index` and `coraline sync` after extraction succeeds. The columns are nullable — `cluster_id IS NULL` and `process_id IS NULL` are normal for nodes/edges that don't participate in any cluster or process trace.

### Incremental Sync

`coraline sync` (and the git post-commit hook) uses `git diff --name-only HEAD~1` to find changed files, then re-parses only those files. Deleted files have their nodes pruned from the graph.

---

## Reference Resolution

Resolution happens in two passes:

1. **Name-based**: The `resolution::resolve_unresolved` function looks up reference names in the DB using ranked candidate scoring (file proximity, name similarity, kind match).

2. **Framework fallback**: When no candidates score above threshold, `framework_fallback` is called. The registered `FrameworkResolver` implementations detect the active framework (by checking for `Cargo.toml`, `package.json`, `artisan`, `.csproj`, etc.) and return candidate file paths. Nodes from those files are then loaded and filtered by the referenced symbol name.

Current framework resolvers:

- **RustResolver** — `crate::`, `super::`, `self::` qualified paths → `.rs` file mapping
- **ReactResolver** — `./Foo` relative imports, `@/` path aliases, PascalCase component search
- **BlazorResolver** — PascalCase component → `.razor` file, dot-qualified .NET types
- **LaravelResolver** — PSR-4 FQN → PHP file, dot-notation views → blade templates

---

## Tool Architecture

All MCP tools implement the `Tool` trait:

```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn input_schema(&self) -> Value;
    fn execute(&self, params: Value) -> ToolResult;
}
```

Tools are registered in a `ToolRegistry`, which:

- Dispatches `tools/call` MCP requests by name
- Automatically generates `tools/list` responses from registered metadata
- Can be used outside MCP (CLI, library API, tests)

---

## Database Schema

The SQLite database (`.coraline/coraline.db`) has these tables:

| Table             | Purpose                                                                                                                                                      |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `nodes`           | All indexed symbols with full metadata, plus a nullable `cluster_id` (Phase 5.1, Louvain community id)                                                       |
| `edges`           | Directed relationships between nodes, plus a `confidence REAL` (Phase 5.2, in `[0.0, 1.0]`) and a nullable `process_id` (Phase 5.1, execution-flow trace id) |
| `nodes_fts`       | FTS5 virtual table for fast name search                                                                                                                      |
| `vectors`         | Optional embeddings storage. `embedding BLOB` (v1) or vec0 `vec0` virtual table (`--features vec-ext`)                                                       |
| `schema_versions` | Additive-migration bookkeeping. Every additive `ALTER TABLE` bumps a version row here.                                                                       |

A `files` table tracks content hashes for incremental sync. An `unresolved_refs` table holds references that couldn't be resolved during extraction, to be retried on full resolution passes.

### Additive migrations

DB schema changes after v1 are additive: each new column is gated by a `PRAGMA table_info(<table>)` check in `db::apply_incremental_migrations` and stamped with a row in `schema_versions`. Existing DBs migrate in place on next `coraline init` / `coraline sync` without backfill. The migration order is:

- v2 — `edges.confidence REAL NOT NULL DEFAULT 1.0` (Phase 5.2)
- v3 — `nodes.cluster_id INTEGER`, `edges.process_id INTEGER` (Phase 5.1)

All migrations are idempotent — the `column_exists` guard makes re-running safe.

---

## MCP Protocol

The MCP server (`coraline serve --mcp`) communicates over `stdin`/`stdout` using JSON-RPC 2.0, conforming to the [Model Context Protocol specification](https://modelcontextprotocol.io/).

Supported MCP methods:

- `initialize` / `notifications/initialized`
- `tools/list` — returns tool descriptors with cursor pagination (`cursor` / `nextCursor`)
- `tools/call` — dispatches to `ToolRegistry`
- `ping`

The server is single-threaded and synchronous; each request is fully processed before the next is read.

---

## Logging

Structured logs use the `tracing` crate:

- **Console**: stderr at the level set by `CORALINE_LOG` (default: `info`)
- **File**: `.coraline/logs/coraline.log` with daily rotation (kept 7 days)

```bash
CORALINE_LOG=debug coraline index      # verbose
CORALINE_LOG=warn coraline serve --mcp # quiet
```
