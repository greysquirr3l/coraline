# Coraline MCP Tools Reference

Coraline exposes **28 MCP tools** when running as an MCP server (`coraline serve --mcp`).
All tool names are prefixed with `coraline_` to avoid collisions with other MCP servers.

Protocol notes:
- Negotiates MCP protocol version `2025-11-25` (with compatibility fallback to `2024-11-05`)
- Expects `notifications/initialized` after `initialize` before normal requests
- `tools/list` supports pagination via `cursor` and `nextCursor`

`coraline_semantic_search` is available by default (the `embeddings` feature ships enabled) but only registered when an ONNX model is present in `.coraline/models/`. Run `coraline model download` then `coraline embed` to activate it. The remaining 27 tools are typically available; memory-backed tools may be skipped if their initialization fails (e.g. due to filesystem or permission issues).

### Background Auto-Sync

When the MCP server starts, it spawns a background thread that periodically checks index freshness and runs incremental sync when files have changed. This keeps the knowledge graph current without manual `coraline_sync` calls.

- **Default interval:** 120 seconds (configurable via `sync.auto_sync_interval_secs` in `config.toml`)
- **Disable:** Set `auto_sync_interval_secs = 0` in `[sync]`
- When embeddings are enabled and an ONNX model is present, newly-added nodes are automatically embedded after each background sync
- The background thread uses SQLite WAL mode for safe concurrent access alongside the main MCP request loop

---

## Quick Reference

| Category | Tool | Description |
|---|---|---|
| **Graph** | `coraline_search` | Find symbols by name or pattern |
| | `coraline_callers` | Find what calls a symbol |
| | `coraline_callees` | Find what a symbol calls |
| | `coraline_impact` | Analyze change impact radius |
| | `coraline_dependencies` | Outgoing dependency graph from a node |
| | `coraline_dependents` | Incoming dependency graph (what depends on a node) |
| | `coraline_path` | Find a path between two nodes |
| | `coraline_stats` | Detailed graph statistics by language/kind/edge |
| | `coraline_find_symbol` | Find symbols with rich metadata + optional body |
| | `coraline_get_symbols_overview` | List all symbols in a file |
| | `coraline_find_references` | Find all references to a symbol |
| | `coraline_node` | Get full node details and source code |
| **Context** | `coraline_context` | Build structured context for an AI task |
| **Audit** | `coraline_audit_docs` | Audit Markdown docs for stale references and undocumented exports |
| **File** | `coraline_read_file` | Read file contents |
| | `coraline_list_dir` | List directory contents |
| | `coraline_find_file` | Find files by glob pattern |
| | `coraline_get_file_nodes` | Get all indexed nodes in a file |
| | `coraline_status` | Show project index statistics |
| | `coraline_sync` | Trigger incremental index sync |
| | `coraline_get_config` | Read project configuration |
| | `coraline_update_config` | Update a config value |
| | `coraline_semantic_search` | Vector similarity search (requires model download — see below) |
| **Memory** | `coraline_write_memory` | Write or update a project memory |
| | `coraline_read_memory` | Read a project memory |
| | `coraline_list_memories` | List all memories |
| | `coraline_delete_memory` | Delete a memory |
| | `coraline_edit_memory` | Edit memory via literal or regex replace |

---

## Graph Tools

### `coraline_search`

Search for code symbols by name or pattern across the indexed codebase.

**Input:**

| Parameter | Type | Required | Default | Description |
|---|---|---|---|---|
| `query` | string | ✅ | — | Symbol name or FTS pattern |
| `kind` | string | | — | Filter: `function`, `method`, `class`, `struct`, `interface`, `trait`, `module` |
| `file` | string | | — | Filter results to this file path (relative or absolute) |
| `limit` | number | | `10` | Maximum results |

**Output:**
```json
{
  "results": [
    {
      "node": {
        "id": "abc123",
        "kind": "function",
        "name": "resolve_unresolved",
        "qualified_name": "coraline::resolution::resolve_unresolved",
        "file_path": "/path/to/resolution/mod.rs",
        "start_line": 42,
        "end_line": 95,
        "language": "Rust",
        "signature": "fn resolve_unresolved(conn: &mut Connection, ...)"
      },
      "score": 0.92
    }
  ],
  "count": 1
}
```

