<div align="center">

# 🧵 Coraline

### Fast, Local Code Intelligence for AI Assistants

**Semantic code indexing • Symbol-level editing • 100% Rust • MCP Server**

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)
[![Rust](https://img.shields.io/badge/Rust-1.93+-orange.svg)](https://www.rust-lang.org/)

</div>

---

## 🚀 What is Coraline?

**Coraline** is a Rust implementation that combines the best ideas from two powerful coding tools:

- **[CodeGraph](https://github.com/colbymchenry/codegraph)** - Semantic code knowledge graphs for efficient AI exploration
- **[Serena](https://github.com/oraios/serena)** - Symbol-level code understanding and editing tools

Built from the ground up in Rust, Coraline provides:
- ⚡ **Native performance** - Fast indexing and queries without Node.js overhead
- 🧠 **Semantic search** - Find code by meaning using vector embeddings
- 🔧 **Symbol-level tools** - IDE-like precision for AI assistants
- 🔒 **100% local** - All processing happens on your machine
- 🌐 **MCP integration** - Works with Claude Desktop, Claude Code, and other MCP clients

## ✨ Key Features

### From CodeGraph

- **Semantic Knowledge Graph** - Pre-indexed symbol relationships and call graphs
- **Multi-language Support** - 33 languages including TypeScript, Rust, Python, Go, C#, Java, C/C++, Ruby, Bash, PHP, Swift, Kotlin, Markdown, TOML
- **Vector Embeddings** - Semantic code search using local ONNX models
- **Impact Analysis** - Understand what code changes will affect
- **Git Integration** - Auto-sync on commits to keep index fresh

### From Serena

- **Symbol-level Operations** - Find, read, and edit code at the function/class/method level
- **Reference Resolution** - Find all references to a symbol across the codebase
- **Precise Editing** - Insert before/after symbols, replace symbol bodies
- **Project Memories** - Persistent knowledge storage for project context
- **Smart Context Building** - Gather relevant code for AI assistants efficiently

## 📦 Installation

### Build from Source

```bash
git clone https://github.com/greysquirr3l/coraline.git
cd coraline
cargo build --release
```

The binary will be at `target/release/coraline`.

## 🚀 Quick Start

### 1. Initialize a Project

```bash
cd your-project
coraline init
```

This creates a `.coraline/` directory with:
- SQLite database for the code graph
- Configuration file
- Project memories

### 2. Index Your Code

```bash
coraline index
```

Coraline will:
- Parse source files using tree-sitter
- Extract symbols (functions, classes, methods, types)
- Build the call graph and reference map
- Generate vector embeddings for semantic search

### 3. Use as MCP Server

Configure your MCP client to use Coraline:

**For Claude Desktop (`~/Library/Application Support/Claude/claude_desktop_config.json`):**

```json
{
  "mcpServers": {
    "coraline": {
      "command": "/path/to/coraline",
      "args": ["serve", "--mcp"],
      "env": {}
    }
  }
}
```

**For Claude Code (`.claude/mcp.json` in your workspace):**

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

## 💻 CLI Usage

```bash
coraline init [path]              # Initialize project
coraline index [path]             # Build code graph
coraline sync [path]              # Incremental update (git-diff based)
coraline status [path]            # Show project status and paths
coraline stats [path]             # Show index statistics
coraline query <search>           # Search symbols
coraline context <task>           # Build AI context
coraline callers <node-id>        # Find what calls a symbol
coraline callees <node-id>        # Find what a symbol calls
coraline impact <node-id>         # Analyze change impact
coraline config [--set key=val]   # Read or update configuration
coraline hooks install|remove     # Manage git post-commit hook
coraline serve --mcp              # Start MCP server
```

See [docs/CLI_REFERENCE.md](docs/CLI_REFERENCE.md) for full documentation.

## 🔌 MCP Tools

When running as an MCP server, Coraline exposes **25 tools** (26 with vector embeddings) prefixed with `coraline_`.
See [docs/MCP_TOOLS.md](docs/MCP_TOOLS.md) for the full reference.

### Graph Tools

| Tool | Description |
| ------ | ------------- |
| `coraline_search` | Find symbols by name or pattern |
| `coraline_callers` | Find what calls a symbol |
| `coraline_callees` | Find what a symbol calls |
| `coraline_impact` | Analyze change impact radius |
| `coraline_dependencies` | Outgoing dependency graph from a node |
| `coraline_dependents` | Incoming dependency graph — what depends on a node |
| `coraline_path` | Find a path between two nodes |
| `coraline_stats` | Detailed statistics by language, kind, and edge type |
| `coraline_find_symbol` | Find symbols with rich metadata + optional body |
| `coraline_get_symbols_overview` | List all symbols in a file |
| `coraline_find_references` | Find all references to a symbol |
| `coraline_node` | Get full node details and source code |

### Context Tool

| Tool | Description |
| ------ | ------------- |
| `coraline_context` | Build structured context for an AI task |

### File & Config Tools

| Tool | Description |
| ------ | ------------- |
| `coraline_read_file` | Read file contents |
| `coraline_list_dir` | List directory contents |
| `coraline_get_file_nodes` | Get all indexed nodes in a file |
| `coraline_status` | Show project index statistics |
| `coraline_sync` | Trigger incremental index sync |
| `coraline_get_config` | Read project configuration |
| `coraline_update_config` | Update a config value |
| `coraline_semantic_search` | Vector similarity search (requires embeddings) |

### Memory Tools

| Tool | Description |
| ------ | ------------- |
| `coraline_write_memory` | Write or update a project memory |
| `coraline_read_memory` | Retrieve a stored memory |
| `coraline_list_memories` | List all memories |
| `coraline_delete_memory` | Remove a memory |
| `coraline_edit_memory` | Edit memory via literal or regex replace |

## 🏗️ Architecture

```asciidoc
┌─────────────────────────────────────────────────────────────────┐
│                        AI Assistant (MCP Client)                 │
│                    (Claude, VS Code, etc.)                       │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                     Coraline MCP Server                          │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐          │
│  │ Graph Tools  │  │ Symbol Tools │  │ Memory Tools │          │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘          │
│         └──────────────────┼──────────────────┘                 │
│                            ▼                                     │
│              ┌─────────────────────────┐                         │
│              │   Core Engine (Rust)    │                         │
│              │   • tree-sitter parser  │                         │
│              │   • SQLite database     │                         │
│              │   • Vector embeddings   │                         │
│              │   • Reference resolver  │                         │
│              └─────────────────────────┘                         │
└─────────────────────────────────────────────────────────────────┘
```

### Data Flow

1. **Parsing**: tree-sitter extracts AST and symbols
2. **Storage**: Nodes and edges stored in SQLite
3. **Resolution**: References resolved to definitions
4. **Embeddings**: Symbols embedded for semantic search (ONNX)
5. **Query**: Graph traversal + vector similarity
6. **Serve**: Results returned via MCP protocol

## 🌐 Supported Languages

Coraline uses tree-sitter for fast, accurate code parsing. Current support:

### ✅ Fully Implemented

| Language | Parser | Status | Notes |
| ---------- | -------- | -------- | ------- |
| TypeScript | tree-sitter-typescript | ✅ Full | Functions, classes, methods, interfaces |
| JavaScript | tree-sitter-javascript | ✅ Full | ES6+, JSX support |
| Rust | tree-sitter-rust | ✅ Full | Full symbol extraction |
| Python | tree-sitter-python | ✅ Full | Classes, functions, methods |
| Go | tree-sitter-go | ✅ Full | Packages, functions, structs |
| Java | tree-sitter-java | ✅ Full | Classes, methods, interfaces |
| C | tree-sitter-c | ✅ Full | Functions, structs, typedefs |
| C++ | tree-sitter-cpp | ✅ Full | Classes, templates, namespaces |
| C# (.NET) | tree-sitter-c-sharp | ✅ Full | ASP.NET Core, Blazor, .razor files |
| Ruby | tree-sitter-ruby | ✅ Full | Classes, modules, methods |
| Bash | tree-sitter-bash | ✅ Full | Shell scripts, functions |
| Dart | tree-sitter-dart | ✅ Full | Classes, functions, widgets |
| Elixir | tree-sitter-elixir | ✅ Full | Modules, functions, macros |
| Elm | tree-sitter-elm | ✅ Full | Functions, types, modules |
| Erlang | tree-sitter-erlang | ✅ Full | Modules, functions |
| Fortran | tree-sitter-fortran | ✅ Full | Subroutines, functions, modules |
| Groovy | tree-sitter-groovy | ✅ Full | Classes, methods, closures |
| Haskell | tree-sitter-haskell | ✅ Full | Functions, types, typeclasses |
| Julia | tree-sitter-julia | ✅ Full | Functions, types, modules |
| Kotlin | tree-sitter-kotlin-ng | ✅ Full | Classes, functions, objects |
| Lua | tree-sitter-lua | ✅ Full | Functions, tables, modules |
| Markdown | tree-sitter-markdown-fork | ✅ Full | Documents, headings, lists |
| MATLAB | tree-sitter-matlab | ✅ Full | Functions, scripts |
| Nix | tree-sitter-nix | ✅ Full | Derivations, functions |
| Perl | tree-sitter-perl | ✅ Full | Packages, subroutines |
| PHP | tree-sitter-php | ✅ Full | Classes, functions, methods |
| PowerShell | tree-sitter-powershell | ✅ Full | Functions, cmdlets, scripts |
| R | tree-sitter-r | ✅ Full | Functions, scripts |
| Scala | tree-sitter-scala | ✅ Full | Classes, objects, traits |
| Swift | tree-sitter-swift | ✅ Full | Classes, structs, functions |
| TOML | tree-sitter-toml-ng | ✅ Full | Configuration, tables, keys |
| YAML | tree-sitter-yaml | ✅ Full | Structure, mappings |
| Zig | tree-sitter-zig | ✅ Full | Functions, structs |

### 🔄 In Progress

| Language | Status | Notes |
| ---------- | -------- | ------- |
| Liquid | ⏸️ Pending | Parser API compatibility issue |

> **Note**: Language support requires tree-sitter grammar integration. Some parsers require older tree-sitter versions and will be added when updated parsers are available.

## ⚙️ Configuration

Configuration lives in `.coraline/config.toml`. A commented template is created by `coraline init`.

```toml
[indexing]
max_file_size = 1048576   # 1 MB
batch_size    = 100

[context]
max_nodes          = 20
max_code_blocks    = 5
max_code_block_size = 1500
traversal_depth    = 1

[sync]
git_hooks_enabled = true

[vectors]
enabled = false   # ONNX embedding support (pending stable ort 2.0)
```

See [docs/CONFIGURATION.md](docs/CONFIGURATION.md) for the full reference.

## 📊 Testing

```bash
# Run all tests
cargo test --all-features

# Run with output
cargo test --all-features -- --nocapture

# Run specific integration test file
cargo test --test context_test
```

Current status: **37/37 tests passing**  
See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for the full test structure.

## 📚 Documentation

| Document | Description |
|---|---|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | System design and data model |
| [docs/MCP_TOOLS.md](docs/MCP_TOOLS.md) | Complete MCP tools reference |
| [docs/CLI_REFERENCE.md](docs/CLI_REFERENCE.md) | All CLI commands and flags |
| [docs/CONFIGURATION.md](docs/CONFIGURATION.md) | Configuration guide |
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) | Build, test, and contribute |

## 🤝 Contributing

Contributions welcome! See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for build setup, coding style, and how to add new tools and language parsers.

## 📄 License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

## 🙏 Acknowledgements

Coraline is inspired by and built upon ideas from:

- **[CodeGraph](https://github.com/colbymchenry/codegraph)** by Colby McHenry - Semantic code graphs for AI
- **[Serena](https://github.com/oraios/serena)** by Oraios AI - Symbol-level code intelligence
- **[tree-sitter](https://tree-sitter.github.io/)** - Fast, incremental parsing library

## 🔗 References

- CodeGraph: <https://github.com/colbymchenry/codegraph>
- Serena: <https://github.com/oraios/serena>
- MCP: <https://modelcontextprotocol.io/>
- tree-sitter: <https://tree-sitter.github.io/>

---

<div align="center">

**Built with 🦀 Rust for the AI coding community**

[Report Bug](https://github.com/greysquirr3l/coraline/issues) · [Request Feature](https://github.com/greysquirr3l/coraline/issues)

</div>
