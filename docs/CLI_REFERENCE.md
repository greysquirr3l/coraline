# Coraline CLI Reference

Coraline is invoked as `coraline [COMMAND] [OPTIONS] [PATH]`.

When `[PATH]` is omitted, the current working directory is used as the project root.

---

## Commands at a Glance

| Command | Description |
|---|---|
| `init` | Initialize a new project |
| `index` | Full reindex of the project |
| `sync` | Incremental update (git-diff based) |
| `status` | Show project status and paths |
| `stats` | Show index statistics |
| `query` | Search symbols by name |
| `context` | Build AI context for a task |
| `callers` | Find what calls a node |
| `callees` | Find what a node calls |
| `impact` | Analyze change impact radius |
| `config` | Read or update configuration |
| `hooks` | Manage git hooks |
| `serve` | Start the MCP server |
| `update` | Check for available updates on crates.io |
| `embed` | Generate vector embeddings for indexed nodes |
| `model` | Manage the ONNX embedding model |

---

## `coraline init [PATH]`

Initialize Coraline in a project directory. Creates `.coraline/` with a SQLite database, default `config.toml`, and initial memory templates.

When stdin is a TTY, prompts to download the embedding model (~137 MB) after initialization. Decline to skip — all non-embedding tools remain fully functional and you can download later with `coraline model download`.

If `.coraline/` already exists and `--index` is passed **without** `--force`, `init` skips the overwrite and runs indexing directly on the existing project.

**Options:**

| Flag | Description |
|---|---|
| `-i`, `--index` | Run a full index immediately after initialization |
| `-f`, `--force` | Overwrite an existing `.coraline/` directory without prompting |
| `--no-hooks` | Skip automatic git hook installation |

**Examples:**
```bash
coraline init                    # Initialize current directory
coraline init /path/to/my-app   # Initialize a specific path
coraline init -i                 # Initialize, prompt for model, then index
coraline init -i --no-hooks      # Initialize and index, skip git hooks
coraline init --force            # Wipe and reinitialize existing project
```

**On success, creates:**
- `.coraline/coraline.db` — SQLite knowledge graph
- `.coraline/config.toml` — Annotated config template
- `.coraline/memories/` — Initial memory files
- `.coraline/.gitignore` — Excludes local data files from git
- `.git/hooks/post-commit` — Auto-sync hook (unless `--no-hooks`)

---

## `coraline index [PATH]`

Perform a full reindex of the project. Parses all matching source files, extracts symbols and edges, resolves cross-file references, and stores results in the knowledge graph.

**Options:**

| Flag | Description |
|---|---|
| `-f`, `--force` | Force re-parse all files, even unchanged ones |
| `-q`, `--quiet` | Suppress progress output |

**Examples:**
```bash
coraline index                   # Index current directory
coraline index /path/to/project  # Index a specific path
coraline index -f                # Force full re-parse
coraline index -q                # Silent (useful in scripts)
```

---

## `coraline sync [PATH]`

Perform an incremental update using git-diff to identify changed files. Faster than a full `index` for routine updates.

**Options:**

| Flag | Description |
|---|---|
| `-q`, `--quiet` | Suppress progress output |

**Examples:**
```bash
coraline sync                    # Sync current directory
coraline sync -q                 # Silent sync (used by git hook)
```

---

## `coraline status [PATH]`

Show the current project status: initialization state, paths to config and database, database size, and git hook status.

**Examples:**
```bash
coraline status
```

**Sample output:**
```
Coraline Status

Project: /home/user/my-app
Config:  /home/user/my-app/.coraline/config.toml
Database: /home/user/my-app/.coraline/coraline.db (1048576 bytes)
Git hooks: installed
```

---

## `coraline stats [PATH]`

Show index statistics: file count, node count, edge count, and unresolved reference count.

**Options:**

| Flag | Description |
|---|---|
| `-j`, `--json` | Output as JSON |

**Examples:**
```bash
coraline stats
coraline stats --json
```