---

### `coraline_callers`

Find all functions/methods that call a given symbol (incoming `calls` edges).

**Input:**

| Parameter | Type | Required | Default | Description |
|---|---|---|---|---|
| `node_id` | string | | — | ID of the target node |
| `name` | string | | — | Symbol name (alternative to `node_id`) |
| `file` | string | | — | Disambiguate `name` by file path |
| `limit` | number | | `20` | Maximum callers to return |

Either `node_id` or `name` must be provided. When `name` matches multiple symbols, supply `file` to disambiguate or the tool returns a listing of candidates.

**Output:**
```json
{
  "callers": [
    {
      "id": "def456",
      "kind": "function",
      "name": "index_all",
      "qualified_name": "coraline::extraction::index_all",
      "file_path": "/path/to/extraction.rs",
      "start_line": 120,
      "line": 158
    }
  ],
  "count": 1
}
```

---

### `coraline_callees`

Find all functions/methods that a given symbol calls (outgoing `calls` edges).

**Input:**

| Parameter | Type | Required | Default | Description |
|---|---|---|---|---|
| `node_id` | string | | — | ID of the source node |
| `name` | string | | — | Symbol name (alternative to `node_id`) |
| `file` | string | | — | Disambiguate `name` by file path |
| `limit` | number | | `20` | Maximum callees to return |

Either `node_id` or `name` must be provided.

**Output:** Same shape as `coraline_callers` but field is `callees`.

**Precision Notes:**

- Call-edge resolution prefers extractor-provided candidate IDs (better locality signal) to avoid false-positive cross-project links in mixed active/legacy workspaces.
- When a call target is ambiguous and no scoped match exists, it is left unresolved (empty) rather than linked to a low-confidence global match.
- Results are ordered deterministically by (line, column, target) to ensure consistency across repeated queries.

---

### `coraline_impact`

Analyze the impact radius of changing a symbol — finds everything that directly or transitively depends on it, via BFS over incoming `calls` and `references` edges.

**Input:**

| Parameter | Type | Required | Default | Description |
|---|---|---|---|---|
| `node_id` | string | | — | ID of the node to analyze |
| `name` | string | | — | Symbol name (alternative to `node_id`) |
| `file` | string | | — | Disambiguate `name` by file path |
| `max_depth` | number | | `2` | BFS traversal depth |
| `max_nodes` | number | | `50` | Cap on returned nodes |

Either `node_id` or `name` must be provided.

**Output:**
```json
{
  "nodes": [ ... ],
  "edges": [ ... ],
  "stats": {
    "node_count": 12,
    "edge_count": 15,
    "file_count": 4,
    "max_depth": 2
  }
}
```

---

### `coraline_dependencies`

Get the outgoing dependency graph from a node — what does this symbol import, call, or reference, recursively up to a configurable depth?

**Input:**

| Parameter | Type | Required | Default | Description |
|---|---|---|---|---|
| `node_id` | string | | — | ID of the source node |
| `name` | string | | — | Symbol name (alternative to `node_id`) |
| `file` | string | | — | Disambiguate `name` by file path |
| `max_depth` | number | | `2` | BFS traversal depth |
| `max_nodes` | number | | `50` | Cap on returned nodes |
| `edge_kinds` | string[] | | all | Edge kinds to follow (e.g. `["calls", "imports"]`) |

Either `node_id` or `name` must be provided.

**Output:**
```json
{
  "root_id": "abc123",
  "nodes": [ ... ],
  "edges": [ ... ],
  "stats": { "node_count": 8, "edge_count": 10, "file_count": 3, "max_depth": 2 }
}
```

---

### `coraline_dependents`

Get the incoming dependency graph — what symbols depend on (call, import, or reference) this node, recursively?

**Input:** Same as `coraline_dependencies` (supports `node_id` or `name` + `file`).

**Output:** Same shape as `coraline_dependencies` but traversal follows edges in reverse.

---

### `coraline_path`

Find a path between two nodes in the graph, using BFS over all edge kinds.

**Input:**

