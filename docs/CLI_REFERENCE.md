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
| `doctor` | Run diagnostic checks (config, database, model, embed coverage) |
| `serve` | Start the MCP server |
| `update` | Check for available updates on crates.io |
| `embed` | Generate vector embeddings for indexed nodes |
| `model` | Manage the ONNX embedding model |

---

## `coraline init [PATH]`

Initialize Coraline in a project directory. Creates `.coraline/` with a SQLite database, default `config.toml`, and initial memory templates.

When stdin is a TTY and no model decision flag is given, prompts to download the embedding model (~137 MB) after initialization. Decline to skip — all non-embedding tools remain fully functional and you can download later with `coraline model download`. Use `--embed`, `--no-embed`, or `--yes` to make the decision non-interactively (see below); if the model is already present on disk, `init` skips the prompt entirely regardless of flags.

If `.coraline/` already exists and `--index` is passed **without** `--force`, `init` skips the overwrite and runs indexing directly on the existing project.

**Options:**

| Flag | Description |
|---|---|
| `-i`, `--index` | Run a full index immediately after initialization |
| `-f`, `--force` | Overwrite an existing `.coraline/` directory without prompting |
| `--no-hooks` | Skip automatic git hook installation |
| `--embed` | Download the embedding model during init (skips the TTY prompt) |
| `--no-embed` | Skip the embedding model entirely — no prompt, no download. Conflicts with `--embed` and always wins over `--yes` |
| `-y`, `--yes` | Non-interactive mode: auto-accept the model download prompt |

**Examples:**
```bash
coraline init                    # Initialize current directory
coraline init /path/to/my-app   # Initialize a specific path
coraline init -i                 # Initialize, prompt for model, then index
coraline init -i --no-hooks      # Initialize and index, skip git hooks
coraline init --force            # Wipe and reinitialize existing project
coraline init --embed            # Initialize and download the embedding model, no prompt
coraline init --no-embed         # Initialize, skip the model entirely, no prompt
coraline init --yes              # Initialize non-interactively; auto-downloads the model
```

> Which model is downloaded is controlled by `vectors.model` in config (default `nomic-embed-text-v1.5`), not by an `init` flag — see `coraline model` below and the `[vectors]` section in [CONFIGURATION.md](CONFIGURATION.md).

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

Show the current project status: initialization state, paths to config and database, database size, embedding-model state, and git hook status.

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
Embeddings: model_quantized.onnx (161 MB)
Model:      jina-embeddings-v2-base-code
Model dir:  /home/user/.config/coraline/models/jina-embeddings-v2-base-code
Git hooks: installed
```

The `Embeddings` line shows `not present` (with a fix hint) when no model file has been downloaded yet for the project's configured model (`vectors.model`, default `nomic-embed-text-v1.5`). See `coraline model` and `coraline doctor` below.

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

## `coraline doctor [PATH]`

Run diagnostic checks against a project and report pass/fail per check, with a fix hint for anything failing. Exits `0` if every check passed, `1` otherwise — safe to use as a CI gate.

**Checks (in order):**

| Check | What it verifies |
|---|---|
| `config` | `.coraline/config.toml` exists and is readable |
| `database` | `.coraline/coraline.db` opens and returns stats (node/edge/file counts) |
| `git hooks` | The post-commit hook is installed (or the project isn't a git repo, which is fine) |
| `model file` | At least one ONNX variant of the configured model (`vectors.model`) is present on disk |
| `model loads` *(deep only)* | The ONNX model actually loads into an inference session |
| `inference` *(deep only)* | A sample embedding runs through the loaded model successfully |
| `embed coverage` *(deep only)* | Every indexed node has an embedding for the configured model |

The three deep checks require a build with the `embeddings` or `embeddings-dynamic` feature; they're skipped (not failed) on builds without it.

**Options:**

| Flag | Description |
|---|---|
| `--quick` | Skip the three deep model-load/inference/coverage checks (fast, no ONNX runtime needed) |
| `--deep` | Explicit deep mode — this is already the default; mutually exclusive with `--quick` |
| `--json` | Print the report as JSON instead of the human-readable `✔`/`✘` list |

**Examples:**
```bash
coraline doctor                  # Full diagnostic run (deep checks included)
coraline doctor --quick          # Skip slow model checks — good for CI
coraline doctor --json           # Machine-readable report
```

**Sample output:**
```
✔  config
✔  database
✔  git hooks
✘  model file
    → Run `coraline model download`.
