# Coraline MCP Tools Reference

Coraline exposes **20 MCP tools** when running as an MCP server (`coraline serve --mcp`).
All tool names are prefixed with `coraline_` to avoid collisions with other MCP servers.

---

## Quick Reference

| Category | Tool | Description |
|---|---|---|
| **Graph** | `coraline_search` | Find symbols by name or pattern |
| | `coraline_callers` | Find what calls a symbol |
| | `coraline_callees` | Find what a symbol calls |
| | `coraline_impact` | Analyze change impact radius |
| | `coraline_find_symbol` | Find symbols with rich metadata + optional body |
| | `coraline_get_symbols_overview` | List all symbols in a file |
| | `coraline_find_references` | Find all references to a symbol |
| | `coraline_node` | Get full node details and source code |
| **Context** | `coraline_context` | Build structured context for an AI task |
| **File** | `coraline_read_file` | Read file contents |
| | `coraline_list_dir` | List directory contents |
| | `coraline_get_file_nodes` | Get all indexed nodes in a file |
| | `coraline_status` | Show project index statistics |
| | `coraline_get_config` | Read project configuration |
| | `coraline_update_config` | Update a config value |
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
| `limit` | number | | `10` | Maximum results |

**Output:**
```json
{
  "results": [
    {
      "node": {
        "id": "abc123",
        "kind": "Function",
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
| `node_id` | string | ✅ | — | ID of the target node |
| `limit` | number | | `20` | Maximum callers to return |

**Output:**
```json
{
  "callers": [
    {
      "id": "def456",
      "kind": "Function",
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
| `node_id` | string | ✅ | — | ID of the source node |
| `limit` | number | | `20` | Maximum callees to return |

**Output:** Same shape as `coraline_callers` but field is `callees`.

---

### `coraline_impact`

Analyze the impact radius of changing a symbol — finds everything that directly or transitively depends on it, via BFS over incoming `calls` and `references` edges.

**Input:**

| Parameter | Type | Required | Default | Description |
|---|---|---|---|---|
| `node_id` | string | ✅ | — | ID of the node to analyze |
| `max_depth` | number | | `2` | BFS traversal depth |
| `max_nodes` | number | | `50` | Cap on returned nodes |

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

### `coraline_find_symbol`

Find symbols by name pattern with richer metadata than `coraline_search`, including optional source code body. Good for `Foo/__init__`-style path patterns.

**Input:**

| Parameter | Type | Required | Default | Description |
|---|---|---|---|---|
| `name_pattern` | string | ✅ | — | Symbol name or substring |
| `kind` | string | | — | Same kind filter as `coraline_search` |
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
| `node_id` | string | ✅ | — | ID of the target node |
| `edge_kind` | string | | all | Filter: `calls`, `imports`, `extends`, `implements`, `references` |
| `limit` | number | | `50` | Maximum references |

**Output:** `{ "node_id": "...", "references": [...], "count": N }` — each reference includes its `edge_kind` and the line number of the edge.

---

### `coraline_node`

Get complete details for a specific node by ID, including its source code body read from disk.

**Input:**

| Parameter | Type | Required | Default | Description |
|---|---|---|---|---|
| `node_id` | string | ✅ | — | The node ID |
| `include_edges` | boolean | | `false` | Also return incoming/outgoing edge counts |

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

## File Tools

### `coraline_read_file`

Read the contents of a file within the project.

**Input:**

| Parameter | Type | Required | Default | Description |
|---|---|---|---|---|
| `file_path` | string | ✅ | — | File path (relative or absolute) |
| `start_line` | number | | — | Start line (1-based, inclusive) |
| `end_line` | number | | — | End line (1-based, inclusive) |

---

### `coraline_list_dir`

List the contents of a directory within the project.

**Input:**

| Parameter | Type | Required | Default | Description |
|---|---|---|---|---|
| `path` | string | | `.` | Directory path (relative or absolute) |
| `recursive` | boolean | | `false` | Recurse into subdirectories |

---

### `coraline_get_file_nodes`

Get all indexed symbols (nodes) for a specific file.

**Input:**

| Parameter | Type | Required | Description |
|---|---|---|---|
| `file_path` | string | ✅ | File path (relative or absolute) |

**Output:** `{ "file_path": "...", "nodes": [...], "count": N }`

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