| Parameter | Type | Required | Description |
|---|---|---|---|
| `from_id` | string | | Starting node ID |
| `from_name` | string | | Starting node name (alternative to `from_id`) |
| `from_file` | string | | Disambiguate `from_name` by file path |
| `to_id` | string | | Target node ID |
| `to_name` | string | | Target node name (alternative to `to_id`) |
| `to_file` | string | | Disambiguate `to_name` by file path |

For each endpoint, either the `_id` or `_name` parameter must be provided.

**Output:**
```json
{
  "from_id": "abc123",
  "to_id": "def456",
  "path_found": true,
  "path": ["abc123", "mid789", "def456"],
  "length": 3
}
```
Returns `{ "path_found": false }` if no path exists.

---

### `coraline_stats`

Return detailed graph statistics: total counts, per-language file breakdown, node kind breakdown, and edge kind breakdown.

**Input:** None.

**Output:**
```json
{
  "totals": {
    "nodes": 1842,
    "edges": 4201,
    "files": 47,
    "unresolved_references": 123,
    "vectors": 0
  },
  "files_by_language": { "rust": 28, "typescript": 14, "toml": 5 },
  "nodes_by_kind":     { "function": 412, "method": 287, "import": 201, "struct": 88 },
  "edges_by_kind":     { "contains": 1842, "calls": 987, "imports": 201, "exports": 178 }
}
```

---

### `coraline_find_symbol`

Find symbols by name pattern with richer metadata than `coraline_search`, including optional source code body. Good for `Foo/__init__`-style path patterns.

**Input:**

| Parameter | Type | Required | Default | Description |
|---|---|---|---|---|
| `name_pattern` | string | ✅ | — | Symbol name or substring |
| `kind` | string | | — | Same kind filter as `coraline_search` |
| `file` | string | | — | Filter results to this file path |
| `include_body` | boolean | | `false` | Attach source code body |
| `limit` | number | | `10` | Maximum results |

**Output:** `{ "symbols": [...], "count": N }` — each symbol includes `docstring`, `is_exported`, `is_async`, `is_static`, `score`, and optionally `body`.

---

### `coraline_get_symbols_overview`

Get an overview of all symbols in a file, grouped by kind and ordered by line number.

**Input:**

| Parameter | Type | Required | Description |
|---|---|---|---|
| `file_path` | string | ✅ | Path to file (relative to project root or absolute) |

**Output:**
```json
{
  "file_path": "src/lib.rs",
  "symbol_count": 14,
  "by_kind": {
    "function": [ ... ],
    "struct": [ ... ]
  },
  "symbols": [ ... ]
}
```

---

### `coraline_find_references`

Find all nodes that reference (call, import, extend, implement, etc.) a given symbol.

**Input:**

| Parameter | Type | Required | Default | Description |
|---|---|---|---|---|
| `node_id` | string | | — | ID of the target node |
| `name` | string | | — | Symbol name (alternative to `node_id`) |
| `file` | string | | — | Disambiguate `name` by file path |
| `edge_kind` | string | | all | Filter: `calls`, `imports`, `extends`, `implements`, `references` |
| `limit` | number | | `50` | Maximum references |

Either `node_id` or `name` must be provided.

**Output:** `{ "node_id": "...", "references": [...], "count": N }` — each reference includes its `edge_kind` and the line number of the edge.

---

### `coraline_node`

Get complete details for a specific node by ID, including its source code body read from disk.

**Input:**

| Parameter | Type | Required | Default | Description |
|---|---|---|---|---|
| `node_id` | string | | — | The node ID |
| `name` | string | | — | Symbol name (alternative to `node_id`) |
| `file` | string | | — | Disambiguate `name` by file path |
| `include_edges` | boolean | | `false` | Also return incoming/outgoing edge counts |

Either `node_id` or `name` must be provided.

**Output:** Full node record including `body` (source lines), `visibility`, `decorators`, `type_parameters`, `is_async`, `is_static`, `is_abstract`, and optionally `incoming_edge_count` / `outgoing_edge_count`.

---

## Context Tool

### `coraline_context`

Build structured context for an AI task description. Searches the graph, traverses relationships, and returns relevant code snippets in Markdown or JSON format.

**Input:**