**Sample output:**
```
Coraline Statistics

Files:     128
Nodes:     4201
Edges:     9872
Unresolved refs: 153
```

---

## `coraline query <SEARCH> [PATH]`

Search for symbols in the knowledge graph by name. Uses SQLite full-text search (FTS5) for fast, fuzzy matching.

**Arguments:**

| Argument | Description |
|---|---|
| `SEARCH` | Symbol name or search pattern |

**Options:**

| Flag | Description |
|---|---|
| `-p`, `--path PATH` | Project root path |
| `-l`, `--limit N` | Maximum results (default: `10`) |
| `-k`, `--kind KIND` | Filter by node kind (see below) |
| `-j`, `--json` | Output as JSON |

**Valid `KIND` values:**
`file`, `module`, `class`, `struct`, `interface`, `trait`, `protocol`, `function`, `method`, `property`, `field`, `variable`, `constant`, `enum`, `enum_member`, `type_alias`, `namespace`, `parameter`, `import`, `export`, `route`, `component`

**Examples:**
```bash
coraline query resolve_unresolved
coraline query "index" --kind function --limit 5
coraline query Auth --json
```

---

## `coraline context <TASK> [PATH]`

Build structured context for an AI task description. Searches the graph, traverses relationships, and returns relevant code snippets.

**Arguments:**

| Argument | Description |
|---|---|
| `TASK` | Natural language task description |

**Options:**

| Flag | Description |
|---|---|
| `-p`, `--path PATH` | Project root path |
| `-n`, `--max-nodes N` | Max graph nodes (default: `50`) |
| `-c`, `--max-code N` | Max code blocks (default: `10`) |
| `--no-code` | Omit source code snippets |
| `-f`, `--format FMT` | `markdown` (default) or `json` |

**Examples:**
```bash
coraline context "add authentication middleware"
coraline context "how does indexing work" --format json
coraline context "refactor database layer" --max-nodes 30 --max-code 5
```

---

## `coraline callers <NODE_ID> [PATH]`

Find all nodes that call the specified node (incoming `calls` edges).

**Arguments:**

| Argument | Description |
|---|---|
| `NODE_ID` | Node ID (from `query` or `stats --json` output) |

**Options:**

| Flag | Description |
|---|---|
| `-p`, `--path PATH` | Project root path |
| `-l`, `--limit N` | Maximum results (default: `20`) |
| `-j`, `--json` | Output as JSON |

**Examples:**
```bash
coraline callers abc123
coraline callers abc123 --limit 50 --json
```

---

## `coraline callees <NODE_ID> [PATH]`

Find all nodes that the specified node calls (outgoing `calls` edges).

Same flags as `callers`.

---

## `coraline impact <NODE_ID> [PATH]`

Analyze the impact radius of a symbol — what would be affected if it changed. Performs a BFS over incoming edges up to `--depth` hops.

**Arguments:**

| Argument | Description |
|---|---|
| `NODE_ID` | Node ID to analyze |

**Options:**

| Flag | Description |
|---|---|
| `-p`, `--path PATH` | Project root path |
| `-d`, `--depth N` | BFS depth (default: `3`) |
| `-j`, `--json` | Output as JSON |

**Examples:**
```bash
coraline impact abc123
coraline impact abc123 --depth 5 --json
```

---

## `coraline config [PATH]`

Read or update the project configuration at `.coraline/config.toml`.

**Options:**

| Flag | Description |
|---|---|
| `-p`, `--path PATH` | Project root path |
| `-j`, `--json` | Print config as JSON |
| `-s`, `--section SEC` | Print only a section (`indexing`, `context`, `sync`, `vectors`) |
| `--set KEY=VALUE` | Set a value: `section.key=value` |

**Examples:**
```bash
coraline config                                 # Print full config (TOML)
coraline config --section context               # Print one section
coraline config --json                          # Print as JSON
coraline config --set context.max_nodes=30      # Update a value
coraline config --set indexing.batch_size=50
coraline config --set vectors.enabled=true
```

---

## `coraline hooks <ACTION> [PATH]`

