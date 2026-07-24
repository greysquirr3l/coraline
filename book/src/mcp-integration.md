# MCP Integration

Coraline exposes its code intelligence capabilities via the Model Context Protocol (MCP), enabling AI assistants to understand and navigate your codebase.

## What is MCP?

The [Model Context Protocol](https://modelcontextprotocol.io/) is a standard for connecting AI assistants to external data sources and tools. Coraline implements MCP to expose 33 code intelligence tools that AI assistants can use.

## Supported Clients

Coraline works with any MCP-compatible client:

- **Claude Desktop** - Official Anthropic desktop app
- **Claude Code** - CLI-based Claude integration
- **OpenCode** - Multi-model terminal AI assistant (see [OpenCode Integration](./opencode.md))
- Any other MCP-compatible client

## Claude Desktop

Claude Desktop is the official Anthropic desktop application with built-in MCP support.

### Configuration

Add Coraline to Claude Desktop's MCP configuration:

**macOS**: `~/Library/Application Support/Claude/claude_desktop_config.json`
**Windows**: `%APPDATA%\Claude\claude_desktop_config.json`
**Linux**: `~/.config/Claude/claude_desktop_config.json`

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

Replace `/path/to/coraline` with the full path to your Coraline binary:

```bash
# Find your Coraline path
which coraline
```

Common paths:
- Cargo install: `~/.cargo/bin/coraline`
- macOS Homebrew: `/opt/homebrew/bin/coraline`
- System install: `/usr/local/bin/coraline`

### Multiple Projects

To work with multiple projects, add multiple servers:

```json
{
  "mcpServers": {
    "coraline-projectA": {
      "command": "/Users/you/.cargo/bin/coraline",
      "args": ["serve", "--mcp", "--path", "/Users/you/projectA"]
    },
    "coraline-projectB": {
      "command": "/Users/you/.cargo/bin/coraline",
      "args": ["serve", "--mcp", "--path", "/Users/you/projectB"]
    }
  }
}
```

### Restart Claude Desktop

After editing the config, restart Claude Desktop to load the new MCP server.

### Verify Connection

In Claude Desktop, you should see MCP tools available. Try a prompt like:

```
Search for authentication functions in my codebase
```

Claude should use the `coraline_search` tool automatically.

## Claude Code

Claude Code is a terminal-based CLI for Claude with MCP support.

### Configuration

Create or edit `.claude/mcp.json` in your **workspace directory**:

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

When `--path` is omitted, Coraline uses the current working directory as the project root.

### Per-Project Setup

Each project can have its own MCP configuration:

```bash
cd your-project
mkdir -p .claude
cat > .claude/mcp.json << 'EOF'
{
  "mcpServers": {
    "coraline": {
      "command": "coraline",
      "args": ["serve", "--mcp"]
    }
  }
}
EOF
```

### Usage

Start Claude Code in your project:

```bash
cd your-project
claude
```

Claude will automatically connect to Coraline and have access to all tools.

### Example Prompts

```
Find all functions that call authenticate()
Show me the impact of changing the User class
Build context for adding a new payment method
What files are in the src/auth directory?
```

## Environment Variables

### Logging

Enable debug logging for troubleshooting:

```json
{
  "mcpServers": {
    "coraline": {
      "command": "coraline",
      "args": ["serve", "--mcp"],
      "env": {
        "CORALINE_LOG": "debug"
      }
    }
  }
}
```

Logs are written to `.coraline/logs/coraline.log`.

### Timeout Configuration

For long-running operations, increase the timeout (in milliseconds):

```json
{
  "mcpServers": {
    "coraline": {
      "command": "coraline",
      "args": ["serve", "--mcp", "--timeout", "300000"]
    }
  }
}
```

Default: 120000ms (2 minutes)
Maximum: 600000ms (10 minutes)

See [OpenCode Integration](./opencode.md) for more timeout details.

## Available Tools

Once configured, AI assistants have access to 33 tools:

**Core Tools:**
- `coraline_search` - Find symbols by name
- `coraline_semantic_search` - Natural language search (requires embeddings)
- `coraline_callers` - Find what calls a symbol
- `coraline_callees` - Find what a symbol calls
- `coraline_impact` - Analyze change impact
- `coraline_context` - Build context for a task

**Advanced:**
- `coraline_batch_get_nodes` - Fetch multiple nodes (60-90% token savings)
- `coraline_batch_callers` - Batch caller queries
- `coraline_batch_callees` - Batch callee queries
- `coraline_search_by_signature` - Search by type signature
- `coraline_search_by_docstring` - Search docstrings
- `coraline_search_exported_symbols` - Public API only

**File & Config:**
- `coraline_read_file` - Read file contents
- `coraline_list_dir` - List directory
- `coraline_get_file_nodes` - Symbols in a file
- `coraline_status` - Project statistics
- `coraline_sync` - Incremental update
- `coraline_get_config` / `coraline_update_config` - Configuration

**Memory:**
- `coraline_write_memory` - Create/update memories
- `coraline_read_memory` - Read memories
- `coraline_list_memories` - List all memories
- `coraline_delete_memory` - Delete a memory
- `coraline_edit_memory` - Edit via find/replace

See [MCP Tools Reference](./mcp-tools.md) for complete documentation.

## Best Practices

### Initialize Before Connecting

Always initialize and index your project before connecting MCP clients:

```bash
cd your-project
coraline init -i
```

### Keep Index Updated

Enable git hooks for automatic updates:

```bash
coraline hooks install
```

Or manually sync after changes:

```bash
coraline sync
```

### Use Compact Output

All tools support `output_format: "compact"` for 65% token reduction:

```
Search for User class with compact output format
```

The AI assistant will use compact format automatically when instructed.

### Batch Queries

For multiple lookups, ask the AI to use batch tools:

```
Use batch tools to get details for these 10 functions
```

This saves 60-90% of tokens compared to individual queries.

### Project Memories

Use memories to maintain context across sessions:

```
Write a memory about the authentication architecture
Update the architecture_notes memory with the new JWT approach
```

## Troubleshooting

### Tools not appearing

1. Check that Coraline is in your PATH:
   ```bash
   which coraline
   ```

2. Verify the config path is correct

3. Restart the MCP client

4. Check logs in `.coraline/logs/coraline.log`

### "Project not initialized"

Initialize the project first:

```bash
cd /path/to/your/project
coraline init -i
```

Then restart the MCP client.

### Timeout errors

Increase the timeout in your MCP config:

```json
"args": ["serve", "--mcp", "--timeout", "300000"]
```

### Stale results

Sync the index:

```bash
coraline sync
```

Or force a full reindex:

```bash
coraline index --force
```

### Permission denied

Ensure Coraline has read access to your project files and write access to `.coraline/`.

## Advanced Configuration

### Custom Project Root

Specify a different project root than the working directory:

```json
{
  "mcpServers": {
    "coraline": {
      "command": "coraline",
      "args": ["serve", "--mcp", "--path", "/custom/project/path"]
    }
  }
}
```

### Monorepo Setup

For monorepos, point each server to a different package:

```json
{
  "mcpServers": {
    "coraline-frontend": {
      "command": "coraline",
      "args": ["serve", "--mcp", "--path", "/monorepo/packages/frontend"]
    },
    "coraline-backend": {
      "command": "coraline",
      "args": ["serve", "--mcp", "--path", "/monorepo/packages/backend"]
    }
  }
}
```

Each package needs its own `.coraline/` directory.

## Next Steps

- [MCP Tools Reference](./mcp-tools.md) - Detailed tool documentation
- [OpenCode Integration](./opencode.md) - Use with non-Claude models
- [Performance & Token Savings](./performance.md) - Optimize token usage
- [Configuration Guide](./configuration.md) - Customize behavior
