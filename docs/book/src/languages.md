# Supported Languages

Coraline ships **36 languages** with tree-sitter grammars. **19** of
them are indexed end to end: declarations, imports, calls,
instantiations, inheritance and cross-file references are all resolved
into the same semantic knowledge graph. The other 17 have their grammar
dependency in place but no extraction contract yet, so a file in those
languages produces no nodes. The table marks which is which.

| Language | Extensions | Indexed |
| --- | --- | :---: |
| TypeScript | `.ts` | ✅ |
| TSX | `.tsx` | ✅ |
| JavaScript | `.js` | ✅ |
| JSX | `.jsx` | ✅ |
| Python | `.py` | ✅ |
| Go | `.go` | ✅ |
| Rust | `.rs` | ✅ |
| Java | `.java` | ✅ |
| C | `.c`, `.h` | ✅ |
| C++ | `.cpp`, `.cc`, `.cxx`, `.hpp` | ✅ |
| C# | `.cs` | ✅ |
| PHP | `.php` | ✅ |
| Ruby | `.rb` | ✅ |
| Swift | `.swift` | ✅ |
| Kotlin | `.kt`, `.kts` | ✅ |
| Liquid | `.liquid` | ✅ |
| Blazor | `.razor`, `.cshtml` | ✅ |
| Markdown | `.md`, `.markdown` | ✅ |
| Perl | `.pl`, `.pm` | ✅ |
| Bash | `.sh`, `.bash` | ⏳ |
| Dart | `.dart` | ⏳ |
| Elixir | `.ex`, `.exs` | ⏳ |
| Elm | `.elm` | ⏳ |
| Erlang | `.erl`, `.hrl` | ⏳ |
| Fortran | `.f`, `.f90`, `.f95` | ⏳ |
| Groovy | `.groovy`, `.gradle` | ⏳ |
| Haskell | `.hs` | ⏳ |
| Julia | `.jl` | ⏳ |
| Lua | `.lua` | ⏳ |
| MATLAB | `.m` | ⏳ |
| Nix | `.nix` | ⏳ |
| PowerShell | `.ps1` | ⏳ |
| R | `.r` | ⏳ |
| Scala | `.scala`, `.sc` | ⏳ |
| TOML | `.toml` | ⏳ |
| YAML | `.yml`, `.yaml` | ⏳ |
| Zig | `.zig` | ⏳ |

**✅** — the language is indexed and its graph is built.
**⏳** — the grammar dependency is in place, but the language has no
extraction contract yet, so it produces no nodes. See
[issue #128](https://github.com/greysquirr3l/coraline/issues/128).

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
