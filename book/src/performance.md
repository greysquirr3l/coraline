# Performance & Token Savings

Coraline provides several features to optimize performance and reduce AI token consumption.

## Token Savings Overview

Coraline's design focuses on reducing token usage in AI conversations:

| Feature | Token Savings | Use Case |
|---|---|---|
| Compact output format | 65% | All tool responses |
| Batch query tools | 60-90% | Multiple lookups |
| Edge filtering | 15% | Targeted traversals |
| Precomputed metrics | 20% latency | Impact analysis |
| Smart result limits | Variable | Large result sets |

### Real-World Example

**Without optimization:**
```
20 individual coraline_node calls
= 20 requests × 50 tokens/request
= 1000 tokens
```

**With batch tools:**
```
1 coraline_batch_get_nodes call
= 1 request × 100 tokens
= 100 tokens (90% reduction)
```

## Compact Output Format

All MCP tools support `output_format: "compact"` parameter for reduced token usage.

### Standard vs Compact

**Standard output (350 chars):**
```json
{
  "node": {
    "id": "abc123def456",
    "kind": "Function",
    "name": "authenticate_user",
    "qualified_name": "auth::authenticate_user",
    "file_path": "/project/src/auth/login.rs",
    "start_line": 42,
    "end_line": 95,
    "language": "Rust",
    "signature": "fn authenticate_user(username: &str, password: &str) -> Result<User>"
  }
}
```

**Compact output (120 chars, 65% reduction):**
```json
{
  "n": {
    "i": "abc123",
    "k": 7,
    "n": "authenticate_user",
    "f": "src/auth/login.rs",
    "l": 42,
    "s": "fn authenticate_user(...)"
  }
}
```

### Usage

Instruct the AI assistant to use compact format:

```
Search for User class using compact output format
Find callers of login() with compact output
```

Or explicitly in tool parameters:

```json
{
  "query": "User",
  "output_format": "compact"
}
```

### Field Mappings

| Standard | Compact | Type |
|---|---|---|
| `id` | `i` | string |
| `kind` | `k` | int (enum) |
| `name` | `n` | string |
| `qualified_name` | `q` | string |
| `file_path` | `f` | string |
| `start_line` | `l` | int |
| `end_line` | `e` | int |
| `language` | `g` | string |
| `signature` | `s` | string |

**NodeKind enum values:**
- 0: File, 1: Module, 2: Class, 3: Struct, 4: Interface, 5: Trait, 6: Protocol, 7: Function, 8: Method, etc.

## Batch Query Tools

Batch tools eliminate round-trip overhead for multiple operations.

### Available Batch Tools

#### `coraline_batch_get_nodes`

Fetch multiple nodes in one call:

```json
{
  "node_ids": ["id1", "id2", "id3", "id4", "id5"],
  "output_format": "compact"
}
```

**Savings:** 90% vs 5 individual `coraline_node` calls

#### `coraline_batch_callers`

Get callers for multiple symbols:

```json
{
  "node_ids": ["func1", "func2", "func3"],
  "limit": 10,
  "output_format": "compact"
}
```

**Savings:** 70% vs 3 individual `coraline_callers` calls

#### `coraline_batch_callees`

Get callees for multiple symbols:

```json
{
  "node_ids": ["func1", "func2", "func3"],
  "limit": 10,
  "output_format": "compact"
}
```

**Savings:** 70% vs 3 individual `coraline_callees` calls

### When to Use Batch Tools

**Good use cases:**
- Getting details for search results (10-20 nodes)
- Analyzing multiple related functions
- Building comprehensive context
- Investigating multiple API endpoints

**Not ideal:**
- Single lookups (use regular tools)
- Very large batches (>50 items, split into multiple calls)
- Unrelated queries (batch overhead not worth it)

## Edge Filtering

Filter traversal operations to specific edge types for more targeted results.

### Edge Types

| Edge Kind | Meaning |
|---|---|
| `calls` | Function/method calls |
| `imports` | Import statements |
| `exports` | Export declarations |
| `extends` | Class inheritance |
| `implements` | Interface implementation |
| `references` | Generic references |
| `type_of` | Type annotations |
| `returns` | Return types |
| `instantiates` | Object creation |

