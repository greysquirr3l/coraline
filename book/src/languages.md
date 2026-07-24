# Supported Languages

Coraline uses [tree-sitter](https://tree-sitter.github.io/) for parsing, enabling support for 33 programming languages with AST-level precision.

## Language Support Table

| Language | Status | tree-sitter grammar | Framework Support |
|---|---|---|---|
| **Bash** | ✅ Full | `tree-sitter-bash` 0.25.1 | - |
| **C** | ✅ Full | `tree-sitter-c` 0.23 | - |
| **C++** | ✅ Full | `tree-sitter-cpp` 0.23 | - |
| **C#** | ✅ Full | `tree-sitter-c-sharp` 0.23 | .NET |
| **Blazor** | ✅ Full | `tree-sitter-blazor` 0.1.1 (custom) | Blazor components |
| **Dart** | ✅ Full | `tree-sitter-dart` 0.0.4 | Flutter |
| **Elixir** | ✅ Full | `tree-sitter-elixir` 0.3.4 | Phoenix |
| **Elm** | ✅ Full | `tree-sitter-elm` 5.9.0 | - |
| **Erlang** | ✅ Full | `tree-sitter-erlang` 0.15.0 | OTP |
| **Fortran** | ✅ Full | `tree-sitter-fortran` 0.5.1 | - |
| **Go** | ✅ Full | `tree-sitter-go` 0.23 | - |
| **Groovy** | ✅ Full | `tree-sitter-groovy` 0.1.2 | Gradle |
| **Haskell** | ✅ Full | `tree-sitter-haskell` 0.23.1 | - |
| **Java** | ✅ Full | `tree-sitter-java` 0.23 | Spring |
| **JavaScript** | ✅ Full | `tree-sitter-javascript` 0.25.0 | Node.js |
| **JSX** | ✅ Full | `tree-sitter-javascript` 0.25.0 | React |
| **Julia** | ✅ Full | `tree-sitter-julia` 0.23.1 | - |
| **Kotlin** | ✅ Full | `tree-sitter-kotlin-ng` 1.1.0 | Android |
| **Lua** | ✅ Full | `tree-sitter-lua` 0.4.1 | - |
| **Markdown** | ✅ Full | `tree-sitter-markdown-fork` 0.7.3 | - |
| **MATLAB** | ✅ Full | `tree-sitter-matlab` 1.3.0 | - |
| **Nix** | ✅ Full | `tree-sitter-nix` 0.3.0 | - |
| **Perl** | ✅ Full | `tree-sitter-perl` 1.1.2 | - |
| **PHP** | ✅ Full | `tree-sitter-php` 0.24.2 | Laravel |
| **PowerShell** | ✅ Full | `tree-sitter-powershell` 0.25.10 | - |
| **Python** | ✅ Full | `tree-sitter-python` 0.23 | Django, Flask |
| **R** | ✅ Full | `tree-sitter-r` 1.2.0 | - |
| **Ruby** | ✅ Full | `tree-sitter-ruby` 0.23 | Rails |
| **Rust** | ✅ Full | `tree-sitter-rust` 0.24.0 | - |
| **Scala** | ✅ Full | `tree-sitter-scala` 0.24.0 | - |
| **Swift** | ✅ Full | `tree-sitter-swift` 0.7.1 | iOS/macOS |
| **TOML** | ✅ Full | `tree-sitter-toml-ng` 0.7.0 | Config files |
| **TypeScript** | ✅ Full | `tree-sitter-typescript` 0.23.1 | Node.js |
| **TSX** | ✅ Full | `tree-sitter-typescript` 0.23.1 | React |
| **YAML** | ✅ Full | `tree-sitter-yaml` 0.7.2 | Config files |
| **Zig** | ✅ Full | `tree-sitter-zig` 1.1.2 | - |

## Language Detection

Coraline detects languages by file extension:

| Extension(s) | Language |
|---|---|
| `.rs` | Rust |
| `.ts` | TypeScript |
| `.tsx` | TSX (TypeScript + JSX) |
| `.js` | JavaScript |
| `.jsx` | JSX (JavaScript + JSX) |
| `.py` | Python |
| `.go` | Go |
| `.java` | Java |
| `.c`, `.h` | C |
| `.cpp`, `.cc`, `.cxx`, `.hpp` | C++ |
| `.cs` | C# |
| `.php` | PHP |
| `.rb` | Ruby |
| `.swift` | Swift |
| `.kt`, `.kts` | Kotlin |
| `.sh`, `.bash` | Bash |
| `.ps1`, `.psm1` | PowerShell |
| `.razor` | Blazor |
| `.dart` | Dart |
| `.ex`, `.exs` | Elixir |
| `.elm` | Elm |
| `.erl`, `.hrl` | Erlang |
| `.f90`, `.f95`, `.f03` | Fortran |
| `.groovy`, `.gradle` | Groovy |
| `.hs` | Haskell |
| `.jl` | Julia |
| `.lua` | Lua |
| `.md`, `.markdown` | Markdown |
| `.m` | MATLAB |
| `.nix` | Nix |
| `.pl`, `.pm` | Perl |
| `.r`, `.R` | R |
| `.scala`, `.sc` | Scala |
| `.toml` | TOML |
| `.yaml`, `.yml` | YAML |
| `.zig` | Zig |

## Node Types by Language

Different languages produce different node kinds:

### Object-Oriented (Java, C#, Swift, Kotlin)
- `class`
- `interface`
- `method`
- `property`
- `field`
- `enum`
- `enum_member`

### Functional (Haskell, Scala, Elixir, Erlang)
- `function`
- `module`
- `trait` (Scala)
- `type_alias`
- `constant`

### Systems (Rust, C, C++, Zig)
- `struct`
- `function`
- `trait` (Rust)
- `enum`
- `type_alias`
- `module` (Rust)

### Scripting (Python, Ruby, PHP, Bash)
- `function`
- `class`
- `method`
- `variable`
- `constant`

### Web Frontend (JavaScript, TypeScript, TSX, JSX)
- `function`
- `class`
- `method`
- `component` (React/JSX/TSX)
- `interface` (TypeScript)
- `type_alias` (TypeScript)

### Markup (Markdown, YAML, TOML)
- `module` (file-level)
- `constant` (YAML/TOML keys)

## Framework-Specific Features

### Rust
- Crate-relative imports (`crate::`, `super::`, `self::`)
- Trait implementation tracking
- Macro detection
- Module tree resolution

### React (JavaScript/TypeScript/JSX/TSX)
- Component detection (PascalCase)
- Hook tracking (`use*` functions)
- Relative imports (`./`, `../`)
- Path aliases (`@/`)

### Blazor (C# + Razor)
- Razor component detection (`.razor` files)
- Component parameter tracking
- .NET type resolution
- Dependency injection detection

### Laravel (PHP)
- PSR-4 autoloading resolution
- Blade template tracking
- Facade detection
- Dot-notation view paths

### Python
- Module imports (`from ... import`)
- Package structure
- Class/function decorators
- Django/Flask detection (future)

### Others
- **Swift/Kotlin**: Protocol/interface conformance
- **Elixir**: Phoenix routing, Ecto schemas
- **Java**: Spring annotations, package resolution

## Cross-Language Support

Coraline can index **polyglot projects** with multiple languages:

```
project/
├── backend/           (Rust)
├── frontend/          (TypeScript + React)
├── mobile/            (Swift, Kotlin)
└── scripts/           (Python, Bash)
```

All languages are indexed into a unified graph. Cross-language references (e.g., TypeScript calling Rust WASM) are tracked as `unresolved` edges unless framework resolvers are available.

## Language-Specific Configuration

Customize indexing per language via `config.toml`:

```toml
[indexing]
include_patterns = [
  "src/**/*.rs",           # Rust source
  "lib/**/*.ts",           # TypeScript libraries
  "app/**/*.tsx",          # React components
  "**/*.py",               # Python anywhere
]
exclude_patterns = [
  "**/target/**",          # Rust builds
  "**/node_modules/**",    # JS/TS deps
  "**/__pycache__/**",     # Python cache
  "**/dist/**",            # Build outputs
]
```

## Adding New Languages

Coraline can be extended with new tree-sitter grammars:

1. Add the grammar to `Cargo.toml`:
   ```toml
   tree-sitter-newlang = "0.1.0"
   ```

2. Register in `extraction.rs`:
   ```rust
   ".newlang" => tree_sitter_newlang::language(),
   ```

3. Add extraction patterns for node/edge types

4. (Optional) Add framework resolver in `resolution/frameworks/`

See [Development Guide](./development.md) for contribution instructions.

## Language Limitations

### Markdown
- Only headings and code blocks are indexed
- No inline link tracking

### YAML/TOML
- Config keys indexed as `constant` nodes
- No deep structure analysis

### Bash/PowerShell
- Function detection only
- Limited variable tracking

### Future Enhancements
- SQL query extraction (embedded SQL strings)
- HTML/CSS parsing (currently ignored)
- GraphQL schema tracking
- Protocol Buffers support

## Performance by Language

Parse speed varies by grammar complexity:

| Language Group | Relative Speed | Notes |
|---|---|---|
| Rust, Go, C | Fast | Simple, deterministic grammars |
| TypeScript, Python | Medium | More complex syntax rules |
| C++, Scala | Slower | High grammar complexity |
| Markdown, YAML | Very Fast | Simple structure |

Actual impact on indexing is minimal (<5% variation) for most projects.

## Testing Language Support

Verify language support for your project:

```bash
# Index and check stats
coraline init -i
coraline stats --json

# Check language breakdown
jq '.files_by_language' .coraline/stats.json
```

Output:
```json
{
  "rust": 42,
  "typescript": 31,
  "python": 12,
  "markdown": 5
}
```

Search for language-specific symbols:

```bash
# Rust
coraline query "impl" --kind trait

# TypeScript
coraline query "interface" --kind interface

# Python
coraline query "def" --kind function
```

## Requesting Language Support

To request a new language:

1. Check if a tree-sitter grammar exists at [tree-sitter.github.io](https://tree-sitter.github.io/)
2. Open an issue at [github.com/greysquirr3l/coraline/issues](https://github.com/greysquirr3l/coraline/issues)
3. Include:
   - Language name and typical file extensions
   - Link to tree-sitter grammar
   - Example files for testing
   - (Optional) Framework-specific features needed

## Next Steps

- [Quick Start Guide](./quick-start.md) - Start indexing your project
- [Configuration Guide](./configuration.md) - Customize language filtering
- [Architecture](./architecture.md) - How language parsing works
- [Development Guide](./development.md) - Contribute new languages
