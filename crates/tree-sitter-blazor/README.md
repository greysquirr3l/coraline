# tree-sitter-blazor

Tree-sitter grammar for Blazor/Razor syntax (.razor and .cshtml files).

## Overview

This crate provides a tree-sitter grammar for parsing Blazor and Razor template syntax, enabling code analysis, syntax highlighting, and tooling support for ASP.NET Blazor applications.

## Features

- Parse `.razor` and `.cshtml` files
- Support for Blazor component syntax
- HTML and C# code mixing
- Directive parsing (`@page`, `@code`, `@inject`, etc.)
- Component parameters and events

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
tree-sitter-blazor = "0.1"
tree-sitter = "0.20"
```

### Example

```rust
use tree_sitter::{Parser, Language};

extern "C" { fn tree_sitter_blazor() -> Language; }

fn main() {
    let mut parser = Parser::new();
    let language = unsafe { tree_sitter_blazor() };
    parser.set_language(&language).expect("Error loading Blazor grammar");
    
    let source_code = r#"
        @page "/counter"
        <h1>Counter</h1>
        <button @onclick="IncrementCount">Click me</button>
    "#;
    
    let tree = parser.parse(source_code, None).unwrap();
    println!("{}", tree.root_node().to_sexp());
}
```

## Development

This grammar is part of the [Coraline](https://github.com/greysquirr3l/coraline) code graph indexer project.

### Building

```bash
cargo build --release
```

### Testing

```bash
cargo test
```

## Grammar Definition

The grammar is defined in `grammar.js` and follows the tree-sitter grammar format. See the
[tree-sitter documentation](https://tree-sitter.github.io/tree-sitter/creating-parsers)
for more information on creating and modifying grammars.

## License

MIT - See LICENSE file for details.

## Contributing

Contributions are welcome! Please open an issue or pull request on GitHub.
