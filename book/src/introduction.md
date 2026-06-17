# Introduction

Coraline is a Rust-based code intelligence system that builds semantic knowledge graphs for AI assistants. It provides fast, local code analysis with zero external dependencies.

## What is Coraline?

Coraline combines ideas from [CodeGraph](https://github.com/colbymchenry/codegraph) and [Serena](https://github.com/oraios/serena) to provide:

- **Fast indexing** - Native Rust performance for rapid code analysis
- **Semantic search** - Find code by meaning using vector embeddings
- **Symbol-level tools** - Precise function/class/method operations
- **100% local** - All processing happens on your machine
- **MCP integration** - Works with Claude Desktop, Claude Code, and OpenCode

## Key Features

### Code Intelligence
- AST-based parsing using tree-sitter (33 languages supported)
- Cross-file reference resolution
- Impact analysis for understanding change ripple effects
- Symbol search with FTS5 full-text indexing

### AI Assistant Integration
- Model Context Protocol (MCP) server with 33 tools
- Compact output format for 65% token reduction
- Batch query tools for 60-90% token savings
- Natural language context building

### Developer Experience
- CLI for direct command-line usage
- Git hooks for automatic incremental updates
- Project memories for persistent context
- Configurable indexing and exclusion patterns

## How It Works

1. **Index** - Coraline scans your project and builds a knowledge graph of all symbols (functions, classes, types, etc.) and their relationships
2. **Query** - Use the CLI or MCP tools to search, traverse, and analyze your codebase
3. **Assist** - AI assistants use Coraline's tools to understand your code structure and answer questions

All data is stored locally in a SQLite database (`.coraline/coraline.db`) with no external API calls or cloud dependencies.

## Use Cases

- **Code exploration** - Understand unfamiliar codebases quickly
- **Impact analysis** - See what will be affected by a change
- **Refactoring** - Find all callers/callees of a function
- **Documentation** - Build context for AI-assisted documentation
- **Code review** - Analyze dependencies and relationships
- **Migration planning** - Map out cross-file dependencies

## Next Steps

- [Install Coraline](./installation.md) to get started
- Follow the [Quick Start Guide](./quick-start.md) for your first project
- Explore the [MCP Tools Reference](./mcp-tools.md) to see what's available
- Check out [MCP Integration](./mcp-integration.md) for AI assistant setup