```

**Sample `--json` output:**
```json
{
  "probes": [
    { "name": "config", "ok": true, "detail": "/path/.coraline/config.toml (3775 bytes)" },
    { "name": "database", "ok": true, "detail": "/path/.coraline/coraline.db (2 nodes, 1 edges, 1 files)" },
    { "name": "git hooks", "ok": true, "detail": "installed" },
    { "name": "model file", "ok": false, "detail": "no model file for 'nomic-embed-text-v1.5' in ...", "fix": "Run `coraline model download`." }
  ],
  "exit_code": 1
}
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

Generate vector embeddings for every currently indexed node using the local ONNX model. Embeddings enable the `coraline_semantic_search` MCP tool.

`embed` does not run `sync` itself — run `coraline sync` (or `coraline index`) first if source files have changed since the last index. The `coraline_semantic_search` MCP tool, by contrast, performs its own lightweight freshness check and incremental sync/re-embed on each call, so MCP clients don't need a manual `coraline sync` step.

**Options:**

| Flag | Description |
|---|---|
| `--download` | Download the model automatically before embedding |
| `--variant FILENAME` | ONNX variant to download (default: the configured model's recommended variant) |
| `--batch-size N` | Nodes per progress batch (default: `50`) |
| `-q`, `--quiet` | Suppress progress output |

**Examples:**
```bash
coraline embed                        # Embed using already-downloaded model
coraline embed --download             # Download the configured model then embed
coraline embed --download --variant model_fp16.onnx
```

Run `coraline index` first. Uses the model configured by `vectors.model` (default `nomic-embed-text-v1.5`), stored in `~/.config/coraline/models/<model>/` — see `coraline model list`.

---

## `coraline model [PATH]`

Manage embedding model files. Supports multiple models — run `coraline model list` to see the registry.

### `coraline model list`

List every embedding model Coraline knows how to download and run, with dimension and description.

```bash
coraline model list
```

### `coraline model download`

Download model files from HuggingFace.

| Flag | Description |
|---|---|
| `--model NAME` | Which supported model to download (default: `vectors.model` from config.toml) |
| `--variant FILENAME` | ONNX variant to download (default: the model's recommended variant) |
| `-f`, `--force` | Re-download even if files already exist |
| `-q`, `--quiet` | Suppress progress output |

Downloads `tokenizer.json`, `tokenizer_config.json`, and the chosen ONNX weights into the shared model directory `~/.config/coraline/models/<model>/`.

**`nomic-embed-text-v1.5` variants (smallest to largest):**

| Variant | Size | Notes |
|---|---|---|
| `model_q4f16.onnx` | ~111 MB | Q4 + fp16 mixed (smallest) |
| `model_int8.onnx` | ~137 MB | int8 quantized (recommended) |
| `model_fp16.onnx` | ~274 MB | fp16 |
| `model.onnx` | ~547 MB | full f32 |

**`jina-embeddings-v2-base-code` variants:**

| Variant | Size | Notes |
|---|---|---|
| `model_quantized.onnx` | ~162 MB | int8 quantized (recommended) |
| `model_fp16.onnx` | ~321 MB | fp16 |
| `model.onnx` | ~642 MB | full f32 |

```bash
coraline model download                                    # download vectors.model's default variant
coraline model download --model jina-embeddings-v2-base-code
```

### `coraline model status`

Show which model files are present in the model directory for the configured (or `--model`-selected) model.

| Flag | Description |
|---|---|
| `--model NAME` | Which supported model to inspect (default: `vectors.model` from config.toml) |

```bash
coraline model status
coraline model status --model jina-embeddings-v2-base-code
```
