# Installation & Setup

This guide covers all installation methods for Coraline.

## From crates.io

The recommended installation method is via Cargo:

```bash
cargo install coraline
```

This downloads and compiles the latest stable release from [crates.io](https://crates.io/crates/coraline).

**Requirements:**
- Rust 1.93 or later
- ~5 minutes compilation time on a modern machine

After installation, verify with:

```bash
coraline --version
```

## Pre-built Binaries

Download pre-compiled binaries from the [GitHub releases page](https://github.com/greysquirr3l/coraline/releases).

Available platforms:
- **macOS** (x86_64 and ARM64)
- **Linux** (x86_64 and ARM64)
- **Windows** (x86_64)

### Installation Steps

1. Download the appropriate binary for your platform
2. Extract the archive
3. Move the binary to a directory in your PATH:

```bash
# macOS/Linux example
chmod +x coraline
sudo mv coraline /usr/local/bin/
```

4. Verify installation:

```bash
coraline --version
```

## Building from Source

For development or custom builds:

```bash
# Clone the repository
git clone https://github.com/greysquirr3l/coraline.git
cd coraline

# Build with all features
cargo build --release --all-features

# Install from local source
cargo install --path crates/coraline --force
```

The binary will be installed to `~/.cargo/bin/coraline`.

### Development Build

For active development:

```bash
cargo build --all-features
```

The debug binary will be at `target/debug/coraline`.

## Semantic Search Setup

Semantic search is included in the default build but requires downloading an ONNX embedding model.

### Download the Model

```bash
coraline model download
```

This downloads `nomic-embed-text-v1.5` (~137 MB) to `.coraline/models/`.

**Available model variants:**

| Variant | Size | Notes |
|---|---|---|
| `model_q4f16.onnx` | ~111 MB | Q4 + fp16 mixed (smallest) |
| `model_int8.onnx` | ~137 MB | int8 quantized (recommended) |
| `model_fp16.onnx` | ~274 MB | fp16 |
| `model.onnx` | ~547 MB | full f32 |

To download a specific variant:

```bash
coraline model download --variant model_fp16.onnx
```

### Generate Embeddings

After downloading the model, generate embeddings for your indexed project:

```bash
cd your-project
coraline init -i           # Initialize and index
coraline embed             # Generate embeddings
```

Or combine download and embed:

```bash
coraline embed --download
```

### Verify Semantic Search

Check that the model is available:

```bash
coraline model status
```

Once embeddings are generated, the `coraline_semantic_search` MCP tool will be available.

## First Project Setup

After installation, initialize your first project:

```bash
cd your-project
coraline init -i
```

This creates `.coraline/` with:
- `coraline.db` - SQLite knowledge graph
- `config.toml` - Configuration file
- `memories/` - Project memory directory
- `logs/` - Log file directory

During initialization, you'll be prompted to download the embedding model. You can decline and download it later with `coraline model download`.

## Updating Coraline

### From crates.io

```bash
cargo install coraline --force
```

### From pre-built binaries

Download the latest release and replace your existing binary.

### From source

```bash
cd coraline
git pull
cargo install --path crates/coraline --force
```

## Uninstallation

### Cargo installation

```bash
cargo uninstall coraline
```

### Binary installation

Remove the binary from your PATH:

```bash
sudo rm /usr/local/bin/coraline
```

### Project data

To remove Coraline data from a project:

```bash
cd your-project
rm -rf .coraline
```

## Troubleshooting

### "command not found: coraline"

Ensure `~/.cargo/bin` (or your installation directory) is in your PATH:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

Add this to your shell configuration file (`.bashrc`, `.zshrc`, etc.) to make it permanent.

### Compilation errors

Ensure you have the latest Rust toolchain:

```bash
rustup update
```

### Model download fails

If the HuggingFace download fails, you can manually download the model files:

1. Visit [nomic-ai/nomic-embed-text-v1.5](https://huggingface.co/nomic-ai/nomic-embed-text-v1.5)
2. Download `tokenizer.json`, `tokenizer_config.json`, and your chosen ONNX variant
3. Place them in `.coraline/models/nomic-embed-text-v1.5/`

### Permission denied on git hooks

If `coraline init` fails to install git hooks:

```bash
coraline hooks install
```

Or manually install:

```bash
chmod +x .git/hooks/post-commit
```

## Next Steps

- [Quick Start Guide](./quick-start.md) - Index your first project
- [MCP Integration](./mcp-integration.md) - Connect to AI assistants
- [Configuration Guide](./configuration.md) - Customize Coraline's behavior
