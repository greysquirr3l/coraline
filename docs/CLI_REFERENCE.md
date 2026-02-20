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

---

## `coraline init [PATH]`

Initialize Coraline in a project directory. Creates `.coraline/` with a SQLite database, default `config.toml`, and initial memory templates.

**Options:**

| Flag | Description |
|---|---|
| `-i`, `--index` | Run a full index immediately after initialization |
| `--no-hooks` | Skip automatic git hook installation |

**Examples:**
```bash
coraline init                    # Initialize current directory
coraline init /path/to/my-app   # Initialize a specific path
coraline init -i                 # Initialize and index immediately
coraline init --no-hooks         # Initialize without git hooks
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