### Usage

**Find only classes extending a base class:**
```json
{
  "node_id": "BaseClass_id",
  "edge_kinds": ["extends"],
  "output_format": "compact"
}
```

**Find only imports of a module:**
```json
{
  "node_id": "module_id",
  "edge_kinds": ["imports"]
}
```

**Find calls and references (exclude type-only):**
```json
{
  "node_id": "func_id",
  "edge_kinds": ["calls", "references"]
}
```

**Savings:** ~15% by excluding irrelevant edges from results.

## Result Limits

Control result set size to avoid overwhelming token budgets.

### Default Limits

| Tool | Default Limit | Recommended |
|---|---|---|
| `coraline_search` | 10 | 5-20 |
| `coraline_callers` | 20 | 10-50 |
| `coraline_callees` | 20 | 10-50 |
| `coraline_impact` | 50 nodes | 20-100 |
| `coraline_context` | 20 nodes | 10-30 |
| `coraline_semantic_search` | 10 | 5-15 |

### Adjusting Limits

**Search with tight limit:**
```json
{
  "query": "authenticate",
  "limit": 5,
  "output_format": "compact"
}
```

**Impact analysis with depth control:**
```json
{
  "node_id": "User_class",
  "max_depth": 2,
  "max_nodes": 30
}
```

**Progressive exploration:**
1. Start with `limit: 5`
2. If more needed, increase to `limit: 20`
3. Use batch tools for detailed inspection

## Precomputed Metrics

Coraline caches transitive caller/callee counts for instant impact analysis.

### Metrics Table

During indexing, Coraline computes:
- Transitive caller count (how many functions call this, directly or indirectly)
- Transitive callee count (how many functions this calls, directly or indirectly)

### Performance Impact

**Without metrics (BFS traversal):**
- 100-500ms per impact query

**With precomputed metrics:**
- <10ms per impact query (50x speedup)

### Usage

Metrics are used automatically by:
- `coraline_impact` - Fast hotspot identification
- `coraline_dependencies` - Quick dependency counts
- `coraline_dependents` - Fast reverse dependency counts

No special configuration required - metrics are computed during `coraline index` and `coraline sync`.

## Advanced Search Tools

Specialized search tools reduce iterative lookups.

### `coraline_search_by_signature`

Find symbols by type signature pattern:

```json
{
  "signature_pattern": "fn.*-> Result<User>",
  "limit": 10
}
```

**Use case:** Find all functions returning `Result<User>` without iterating through all functions.

### `coraline_search_by_docstring`

Search within documentation:

```json
{
  "query": "authentication middleware",
  "limit": 10
}
```

**Use case:** Find relevant code by intent/purpose, not just name.

### `coraline_search_exported_symbols`

Filter to public API only:

```json
{
  "query": "User",
  "limit": 10
}
```

**Use case:** Focus on public API surface, ignore internal helpers.

**Savings:** ~60% by finding the right symbol in one query instead of filtering results manually.

## Optimization Strategies

### Strategy 1: Start Narrow

1. Use specific search terms
2. Apply kind filters (`--kind function`)
3. Use small limits initially
4. Expand only if needed

**Example:**
```
Search for "authenticate" with kind=function and limit=5
→ Found it in first 5 results
→ Saved tokens from fetching 15 more results
```

### Strategy 2: Batch Similar Operations

1. Collect IDs from search
2. Use batch tools for details
3. Process results together

**Example:**
```
1. Search for all "User" classes → 5 IDs
2. Batch get details for 5 IDs → 1 call
3. vs 5 individual calls → 80% token savings
```

### Strategy 3: Use Edge Filters

1. Determine relationship type needed
2. Filter edges in query
3. Get only relevant results

**Example:**
```
Find classes extending BaseController (not just referencing it)
→ Use edge_kinds=["extends"]
→ Skip unrelated references
```

### Strategy 4: Incremental Context

1. Build initial context with `max_nodes=10`
2. Identify gaps
3. Fetch specific details with targeted queries
4. vs loading everything upfront

**Example:**
```
1. Context for "auth system" with max_nodes=10
2. Identified missing middleware details
3. Search for middleware specifically
4. Saved tokens from overly broad initial context
```

### Strategy 5: Compact by Default

