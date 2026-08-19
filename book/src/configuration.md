# Coraline Configuration Reference

Coraline stores its configuration in `.coraline/config.toml` within each project. All settings are optional — a missing or empty file uses sensible defaults.

---

## File Locations

| File | Purpose |
|---|---|
| `.coraline/config.toml` | User-editable configuration (all settings) |
| `.coraline/coraline.db` | SQLite knowledge graph (do not edit) |
| `.coraline/memories/` | Project memory files (Markdown) |
| `.coraline/logs/` | Daily-rotating log files |

> **Migration from v0.3.x or earlier:** If you have a legacy `config.json` file, run `coraline config --migrate` to convert it to `config.toml`. The old file will be backed up as `config.json.backup`.

---

## Full Default Configuration

```toml
# Coraline project configuration
# All settings are optional — defaults are shown below.

[indexing]
max_file_size = 1048576   # 1 MB — skip files larger than this
batch_size    = 100       # Files processed per batch
include_patterns = [
  "**/*.rs", "**/*.ts", "**/*.tsx", "**/*.js", "**/*.jsx",
  "**/*.py", "**/*.go", "**/*.java", "**/*.cs", "**/*.cpp",
  "**/*.c", "**/*.h", "**/*.rb", "**/*.php", "**/*.swift",
  "**/*.kt", "**/*.razor",
]
exclude_patterns = [
  "**/.git/**", "**/target/**", "**/node_modules/**",
  "**/dist/**", "**/build/**", "**/.coraline/**",
]

[context]
max_nodes          = 20    # Max graph nodes in context output
max_code_blocks    = 5     # Max code snippets to attach
max_code_block_size = 1500 # Max chars per code block
traversal_depth    = 1     # Graph hops from entry nodes

[sync]
git_hooks_enabled = true   # Auto-sync on git commit
watch_mode        = false  # Watch for file changes (not yet implemented)
debounce_ms       = 500    # Watch mode debounce delay

[vectors]
enabled    = false                  # Requires ONNX model (see below)
model      = "nomic-embed-text-v1.5" # or "jina-embeddings-v2-base-code"
dimension  = 768
batch_size = 32
```

---

## `[indexing]` Section

Controls which files are indexed and how.

### `max_file_size`

Files larger than this are skipped during indexing.

- **Type:** integer (bytes)
- **Default:** `1048576` (1 MB)

```toml
[indexing]
max_file_size = 524288   # 512 KB
```

### `batch_size`

Number of files processed per indexing batch.

- **Type:** integer
- **Default:** `100`

### `include_patterns`

Glob patterns for files to include. Uses `**` for recursive matching.

- **Type:** array of strings
- **Default:** Common source file extensions for 18+ languages

```toml
[indexing]
include_patterns = [
  "**/*.rs",
  "**/*.ts",
  "**/*.tsx",
  "src/**/*.js",
]
```

### `exclude_patterns`

Glob patterns for paths to exclude. Matched against the full relative file path.

- **Type:** array of strings
- **Default:** Build artifacts, package caches, generated files, IDE directories

```toml
[indexing]
exclude_patterns = [
  "**/.git/**",
  "**/node_modules/**",
  "**/target/**",
  "**/dist/**",
  "tests/fixtures/**",  # custom exclusion
]
```

> **Tip:** The default exclusion list is extensive. You typically only need to add project-specific paths.
>
> **Python virtualenvs:** The defaults cover `.venv/`, `venv/`, `env/`, and named venvs matching `*_venv/` or `*-venv/` (e.g. `cluster_venv/`, `memory_venv/`). If you use a different naming convention, add it explicitly:
> ```toml
> exclude_patterns = ["**/my_custom_env/**"]
> ```

---

## `[context]` Section

Controls the output of `coraline context` (CLI) and `coraline_context` (MCP).

### `max_nodes`

Maximum number of graph nodes to include in a context response.

- **Type:** integer
- **Default:** `20`

### `max_code_blocks`

Maximum number of source code snippets to attach.

- **Type:** integer
- **Default:** `5`

### `max_code_block_size`

Maximum characters per code block. Larger bodies are truncated.

- **Type:** integer
- **Default:** `1500`

### `traversal_depth`

How many graph hops to follow from the initial matching nodes.

- **Type:** integer
- **Default:** `1`

