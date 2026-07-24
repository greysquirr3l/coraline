# OpenCode Integration

OpenCode is a terminal-based AI coding assistant that supports 75+ LLM providers including GPT-4, Gemini, and local models. Coraline's MCP server is fully compatible with OpenCode.

## Compatibility Status

**Status: ✅ COMPATIBLE**

Coraline works with OpenCode out-of-the-box because:
- Uses MCP protocol version `2024-11-05` (standard specification)
- Implements JSON-RPC 2.0 (required by MCP spec)
- Uses stdio transport (OpenCode's expected method)
- Tools properly namespaced (`coraline_` prefix)

### Supported Models

OpenCode works with:
- ✅ **GPT-4** (OpenAI)
- ✅ **Gemini** (Google)
- ✅ **Local models** (Ollama, LM Studio, etc.)
- ✅ **Other providers** (75+ total)

**Note:** Anthropic blocked Claude models in OpenCode in January 2026. Use Coraline with non-Claude models in OpenCode, or use [Claude Desktop](./mcp-integration.md#claude-desktop) / [Claude Code](./mcp-integration.md#claude-code) for Claude access.

## Installation

### Install OpenCode

```bash
npm install -g opencode
```

### Install Coraline

```bash
cargo install coraline
```

See [Installation](./installation.md) for detailed instructions.

## Configuration

Create `.opencode/config.json` in your **project workspace**:

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

### With Timeout Configuration

For projects with large indexes or complex queries, increase the timeout:

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

**Timeout values:**
- Default: `120000` (2 minutes)
- Recommended: `300000` (5 minutes) for large projects
- Maximum: `600000` (10 minutes)

### Absolute Path

If `coraline` is not in your PATH, use an absolute path:

```json
{
  "mcpServers": {
    "coraline": {
      "command": "/Users/you/.cargo/bin/coraline",
      "args": ["serve", "--mcp"]
    }
  }
}
```

Find your Coraline path:

```bash
which coraline
```

### Multiple Projects

Each project needs its own `.opencode/config.json`:

```bash
cd project-a
mkdir -p .opencode
cat > .opencode/config.json << 'EOF'
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

## Setup Workflow

### 1. Initialize Coraline

```bash
cd your-project
coraline init -i
```

### 2. Configure OpenCode

```bash
mkdir -p .opencode
cat > .opencode/config.json << 'EOF'
{
  "mcpServers": {
    "coraline": {
      "command": "coraline",
      "args": ["serve", "--mcp", "--timeout", "300000"]
    }
  }
}
EOF
```

### 3. Start OpenCode

```bash
opencode --model gpt-4
```

Or with Gemini:

```bash
opencode --model gemini-pro
```

Or with a local model:

```bash
opencode --model ollama/codellama
```

### 4. Verify Connection

In OpenCode, type:

```
List available MCP tools
```

You should see Coraline's 33 tools listed.

## Usage Examples

### Code Exploration

```
Search for authentication functions
Find all callers of the login() function
Show me the impact of changing the User class
```

### Context Building

```
Build context for adding a new payment gateway
Show me how the database layer works
What files handle user registration?
```

### Refactoring

```
Find all functions that call authenticateUser
Analyze the impact of renaming the Auth class
Show me the dependency graph for the API module
```

### File Operations

```
Read the src/auth/login.ts file
List all TypeScript files in the src directory
Show me all symbols in the User class file
```

## Timeout Configuration Details

OpenCode enforces timeouts on MCP tool calls. Coraline operations that may exceed the default timeout:

| Operation | Typical Duration | Recommended Timeout |
|---|---|---|
| `coraline_search` | <1s | Default (120s) OK |
| `coraline_callers` | 1-5s | Default OK |
| `coraline_impact` (large) | 5-30s | 300000ms (5 min) |
| `coraline_sync` (many files) | 10-60s | 300000ms (5 min) |
| `coraline_context` (deep) | 5-20s | 300000ms (5 min) |
| `coraline_embed` | 30-300s | 600000ms (10 min) |

### Timeout Error Handling

If you see timeout errors:

1. Increase timeout in `.opencode/config.json`:
   ```json
   "args": ["serve", "--mcp", "--timeout", "600000"]
   ```

2. Optimize your queries:
   - Use `--limit` parameters to reduce result size
   - Use `--max-depth` to limit traversal depth
   - Use compact output format

3. Check logs for actual operation time:
   ```bash
   tail -f .coraline/logs/coraline.log
   ```

## Model-Specific Tips

### GPT-4

GPT-4 has excellent tool use capabilities. It will automatically:
- Use batch tools for multiple queries
- Request compact output format
- Chain tool calls efficiently

```bash
opencode --model gpt-4 "Find all authentication functions and their callers"
```

### Gemini

Gemini works well with explicit instructions:

```bash
opencode --model gemini-pro "Use coraline_search to find the User class, then use coraline_callers to see what uses it"
```

### Local Models

Local models (Ollama, LM Studio) benefit from simpler prompts:

```bash
opencode --model ollama/codellama "Search for main function"
```

For complex tasks, break into steps:

```
1. Search for the authenticate function
2. Find what calls it
3. Show me the code
```

## Performance Optimization

### Use Compact Output

Instruct the model to use compact format:

```
Search for User class using compact output format
```

This reduces tokens by ~65%.

### Use Batch Tools

For multiple queries:

```
Use batch tools to fetch details for these 5 functions: login, logout, authenticate, authorize, validate
```

Saves 60-90% tokens vs individual queries.

### Limit Results

Specify limits in your prompts:

```
Find the top 5 most-called functions in the auth module
```

### Incremental Sync

Keep the index up-to-date with git hooks:

```bash
coraline hooks install
```

This ensures fast sync operations instead of full reindexing.

## Troubleshooting

### "MCP server not found"

1. Verify `.opencode/config.json` exists in your project root
2. Check that `coraline` is in your PATH or use absolute path
3. Restart OpenCode

### "Project not initialized"

Initialize Coraline first:

```bash
coraline init -i
```

### Timeout errors

Increase timeout in config:

```json
"args": ["serve", "--mcp", "--timeout", "600000"]
```

### Tools not working

1. Check Coraline version:
   ```bash
   coraline --version
   ```

2. Check logs:
   ```bash
   tail -f .coraline/logs/coraline.log
   ```

3. Test Coraline directly:
   ```bash
   coraline stats
   ```

### Model doesn't understand tools

Some models need explicit guidance:

```
Use the coraline_search tool to find functions named "authenticate"
```

Instead of:

```
Find authenticate functions
```

## Advanced Configuration

### Custom Log Level

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

### Specific Project Path

Override the working directory:

```json
{
  "mcpServers": {
    "coraline": {
      "command": "coraline",
      "args": ["serve", "--mcp", "--path", "/absolute/path/to/project"]
    }
  }
}
```

### Multiple Servers

Work with multiple projects simultaneously:

```json
{
  "mcpServers": {
    "coraline-frontend": {
      "command": "coraline",
      "args": ["serve", "--mcp", "--path", "/projects/app/frontend"]
    },
    "coraline-backend": {
      "command": "coraline",
      "args": ["serve", "--mcp", "--path", "/projects/app/backend"]
    }
  }
}
```

Specify which server to use in prompts:

```
Use coraline-frontend to search for React components
Use coraline-backend to find API routes
```

## Known Limitations

1. **No Claude models** - Anthropic restriction (as of Jan 2026)
2. **Stdio only** - WebSocket transport not supported
3. **Single project per server** - Multi-repo workspaces not yet implemented
4. **No streaming responses** - All results returned at once

## Comparison with Claude Desktop/Code

| Feature | OpenCode | Claude Desktop | Claude Code |
|---|---|---|---|
| Model choice | 75+ providers | Claude only | Claude only |
| Terminal-based | ✅ | ❌ | ✅ |
| GUI | ❌ | ✅ | ❌ |
| Coraline support | ✅ | ✅ | ✅ |
| Timeout config | ✅ (required) | Optional | Optional |
| Local models | ✅ | ❌ | ❌ |

## Testing Your Setup

### 1. Verify Installation

```bash
opencode --version
coraline --version
```

### 2. Test MCP Connection

```bash
cd your-project
opencode --model gpt-4 "List MCP tools"
```

Should show Coraline tools.

### 3. Test a Tool

```bash
opencode --model gpt-4 "Search for main function using coraline_search"
```

### 4. Test Context Building

```bash
opencode --model gpt-4 "Build context for understanding the authentication system"
```

### 5. Check Logs

```bash
cat .coraline/logs/coraline.log
```

Should show MCP requests and responses.

## Next Steps

- [MCP Tools Reference](./mcp-tools.md) - All available tools
- [Performance & Token Savings](./performance.md) - Optimize usage
- [Configuration Guide](./configuration.md) - Customize Coraline
- [MCP Integration](./mcp-integration.md) - Claude Desktop/Code setup