1. Always use `output_format: "compact"`
2. Only request full format when displaying to user
3. Internal operations use compact

**Example:**
```
20 compact queries = 2000 tokens
vs 20 full queries = 7000 tokens
→ 70% savings
```

## Indexing Performance

### Large Projects (10,000+ files)

**Optimization tips:**

1. **Exclude aggressively:**
   ```toml
   [indexing]
   exclude_patterns = [
     "**/test/**",
     "**/tests/**",
     "**/node_modules/**",
     "**/vendor/**",
     "**/.venv/**",
     "**/dist/**",
     "**/build/**",
   ]
   ```

2. **Increase batch size:**
   ```toml
   [indexing]
   batch_size = 200
   ```

3. **Use incremental sync:**
   ```bash
   coraline sync  # vs coraline index
   ```

4. **Target specific paths:**
   ```toml
   [indexing]
   include_patterns = [
     "src/**/*.rs",
     "lib/**/*.ts",
   ]
   ```

### Benchmarks

| Project Size | Full Index | Incremental Sync | Memory |
|---|---|---|---|
| 1,000 files | ~10s | ~2s | ~50MB |
| 10,000 files | ~90s | ~8s | ~200MB |
| 50,000 files | ~450s | ~30s | ~800MB |

Times measured on M1 MacBook Pro.

## Semantic Search Performance

### Model Inference

| Model Variant | Size | Inference Time (384-dim) |
|---|---|---|
| `model_q4f16.onnx` | 111 MB | ~5ms/item |
| `model_int8.onnx` | 137 MB | ~8ms/item |
| `model_fp16.onnx` | 274 MB | ~12ms/item |
| `model.onnx` | 547 MB | ~20ms/item |

**Recommendation:** Use `model_int8.onnx` for best balance of size/speed/accuracy.

### Embedding Generation

| Project Size | Embedding Time (int8) |
|---|---|
| 1,000 nodes | ~10s |
| 10,000 nodes | ~90s |
| 50,000 nodes | ~450s |

**Tip:** Run `coraline embed` after large index operations, not during development.

## Memory Usage

### Database Size

| Nodes | Edges | DB Size |
|---|---|---|
| 1,000 | 2,500 | ~500KB |
| 10,000 | 25,000 | ~5MB |
| 100,000 | 250,000 | ~50MB |

### Vector Storage

| Nodes | Embedding Size (384-dim, fp32) |
|---|---|
| 1,000 | ~1.5MB |
| 10,000 | ~15MB |
| 100,000 | ~150MB |

### Runtime Memory

| Operation | Peak Memory |
|---|---|
| Indexing (10K files) | ~200MB |
| MCP server (idle) | ~30MB |
| Impact analysis | ~50MB |
| Semantic search | ~100MB (model loaded) |

## Best Practices Summary

1. **Always use compact output format** for internal operations
2. **Use batch tools** for multiple related queries (3+ items)
3. **Apply edge filters** when relationship type is known
4. **Start with small limits** and expand if needed
5. **Use incremental sync** instead of full reindex
6. **Exclude test/fixture directories** from indexing
7. **Use precomputed metrics** for impact analysis
8. **Cache results** in project memories for repeated contexts
9. **Use advanced search tools** to find symbols in one query
10. **Enable git hooks** for automatic incremental updates

## Monitoring Performance

### Enable Debug Logging

```bash
CORALINE_LOG=debug coraline index
```

Check logs for timing information:

```bash
tail -f .coraline/logs/coraline.log
```

### Measure Token Usage

Track token savings in your AI assistant:
- Before optimization: Note token count for a task
- After optimization: Compare token count for same task
- Calculate savings: `(before - after) / before × 100%`

### Profile Indexing

```bash
time coraline index
time coraline sync
```

Compare times to identify bottlenecks.

## Future Optimizations

Planned features for even better performance:

- **GCX1 wire format** - 27% additional token reduction (vs compact JSON)
- **Multi-repo workspaces** - Share indexes across related projects
- **Speculative execution** - Preview changes without reindexing
- **Streaming responses** - Start processing results before all data arrives
- **Incremental embeddings** - Update only changed vectors

See [Development Guide](./development.md) for contribution opportunities.