```toml
[context]
max_nodes          = 40
max_code_blocks    = 10
max_code_block_size = 2000
traversal_depth    = 2
```

---

## `[sync]` Section

Controls incremental sync behavior and git hook integration.

### `git_hooks_enabled`

Whether `coraline init` installs a `post-commit` hook that automatically runs `coraline sync` after each commit.

- **Type:** boolean
- **Default:** `true`

> **Note:** This only determines whether `coraline init` auto-installs the hook. To manage the hook manually, use `coraline hooks install|remove|status`.

### `watch_mode`

Enable file-system watch mode (re-index on changes). Not yet implemented.

- **Type:** boolean
- **Default:** `false`

### `debounce_ms`

Debounce delay for watch mode in milliseconds.

- **Type:** integer
- **Default:** `500`

---

## `[vectors]` Section

Controls vector embedding generation for semantic search (`coraline embed`, the `coraline_semantic_search` MCP tool). Requires a build with the `embeddings` or `embeddings-dynamic` feature.

### `enabled`

Enable vector embedding generation.

- **Type:** boolean
- **Default:** `false`

### `model`

Which embedding model to use. Run `coraline model list` to see every supported model with its dimension and description.

- **Type:** string
- **Default:** `"nomic-embed-text-v1.5"` — general-purpose English text embeddings.
- **Also supported:** `"jina-embeddings-v2-base-code"` — code-specialised embeddings, opt-in.

Model files live in a shared directory (`~/.config/coraline/models/<model>/`, or `model_dir` below), not per-project — every project on the machine reuses the same downloaded weights for a given model. Run `coraline model download` (add `--model <name>` to target a non-default model) to fetch it.

> **Switching models:** changing `model` on a project that already has embeddings does not retroactively re-embed existing nodes. Every read path (`coraline_semantic_search`, `coraline doctor`'s coverage probe, staleness checks) filters by the configured model, so a switch makes every node look unembedded for the *new* model until you re-run `coraline embed`. This is a real (if usually quick) re-embedding pass, not instant.

### `dimension`

Embedding vector dimension. Must match the selected model — both currently supported models emit 768-dim vectors.

- **Type:** integer
- **Default:** `768`

### `batch_size`

Number of symbols embedded per batch.

- **Type:** integer
- **Default:** `32`

### `model_dir`

Override the on-disk model directory. Unset by default — Coraline resolves it from `model` (`~/.config/coraline/models/<model>/`).

- **Type:** string (path), optional

### `model_file`

Pin a specific ONNX filename instead of auto-detecting the best available variant for the configured `model`.

- **Type:** string, optional

### `max_seq_len`

Maximum sequence length in tokens fed to the model.

- **Type:** integer
- **Default:** `512`

---

## CLI Configuration Commands

Read the full config:
```bash
coraline config
```

Read a specific section:
```bash
coraline config --section context
coraline config --section indexing
```

Output as JSON:
```bash
coraline config --json
```

Update a value:
```bash
coraline config --set context.max_nodes=30
coraline config --set indexing.batch_size=50
coraline config --set indexing.max_file_size=524288
coraline config --set vectors.enabled=true
```

The `--set` flag accepts `section.key=value` syntax. Values are parsed as JSON when possible (for booleans, numbers, and arrays), otherwise treated as strings.

Migrate from legacy config.json:
```bash
coraline config --migrate
```

This converts an old `config.json` file to the new `config.toml` format and backs up the original as `config.json.backup`.

---

## MCP Configuration Tools

The same operations are available via MCP:

```
coraline_get_config          → returns full config as JSON
coraline_update_config       → updates a single key
  key: "context.max_nodes"
  value: 30
```

---

## Example: Focused TypeScript Project

```toml
[indexing]
include_patterns = [
  "src/**/*.ts",
  "src/**/*.tsx",
  "tests/**/*.ts",
]
exclude_patterns = [
  "**/node_modules/**",
  "**/dist/**",
  "**/.next/**",
  "src/**/*.d.ts",
]

[context]
max_nodes = 30
max_code_blocks = 8
traversal_depth = 2
```

## Example: Large Rust Workspace

```toml
[indexing]
max_file_size = 2097152   # 2 MB for generated code
batch_size = 200
include_patterns = [
  "crates/**/*.rs",
]
exclude_patterns = [
  "**/target/**",
  "**/.git/**",
]

[context]
max_nodes = 50
traversal_depth = 3
```
