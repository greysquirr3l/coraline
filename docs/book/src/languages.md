# Supported Languages

Coraline parses **36 languages** with tree-sitter and builds the same
semantic knowledge graph for every one of them: declarations, imports,
calls, instantiations, inheritance and cross-file references.

| Language | Extensions |
| --- | --- |
| TypeScript | `.ts` |
| TSX | `.tsx` |
| JavaScript | `.js` |
| JSX | `.jsx` |
| Python | `.py` |
| Go | `.go` |
| Rust | `.rs` |
| Java | `.java` |
| C | `.c`, `.h` |
| C++ | `.cpp`, `.cc`, `.cxx`, `.hpp` |
| C# | `.cs` |
| PHP | `.php` |
| Ruby | `.rb` |
| Swift | `.swift` |
| Kotlin | `.kt`, `.kts` |
| Markdown | `.md`, `.markdown` |
| TOML | `.toml` |
| YAML | `.yml`, `.yaml` |
| Zig | `.zig` |
| Scala | `.scala`, `.sc` |
| Haskell | `.hs` |
| Lua | `.lua` |
| Julia | `.jl` |
| MATLAB | `.m` |
| R | `.r` |
| Erlang | `.erl`, `.hrl` |
| Elixir | `.ex`, `.exs` |
| Groovy | `.groovy`, `.gradle` |
| Bash | `.sh`, `.bash` |
| PowerShell | `.ps1` |
| Nix | `.nix` |
| Dart | `.dart` |
| Fortran | `.f`, `.f90`, `.f95` |
| Elm | `.elm` |
| Perl | `.pl`, `.pm` |
| Blazor | `.razor`, `.cshtml` |

## Enabling a language

Not every language is indexed by default. The default include patterns
cover the common source file extensions; a language outside that set has
to be enabled in `.coraline/config.toml`:

```json
{
  "include": ["**/*.rs", "**/*.md"],
  "exclude": ["**/target/**"]
}
```

The full list of default patterns is in the
[Configuration guide](configuration.md).

## What is extracted

The graph is the same shape for every language:

- **Declarations** — functions, methods, classes, structs, interfaces,
  traits, enums, type aliases, fields, variables and constants.
- **Containment** — a `contains` edge from every declaration to the
  scope that holds it, so a method knows its class.
- **Calls** — a `calls` edge from the caller to the callee, resolved
  across files when an import or package membership backs the link.
- **Instantiation** — a constructor call produces an `instantiates` edge
  rather than a plain call.
- **Inheritance** — `extends` and `implements` edges.
- **Imports** — an `imports` edge per import statement, which is also
  what lets a call across a package boundary be resolved rather than
  guessed from the name.

The per-language mapping from tree-sitter node kinds to graph node kinds
lives in one table per language in
[`crates/coraline/src/grammar.rs`](https://github.com/greysquirr3l/coraline/blob/main/crates/coraline/src/grammar.rs),
and a guard test asserts that every kind the table names actually exists
in the grammar it claims to describe.

## Querying

The MCP server exposes the graph directly:

```json
{ "tool": "coraline_search", "arguments": { "name_pattern": "Widget" } }
```

`coraline_search` finds symbols by name, `coraline_callers` and
`coraline_callees` walk the call graph, `coraline_impact` reports what a
change breaks, and `coraline_context` assembles a task-shaped brief from
the graph. See the [MCP Tools reference](mcp-tools.md).
