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
- **Multi-language Support** - 28 languages including TypeScript, Rust, Python, Go, C#, Java, C/C++, Ruby, Bash, Dart, Elixir, Haskell, Scala
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
coraline sync [path]              # Incremental update
coraline status [path]            # Show statistics
coraline query <search>           # Search symbols
coraline context <task>           # Build AI context
coraline serve --mcp              # Start MCP server
```

## 🔌 MCP Tools

When running as an MCP server, Coraline provides these tools:

### Graph Tools (CodeGraph-inspired)

| Tool | Description |
| ------ | ------------- |
| `codegraph_search` | Find symbols by name |
| `codegraph_context` | Build context for a task |
| `codegraph_callers` | Find what calls a function |
| `codegraph_callees` | Find what a function calls |
| `codegraph_impact` | Analyze change impact |
| `codegraph_node` | Get symbol details + code |

### Symbol Tools (Serena-inspired)

| Tool | Description |
| ------ | ------------- |
| `find_symbol` | Find symbols with pattern matching |
| `get_symbols_overview` | List all symbols in a file |
| `find_referencing_symbols` | Find all references to a symbol |
| `read_symbol` | Read a symbol's source code |

### Memory Tools

| Tool | Description |
| ------ | ------------- |
| `write_memory` | Store project knowledge |
| `read_memory` | Retrieve stored knowledge |
| `list_memories` | List all memories |
| `delete_memory` | Remove a memory |

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
| Lua | tree-sitter-lua | ✅ Full | Functions, tables, modules |
| MATLAB | tree-sitter-matlab | ✅ Full | Functions, scripts |
| Nix | tree-sitter-nix | ✅ Full | Derivations, functions |
| Perl | tree-sitter-perl | ✅ Full | Packages, subroutines |
| PowerShell | tree-sitter-powershell | ✅ Full | Functions, cmdlets, scripts |
| R | tree-sitter-r | ✅ Full | Functions, scripts |
| Scala | tree-sitter-scala | ✅ Full | Classes, objects, traits |
| YAML | tree-sitter-yaml | ✅ Full | Structure, mappings |
| Zig | tree-sitter-zig | ✅ Full | Functions, structs |

### 🔄 In Progress

| Language | Status | Notes |
| ---------- | -------- | ------- |
| PHP | ⏸️ Pending | Parser API compatibility issue |
| Swift | ⏸️ Pending | Parser API compatibility issue |
| Kotlin | ⏸️ Pending | Parser API compatibility issue |
| Markdown | ⏸️ Pending | Requires tree-sitter 0.19 (incompatible with 0.26) |
| TOML | ⏸️ Pending | Requires tree-sitter 0.20 (incompatible with 0.26) |

> **Note**: Language support requires tree-sitter grammar integration. Some parsers require older tree-sitter versions and will be added when updated parsers are available.

## ⚙️ Configuration

The `.coraline/config.json` file controls behavior:

```json
{
  "version": 1,
  "project_name": "my-project",
  "languages": ["typescript", "rust"],
  "exclude": [
    "target/**",
    "node_modules/**",
    "dist/**"
  ],
  "max_file_size": 1048576,
  "embedding_model": "nomic-embed-text-v1.5",
  "git_hooks_enabled": true
}
```

## 🧪 Development Status

Coraline is under active development. Current status:

### ✅ Phase 1: Foundation (Complete)

- [x] Tool abstraction layer
- [x] Memory system
- [x] Testing infrastructure (97% coverage)
- [x] CI/CD pipeline
- [x] Full-text search (FTS5)

### 🔄 Phase 2: Core Features (In Progress)

- [x] Vector embedding infrastructure
- [ ] ONNX model integration (awaiting ort 2.0 stable)
- [ ] Enhanced MCP tools
- [ ] Advanced configuration system

### 📋 Future Phases

- Language server integration (LSP)
- Additional language parsers
- Graph visualization tools
- VS Code extension

See [IMPROVEMENT_PLAN.md](docs/IMPROVEMENT_PLAN.md) for details.

## 📊 Testing

```bash
# Run all tests
cargo test --all-features

# Run with output
cargo test --all-features -- --nocapture

# Run specific test suite
cargo test --package coraline context_test
```

Current test coverage: **32/33 tests passing (97%)**

## 🤝 Contributing

Contributions welcome! Please see our development docs:
- [Development Guidelines](docs/dev/project_guidelines.md)
- [Rust Patterns](docs/dev/11_rust_patterns_you_will_use.md)
- [Testing Guide](docs/dev/testing_rust.md)

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
