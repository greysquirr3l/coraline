# Getting Started

## Install

Install from crates.io:

```bash
cargo install coraline
```

## Initialize and Index

```bash
coraline init
coraline index
```

## Optional: Semantic Search Model

```bash
coraline model download
coraline embed
```

By default, embedding runs a freshness check and auto-syncs stale indexed state before generating embeddings.

## Start MCP Server

```bash
coraline serve --mcp
```

## Next Steps

- [CLI Reference](cli-reference.md)
- [MCP Tools](mcp-tools.md)
- [Configuration](configuration.md)