Manage the git `post-commit` hook that runs `coraline sync` automatically after each commit.

**Actions:**

| Action | Description |
|---|---|
| `install` | Install the hook (backs up existing hook) |
| `remove` | Remove the hook (restores backup if present) |
| `status` | Show whether the hook is installed |

**Options:**

| Flag | Description |
|---|---|
| `-p`, `--path PATH` | Project root path |

**Examples:**
```bash
coraline hooks install
coraline hooks status
coraline hooks remove
```

---

## `coraline serve [PATH]`

Start the MCP server. With `--mcp`, communicates over stdio using the Model Context Protocol.

**Options:**

| Flag | Description |
|---|---|
| `-p`, `--path PATH` | Project root path |
| `--mcp` | Start MCP stdio server (required) |

**Examples:**
```bash
coraline serve --mcp
coraline serve --mcp --path /path/to/project
```

Typically invoked by an MCP client (Claude Desktop, Claude Code, etc.) rather than directly.

---

## `coraline update`

Check whether a newer version of Coraline is published on crates.io. Compares the running binary version against the latest release and prints upgrade instructions when an update is available.

**Options:** None.

**Examples:**
```bash
coraline update
```

**Output (when up to date):**
```
✓ coraline is up to date (v0.3.0)
```

**Output (when update available):**
```
Update available: v0.3.0 → v0.4.0
Run `cargo install coraline` to upgrade.
```

---

## Environment Variables

| Variable | Description |
|---|---|
| `CORALINE_LOG` | Log level filter (default: `coraline=info`). Examples: `debug`, `coraline=trace`, `warn` |

**Examples:**
```bash
CORALINE_LOG=debug coraline index
CORALINE_LOG=coraline=trace coraline serve --mcp
```

Logs are written to `.coraline/logs/coraline.log` (daily rotating) and to stderr at the configured level.

---

## `coraline embed [PATH]`

Generate vector embeddings for all indexed nodes using the local ONNX model. Embeddings enable the `coraline_semantic_search` MCP tool.

By default, `embed` performs a lightweight freshness check and runs incremental `sync` first when indexed state is stale. This keeps embeddings aligned with current source files without requiring a manual `coraline sync` step.

**Options:**

| Flag | Description |
|---|---|
| `--download` | Download the model automatically before embedding |
| `--variant FILENAME` | ONNX variant to download (default: `model_int8.onnx`) |
| `--skip-sync` | Skip automatic pre-embed sync check (embeddings may be stale) |
| `--batch-size N` | Nodes per progress batch (default: `50`) |
| `-q`, `--quiet` | Suppress progress output |

**Examples:**
```bash
coraline embed                        # Embed using already-downloaded model
coraline embed --skip-sync            # Skip auto-sync and embed current index state
coraline embed --download             # Download model_int8.onnx then embed
coraline embed --download --variant model_fp16.onnx
```

Run `coraline index` first. Models are stored in `.coraline/models/nomic-embed-text-v1.5/`.

---

## `coraline model [PATH]`

Manage the ONNX embedding model files.

### `coraline model download`

Download model files from HuggingFace (`nomic-ai/nomic-embed-text-v1.5`).

| Flag | Description |
|---|---|
| `--variant FILENAME` | ONNX variant to download (default: `model_int8.onnx`) |
| `-f`, `--force` | Re-download even if files already exist |
| `-q`, `--quiet` | Suppress progress output |

Downloads `tokenizer.json`, `tokenizer_config.json`, and the chosen ONNX weights into `.coraline/models/nomic-embed-text-v1.5/`.

**Available variants (smallest to largest):**

| Variant | Size | Notes |
|---|---|---|
| `model_q4f16.onnx` | ~111 MB | Q4 + fp16 mixed (smallest) |
| `model_int8.onnx` | ~137 MB | int8 quantized (recommended) |
| `model_fp16.onnx` | ~274 MB | fp16 |
| `model.onnx` | ~547 MB | full f32 |

### `coraline model status`

Show which model files are present in the model directory.

```bash
coraline model status
```