| Parameter | Type | Required | Default | Description |
|---|---|---|---|---|
| `task` | string | ✅ | — | Natural language task description |
| `max_nodes` | number | | `20` | Max graph nodes to include |
| `max_code_blocks` | number | | `5` | Max code block attachments |
| `max_code_block_size` | number | | `1500` | Max chars per code block |
| `include_code` | boolean | | `true` | Attach source code snippets |
| `traversal_depth` | number | | `1` | Graph traversal depth |
| `format` | string | | `"markdown"` | `"markdown"` or `"json"` |

**Output:** A Markdown or JSON document containing relevant symbols and code, ready to paste as context for an LLM.

---

## Audit Tool

### `coraline_audit_docs`

Audit Markdown documentation coverage against the indexed code graph.

Detects two classes of issues:
- `stale_refs`: inline code-span symbol references in Markdown that do not resolve to indexed symbols
- `undocumented_exports`: exported code symbols with no inbound `references` edge from Markdown docs

**Input:**

| Parameter | Type | Required | Default | Description |
|---|---|---|---|---|
| `show_undocumented` | boolean | | `true` | Include undocumented export results |
| `show_stale` | boolean | | `true` | Include stale reference results |
| `limit` | number | | `50` | Max items returned per result set |

**Output:**
```json
{
  "summary": {
    "doc_files_indexed": 12,
    "doc_sections_indexed": 89,
    "stale_refs_count": 3,
    "undocumented_exports_count": 7
  },
  "stale_refs": [
    {
      "reference": "resolve_unresolved",
      "doc_file": "docs/ARCHITECTURE.md",
      "section": "Resolution",
      "line": 42,
      "column": 18
    }
  ],
  "undocumented_exports": [
    {
      "name": "audit_docs",
      "qualified_name": "coraline::audit::audit_docs",
      "kind": "function",
      "file": "crates/coraline/src/audit.rs",
      "line": 48
    }
  ]
}
```

---

## File Tools

### `coraline_read_file`

Read the contents of a file within the project.

**Input:**

| Parameter | Type | Required | Default | Description |
|---|---|---|---|---|
| `path` | string | ✅ | — | File path (relative to project root or absolute) |
| `start_line` | number | | `1` | First line to read (1-indexed, inclusive) |
| `limit` | number | | `200` | Maximum number of lines to return |

---

### `coraline_list_dir`

List the contents of a directory within the project.

**Input:**

| Parameter | Type | Required | Default | Description |
|---|---|---|---|---|
| `path` | string | | `.` | Directory path (relative to project root or absolute). Defaults to project root. |

---

### `coraline_get_file_nodes`

Get all indexed symbols (nodes) for a specific file.

**Input:**

| Parameter | Type | Required | Description |
|---|---|---|---|
| `file_path` | string | ✅ | File path (relative or absolute) |

**Output:** `{ "file_path": "...", "nodes": [...], "count": N }`

---

### `coraline_find_file`

Find files by name or glob pattern. Recursively walks the project tree, skipping `.git`, `node_modules`, `target`, and `.coraline` directories.

**Input:**

| Parameter | Type | Required | Default | Description |
|---|---|---|---|---|
| `pattern` | string | ✅ | — | File name, substring, or glob pattern (`*.rs`, `test_*`, `[Cc]argo.toml`) |
| `limit` | number | | `20` | Maximum results |

**Output:**
```json
{
  "pattern": "*.rs",
  "files": ["src/lib.rs", "src/db.rs", "src/graph.rs"],
  "count": 3
}
```

---

### `coraline_status`

Show project statistics: total files, nodes, edges, and unresolved reference counts.

**Input:** None.

**Output:**
```json
{
  "files": 128,
  "nodes": 4201,
  "edges": 9872,
  "unresolved": 153,
  "db_size_bytes": 2097152
}
```

---

### `coraline_get_config`

Read the current project configuration from `.coraline/config.toml`.

**Input:** None.

**Output:** Full `CoralineConfig` as JSON with all four sections (`indexing`, `context`, `sync`, `vectors`).

---

### `coraline_update_config`

Update a single configuration value using dot-notation path.

**Input:**

| Parameter | Type | Required | Description |
|---|---|---|---|
| `key` | string | ✅ | Dot-notation path, e.g. `context.max_nodes` |
| `value` | any | ✅ | New value (type must match the field) |

