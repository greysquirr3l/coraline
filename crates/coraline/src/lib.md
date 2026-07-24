# Coraline

A local-first code intelligence library that builds a semantic knowledge graph from any codebase.

## Modules

| Module | Description |
|---|---|
| `config` | Load and save project configuration (TOML + JSON) |
| `context` | Build structured context docs for AI tasks |
| `db` | SQLite graph storage, FTS search, and schema management |
| `extraction` | Tree-sitter-based AST parsing and indexing pipeline |
| `graph` | BFS/DFS graph traversal and subgraph construction |
| `logging` | Structured tracing setup with daily-rotating file output |
| `mcp` | MCP server (JSON-RPC 2.0 over stdio) |
| `memory` | Project memory CRUD (`.coraline/memories/*.md`) |
| `resolution` | Cross-file reference resolution with framework-specific fallbacks |
| `sync` | Incremental sync (git-diff based) and post-commit hook management |
| `tools` | MCP tool trait, registry, and all 20 tool implementations |
| `types` | Shared types: `Node`, `Edge`, `NodeKind`, `EdgeKind`, `Language`, config structs |
| `vectors` | Vector embedding storage and cosine similarity search |

## Quick Start

```rust
use coraline::{config, db, extraction};
use std::path::Path;

let root = Path::new("/my/project");

// Initialize
let cfg = config::create_default_config(root);
config::save_config(root, &cfg)?;
db::initialize_database(root)?;

// Index
extraction::index_all(root, &cfg, false, None)?;

// Query
let conn = db::open_database(root)?;
let results = db::search_nodes(&conn, "my_function", None, 10)?;
```

## MCP Integration

Start an MCP server targeting a project:

```rust
use coraline::mcp::McpServer;

let mut server = McpServer::new(Some("/my/project".into()));
server.start()?;
```

The server communicates over stdin/stdout using the Model Context Protocol.
See [`docs/MCP_TOOLS.md`](../../../../docs/MCP_TOOLS.md) for all 20 available tools.

