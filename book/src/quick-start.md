# Quick Start Guide

Get Coraline running on your project in under 5 minutes.

## 1. Initialize a Project

Navigate to your project directory and initialize Coraline:

```bash
cd your-project
coraline init -i
```

The `-i` flag runs indexing immediately after initialization.

You'll see output like:

```
Initializing Coraline in /path/to/your-project
Created .coraline/ directory
Created database at .coraline/coraline.db
Created config at .coraline/config.toml
Installed git post-commit hook

Would you like to download the embedding model (~137 MB)? [y/N]:
```

You can decline the embedding model for now - all other features work without it.

## 2. Verify the Index

Check the indexing results:

```bash
coraline stats
```

Output:

```
Coraline Statistics

Files:     128
Nodes:     4201
Edges:     9872
Unresolved refs: 153
```

## 3. Search for Symbols

Try searching for a function or class:

```bash
coraline query "authenticate"
coraline query "User" --kind class
```

## 4. Find Relationships

Get the node ID from the query output, then explore relationships:

```bash
# Find what calls a function
coraline callers <node-id>

# Find what a function calls
coraline callees <node-id>

# Analyze impact of changing a symbol
coraline impact <node-id>
```

## 5. Build Context for AI

Generate context for a coding task:

```bash
coraline context "add authentication middleware"
coraline context "how does the database layer work"
```

This outputs a Markdown document with relevant code snippets ready to paste into an AI assistant.

## Common Workflows

### Daily Development

With git hooks installed, Coraline automatically syncs after each commit:

```bash
git add .
git commit -m "Add feature"
# Coraline syncs automatically
```

Or manually sync changes:

```bash
coraline sync
```

### Exploring a New Codebase

```bash
# Index the project
coraline init -i

# Get overview
coraline stats

# Search for entry points
coraline query "main" --kind function

# Find what main calls
coraline callees <main-node-id>

# Build context for understanding a feature
coraline context "how does the authentication system work"
```

### Refactoring

```bash
# Find a symbol to refactor
coraline query "old_function_name"

# Check impact
coraline impact <node-id>

# Find all callers
coraline callers <node-id>

# After making changes, sync
coraline sync
```

### Code Review

```bash
# Sync to latest
coraline sync

# Check impact of changed symbols
coraline impact <changed-node-id>

# Find test coverage
coraline query "test_authentication" --kind function
coraline callees <test-node-id>
```

## Incremental vs Full Indexing

**Incremental sync** (fast, recommended):
```bash
coraline sync
```

Uses git to detect changes and only re-indexes modified files.

**Full reindex** (slower, comprehensive):
```bash
coraline index
```

Re-parses all files. Use when:
- Switching branches with many changes
- After updating configuration
- If sync results seem incorrect

## Optional: Semantic Search

Download the embedding model and generate vectors:

```bash
coraline model download
coraline embed
```

This enables natural language search:

```bash
# Via CLI (when MCP tools are used)
coraline serve --mcp
# Then use coraline_semantic_search tool
```

Semantic search is most useful in MCP integration - see [MCP Integration](./mcp-integration.md).

## Configuration

Customize indexing behavior by editing `.coraline/config.toml`:

```toml
[indexing]
include_patterns = [
  "src/**/*.rs",
  "lib/**/*.ts",
]
exclude_patterns = [
  "**/test/**",
  "**/node_modules/**",
]

[context]
max_nodes = 30
max_code_blocks = 10
```

See [Configuration Guide](./configuration.md) for all options.

## Project Memories

Store persistent context about your project:

```bash
# Via MCP tools (recommended)
# Use coraline_write_memory, coraline_read_memory, etc.

# Or manually create files in .coraline/memories/
echo "# Project Overview" > .coraline/memories/overview.md
```

Memories help AI assistants maintain context across sessions.

## Next Steps

### For CLI Usage
- [CLI Reference](./cli-reference.md) - Complete command documentation
- [Configuration Guide](./configuration.md) - Customize behavior
- [Performance Tips](./performance.md) - Optimize for large projects

### For AI Assistant Integration
- [MCP Integration](./mcp-integration.md) - Connect to Claude Desktop/Code
- [OpenCode Integration](./opencode.md) - Use with OpenCode
- [MCP Tools Reference](./mcp-tools.md) - Explore available tools

## Troubleshooting

### "Project not initialized"

Run `coraline init` first.

### "No results found"

Ensure files match `include_patterns` in `.coraline/config.toml`. Check:

```bash
coraline status
```

### Slow indexing

For large projects (10,000+ files):
1. Use more specific `include_patterns`
2. Increase `batch_size` in config
3. Exclude test/fixture directories

### Out-of-date results

Run a full reindex:

```bash
coraline index --force
```

### Git hooks not working

Reinstall hooks:

```bash
coraline hooks install
```

Or check hook status:

```bash
coraline hooks status
```