---

### `coraline_sync`

Trigger an incremental sync of the index. Detects files added, modified, or removed since the last index run and updates only what changed. Run after editing source files to keep the graph current.

**Input:** None.

**Output:**
```json
{
  "files_checked": 42,
  "files_added": 1,
  "files_modified": 3,
  "files_removed": 0,
  "nodes_updated": 47,
  "duration_ms": 380
}
```

---

### `coraline_semantic_search`

Search indexed nodes using natural-language vector similarity. Included in the default build; only registered as an MCP tool once an ONNX model is present in `.coraline/models/`. To activate:

```bash
coraline model download   # download nomic-embed-text-v1.5 (~137 MB)
coraline embed            # generate embeddings for all indexed nodes
```

When this tool is used, Coraline periodically performs a throttled freshness check. If indexed state is stale it runs incremental sync automatically, then refreshes stale/missing embeddings before search.

**Input:**

| Parameter | Type | Required | Default | Description |
|---|---|---|---|---|
| `query` | string | ✅ | — | Natural-language description of what you're looking for |
| `limit` | number | | `10` | Max results |
| `min_similarity` | number | | `0.3` | Minimum cosine similarity threshold (0–1) |

**Output:**
```json
{
  "query": "how is sync staleness detected",
  "freshness": {
    "checked": true,
    "stale_files_added": 0,
    "stale_files_modified": 2,
    "stale_files_removed": 0,
    "synced": true,
    "files_added": 0,
    "files_modified": 2,
    "files_removed": 0,
    "embeddings_refreshed": true,
    "embeddings_refreshed_count": 18,
    "check_interval_seconds": 30
  },
  "results": [
    {
      "id": "abc123",
      "name": "resolve_unresolved",
      "qualified_name": "coraline::resolution::ReferenceResolver::resolve_unresolved",
      "kind": "function",
      "file_path": "src/resolution/mod.rs",
      "start_line": 42,
      "docstring": null,
      "signature": "fn resolve_unresolved(...)",
      "score": 0.87
    }
  ]
}
```

---

## Memory Tools

Project memories are Markdown files stored in `.coraline/memories/`. They persist across sessions and help AI assistants maintain project context.

### `coraline_write_memory`

Write or update a project memory.

**Input:**

| Parameter | Type | Required | Description |
|---|---|---|---|
| `name` | string | ✅ | Memory name (without `.md`). E.g. `project_overview` |
| `content` | string | ✅ | Memory content in Markdown format |

---

### `coraline_read_memory`

Read a project memory by name.

**Input:**

| Parameter | Type | Required | Description |
|---|---|---|---|
| `name` | string | ✅ | Memory name (without `.md`) |

**Output:** `{ "name": "...", "content": "..." }`

---

### `coraline_list_memories`

List all available memories for the project.

**Input:** None.

**Output:** `{ "memories": ["project_overview", "architecture_notes", ...], "count": N }`

---

### `coraline_delete_memory`

Delete a project memory.

**Input:**

| Parameter | Type | Required | Description |
|---|---|---|---|
| `name` | string | ✅ | Memory name to delete |

---

### `coraline_edit_memory`

Edit a memory file by replacing text — either as a literal string match or a regex pattern.

**Input:**

| Parameter | Type | Required | Default | Description |
|---|---|---|---|---|
| `name` | string | ✅ | — | Memory name (without `.md`) |
| `pattern` | string | ✅ | — | Text to find |
| `replacement` | string | ✅ | — | Replacement text |
| `mode` | string | | `"literal"` | `"literal"` or `"regex"` |

---

## MCP Client Configuration

### Claude Desktop

Add to `~/Library/Application Support/Claude/claude_desktop_config.json` (macOS):

```json
{
  "mcpServers": {
    "coraline": {
      "command": "/path/to/coraline",
      "args": ["serve", "--mcp", "--path", "/path/to/your/project"]
    }
  }
}
```

### Claude Code

Add to `.claude/mcp.json` in your project workspace:

```json
{
  "mcpServers": {
    "coraline": {
      "command": "coraline",
      "args": ["serve", "--mcp"]
    }
  }
}
```

When `--path` is omitted, the working directory is used as the project root.
