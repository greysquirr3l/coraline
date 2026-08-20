#![forbid(unsafe_code)]

//! Graph query tools for exploring the code graph

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::db;
use crate::graph;
use crate::types::{EdgeKind, NodeKind, TraversalDirection, TraversalOptions};

use super::{Tool, ToolError, ToolResult};

/// Tool for searching nodes by name or pattern
pub struct SearchTool {
    project_root: PathBuf,
}

impl SearchTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for SearchTool {
    fn name(&self) -> &'static str {
        "coraline_search"
    }

    fn description(&self) -> &'static str {
        "Search for code symbols by name or pattern across the indexed codebase"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query (symbol name or pattern)"
                },
                "kind": {
                    "type": "string",
                    "description": "Node kind filter (function, class, method, etc.)",
                    "enum": ["function", "method", "class", "struct", "interface", "trait", "module"]
                },
                "limit": {
                    "type": "number",
                    "description": "Maximum number of results to return",
                    "default": 10
                },
                "output_format": {
                    "type": "string",
                    "description": "Output format: 'full' (verbose) or 'compact' (65% token reduction)",
                    "enum": ["full", "compact"],
                    "default": "full"
                }
            },
            "required": ["query"]
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let query = params
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_params("query must be a string"))?;

        let kind = params
            .get("kind")
            .and_then(Value::as_str)
            .and_then(|s| match s {
                "function" => Some(NodeKind::Function),
                "method" => Some(NodeKind::Method),
                "class" => Some(NodeKind::Class),
                "struct" => Some(NodeKind::Struct),
                "interface" => Some(NodeKind::Interface),
                "trait" => Some(NodeKind::Trait),
                "module" => Some(NodeKind::Module),
                _ => None,
            });

        let limit = usize::try_from(params.get("limit").and_then(Value::as_u64).unwrap_or(10))
            .unwrap_or(usize::MAX);

        let output_format = params
            .get("output_format")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<super::OutputFormat>().ok())
            .unwrap_or_default();

        let conn = db::open_database(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to open database: {e}")))?;

        let results = db::search_nodes(&conn, query, kind, limit)
            .map_err(|e| ToolError::internal_error(format!("Search failed: {e}")))?;

        let results_json: Vec<Value> = results
            .into_iter()
            .map(|r| {
                let node_json = super::serialize_node(&r.node, output_format);
                if output_format == super::OutputFormat::Compact {
                    // Compact format: merge score into node object
                    if let Some(obj) = node_json.as_object() {
                        let mut obj = obj.clone();
                        obj.insert("sc".to_string(), json!(r.score));
                        Value::Object(obj)
                    } else {
                        node_json
                    }
                } else {
                    json!({
                        "node": node_json,
                        "score": r.score,
                    })
                }
            })
            .collect();

        Ok(json!({
            "results": results_json,
            "count": results_json.len(),
        }))
    }
}

/// Tool for finding callers of a function/method
pub struct CallersTool {
    project_root: PathBuf,
}

impl CallersTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for CallersTool {
    fn name(&self) -> &'static str {
        "coraline_callers"
    }

    fn description(&self) -> &'static str {
        "Find all functions/methods that call a given symbol. Filter by edge type for precise queries.\n\
         \n\
         Examples:\n\
         - Find all callers: {\"node_id\": \"my_function\"}\n\
         - Find only direct calls: {\"node_id\": \"my_function\", \"edge_kind\": \"calls\"}\n\
         - Find classes extending: {\"node_id\": \"MyClass\", \"edge_kind\": \"extends\"}\n\
         - Find implementers: {\"node_id\": \"MyInterface\", \"edge_kind\": \"implements\"}"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "node_id": {
                    "type": "string",
                    "description": "ID of the node to find callers for"
                },
                "edge_kind": {
                    "type": "string",
                    "description": "Filter by edge kind (calls, imports, extends, implements, references)",
                    "enum": ["calls", "imports", "extends", "implements", "references"]
                },
                "limit": {
                    "type": "number",
                    "description": "Maximum number of callers to return",
                    "default": 20
                },
                "output_format": {
                    "type": "string",
                    "description": "Output format: 'full' (verbose) or 'compact' (65% token reduction)",
                    "enum": ["full", "compact"],
                    "default": "full"
                }
            },
            "required": ["node_id"]
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let node_id = params
            .get("node_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_params("node_id must be a string"))?;

        let edge_kind =
            params
                .get("edge_kind")
                .and_then(Value::as_str)
                .map_or(Some(EdgeKind::Calls), |s| match s {
                    "calls" => Some(EdgeKind::Calls),
                    "imports" => Some(EdgeKind::Imports),
                    "extends" => Some(EdgeKind::Extends),
                    "implements" => Some(EdgeKind::Implements),
                    "references" => Some(EdgeKind::References),
                    _ => None,
                });

        let limit = usize::try_from(params.get("limit").and_then(Value::as_u64).unwrap_or(20))
            .unwrap_or(usize::MAX);

        let output_format = params
            .get("output_format")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<super::OutputFormat>().ok())
            .unwrap_or_default();

        let conn = db::open_database(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to open database: {e}")))?;

        // Get edges where this node is the target
        let edges = db::get_edges_by_target(&conn, node_id, edge_kind, limit)
            .map_err(|e| ToolError::internal_error(format!("Failed to get edges: {e}")))?;

        let mut callers = Vec::new();
        for edge in edges {
            if let Some(caller) = db::get_node_by_id(&conn, &edge.source)
                .map_err(|e| ToolError::internal_error(format!("Failed to get node: {e}")))?
            {
                if output_format == super::OutputFormat::Compact {
                    let mut node_json = super::node_to_compact_json(&caller);
                    if edge.line.is_some()
                        && let Some(obj) = node_json.as_object_mut()
                    {
                        obj.insert("ln".to_string(), json!(edge.line));
                    }
                    callers.push(node_json);
                } else {
                    callers.push(json!({
                        "id": caller.id,
                        "kind": caller.kind,
                        "name": caller.name,
                        "qualified_name": caller.qualified_name,
                        "file_path": caller.file_path,
                        "start_line": caller.start_line,
                        "line": edge.line,
                    }));
                }
            }
        }

        Ok(json!({
            "callers": callers,
            "count": callers.len(),
        }))
    }
}

/// Tool for finding callees (what a function calls)
pub struct CalleesTool {
    project_root: PathBuf,
}

impl CalleesTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for CalleesTool {
    fn name(&self) -> &'static str {
        "coraline_callees"
    }

    fn description(&self) -> &'static str {
        "Find all functions/methods that a given symbol calls. Filter by edge type for precise queries.\n\
         \n\
         Examples:\n\
         - Find all callees: {\"node_id\": \"my_function\"}\n\
         - Find only function calls: {\"node_id\": \"my_function\", \"edge_kind\": \"calls\"}\n\
         - Find imports: {\"node_id\": \"my_module\", \"edge_kind\": \"imports\"}\n\
         - Find references: {\"node_id\": \"MyClass\", \"edge_kind\": \"references\"}"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "node_id": {
                    "type": "string",
                    "description": "ID of the node to find callees for"
                },
                "edge_kind": {
                    "type": "string",
                    "description": "Filter by edge kind (calls, imports, extends, implements, references)",
                    "enum": ["calls", "imports", "extends", "implements", "references"]
                },
                "limit": {
                    "type": "number",
                    "description": "Maximum number of callees to return",
                    "default": 20
                },
                "output_format": {
                    "type": "string",
                    "description": "Output format: 'full' (verbose) or 'compact' (65% token reduction)",
                    "enum": ["full", "compact"],
                    "default": "full"
                }
            },
            "required": ["node_id"]
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let node_id = params
            .get("node_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_params("node_id must be a string"))?;

        let edge_kind =
            params
                .get("edge_kind")
                .and_then(Value::as_str)
                .map_or(Some(EdgeKind::Calls), |s| match s {
                    "calls" => Some(EdgeKind::Calls),
                    "imports" => Some(EdgeKind::Imports),
                    "extends" => Some(EdgeKind::Extends),
                    "implements" => Some(EdgeKind::Implements),
                    "references" => Some(EdgeKind::References),
                    _ => None,
                });

        let limit = usize::try_from(params.get("limit").and_then(Value::as_u64).unwrap_or(20))
            .unwrap_or(usize::MAX);

        let output_format = params
            .get("output_format")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<super::OutputFormat>().ok())
            .unwrap_or_default();

        let conn = db::open_database(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to open database: {e}")))?;

        // Get edges where this node is the source
        let edges = db::get_edges_by_source(&conn, node_id, edge_kind, limit)
            .map_err(|e| ToolError::internal_error(format!("Failed to get edges: {e}")))?;

        let mut callees = Vec::new();
        for edge in edges {
            if let Some(callee) = db::get_node_by_id(&conn, &edge.target)
                .map_err(|e| ToolError::internal_error(format!("Failed to get node: {e}")))?
            {
                if output_format == super::OutputFormat::Compact {
                    let mut node_json = super::node_to_compact_json(&callee);
                    if edge.line.is_some()
                        && let Some(obj) = node_json.as_object_mut()
                    {
                        obj.insert("ln".to_string(), json!(edge.line));
                    }
                    callees.push(node_json);
                } else {
                    callees.push(json!({
                        "id": callee.id,
                        "kind": callee.kind,
                        "name": callee.name,
                        "qualified_name": callee.qualified_name,
                        "file_path": callee.file_path,
                        "start_line": callee.start_line,
                        "line": edge.line,
                    }));
                }
            }
        }

        Ok(json!({
            "callees": callees,
            "count": callees.len(),
        }))
    }
}

/// Tool for impact radius analysis
pub struct ImpactTool {
    project_root: PathBuf,
}

impl ImpactTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for ImpactTool {
    fn name(&self) -> &'static str {
        "coraline_impact"
    }

    fn description(&self) -> &'static str {
        "Analyze the impact radius of changing a symbol - what might be affected"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "node_id": {
                    "type": "string",
                    "description": "ID of the node to analyze impact for"
                },
                "max_depth": {
                    "type": "number",
                    "description": "Maximum traversal depth",
                    "default": 2
                },
                "max_nodes": {
                    "type": "number",
                    "description": "Maximum nodes to include in result",
                    "default": 50
                }
            },
            "required": ["node_id"]
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let node_id = params
            .get("node_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_params("node_id must be a string"))?;

        let max_depth = params
            .get("max_depth")
            .and_then(Value::as_u64)
            .map(|n| usize::try_from(n).unwrap_or(usize::MAX));
        let max_nodes = params
            .get("max_nodes")
            .and_then(Value::as_u64)
            .map(|n| usize::try_from(n).unwrap_or(usize::MAX));

        let conn = db::open_database(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to open database: {e}")))?;

        let traversal_options = TraversalOptions {
            max_depth,
            edge_kinds: Some(vec![EdgeKind::Calls, EdgeKind::References]),
            node_kinds: None,
            direction: Some(TraversalDirection::Incoming), // Find what depends on this
            limit: max_nodes,
            include_start: Some(true),
        };

        let subgraph = graph::build_subgraph(&conn, &[node_id.to_string()], &traversal_options)
            .map_err(|e| ToolError::internal_error(format!("Failed to build subgraph: {e}")))?;

        let nodes: Vec<Value> = subgraph
            .nodes
            .values()
            .map(|node| {
                json!({
                    "id": node.id,
                    "kind": node.kind,
                    "name": node.name,
                    "qualified_name": node.qualified_name,
                    "file_path": node.file_path,
                    "start_line": node.start_line,
                })
            })
            .collect();

        let edges: Vec<Value> = subgraph
            .edges
            .iter()
            .map(|edge| {
                json!({
                    "source": edge.source,
                    "target": edge.target,
                    "kind": edge.kind,
                    "line": edge.line,
                })
            })
            .collect();

        let files: std::collections::HashSet<_> =
            subgraph.nodes.values().map(|n| &n.file_path).collect();

        Ok(json!({
            "nodes": nodes,
            "edges": edges,
            "stats": {
                "node_count": nodes.len(),
                "edge_count": edges.len(),
                "file_count": files.len(),
                "max_depth": max_depth.unwrap_or(2),
            }
        }))
    }

    fn timeout_hint(&self) -> Option<u64> {
        Some(300_000)
    }
}

/// Tool for finding a symbol by name pattern (richer than search — returns hierarchy/depth info)
pub struct FindSymbolTool {
    project_root: PathBuf,
}

impl FindSymbolTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for FindSymbolTool {
    fn name(&self) -> &'static str {
        "coraline_find_symbol"
    }

    fn description(&self) -> &'static str {
        "Find symbols by exact name or substring pattern. Returns node metadata and optionally the source code body."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name_pattern": {
                    "type": "string",
                    "description": "Symbol name or substring to search for"
                },
                "kind": {
                    "type": "string",
                    "description": "Optional node kind filter",
                    "enum": ["function", "method", "class", "struct", "interface", "trait", "module"]
                },
                "include_body": {
                    "type": "boolean",
                    "description": "Whether to include the source code body of the symbol",
                    "default": false
                },
                "limit": {
                    "type": "number",
                    "description": "Maximum results to return",
                    "default": 10
                }
            },
            "required": ["name_pattern"]
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let pattern = params
            .get("name_pattern")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_params("name_pattern must be a string"))?;

        let kind = params
            .get("kind")
            .and_then(Value::as_str)
            .and_then(str_to_node_kind);

        let include_body = params
            .get("include_body")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let limit = usize::try_from(params.get("limit").and_then(Value::as_u64).unwrap_or(10))
            .unwrap_or(usize::MAX);

        let conn = db::open_database(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to open database: {e}")))?;

        // Use FTS search for the pattern
        let results = db::search_nodes(&conn, pattern, kind, limit)
            .map_err(|e| ToolError::internal_error(format!("Search failed: {e}")))?;

        let symbols: Vec<Value> = results
            .into_iter()
            .map(|r| {
                let body = if include_body {
                    read_node_source(&self.project_root, &r.node)
                } else {
                    None
                };
                json!({
                    "id": r.node.id,
                    "kind": r.node.kind,
                    "name": r.node.name,
                    "qualified_name": r.node.qualified_name,
                    "file_path": r.node.file_path,
                    "language": r.node.language,
                    "start_line": r.node.start_line,
                    "end_line": r.node.end_line,
                    "signature": r.node.signature,
                    "docstring": r.node.docstring,
                    "is_exported": r.node.is_exported,
                    "is_async": r.node.is_async,
                    "is_static": r.node.is_static,
                    "score": r.score,
                    "body": body,
                })
            })
            .collect();

        Ok(json!({ "symbols": symbols, "count": symbols.len() }))
    }
}

/// Tool for getting a symbol overview for a file
pub struct GetSymbolsOverviewTool {
    project_root: PathBuf,
}

impl GetSymbolsOverviewTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for GetSymbolsOverviewTool {
    fn name(&self) -> &'static str {
        "coraline_get_symbols_overview"
    }

    fn description(&self) -> &'static str {
        "Get an overview of all symbols in a file, grouped by kind and ordered by line number."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file (relative to project root or absolute)"
                }
            },
            "required": ["file_path"]
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let file_path = params
            .get("file_path")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_params("file_path must be a string"))?;

        // Normalise: if relative, make absolute using project root
        let abs_path = if std::path::Path::new(file_path).is_absolute() {
            file_path.to_string()
        } else {
            self.project_root
                .join(file_path)
                .to_string_lossy()
                .to_string()
        };

        let conn = db::open_database(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to open database: {e}")))?;

        let nodes = db::get_nodes_by_file(&conn, &abs_path, None)
            .map_err(|e| ToolError::internal_error(format!("Failed to get nodes: {e}")))?;

        if nodes.is_empty() {
            // Try with the path as-is (might be stored relative)
            let nodes_fallback = db::get_nodes_by_file(&conn, file_path, None)
                .map_err(|e| ToolError::internal_error(format!("Failed to get nodes: {e}")))?;

            return Ok(build_overview_response(&nodes_fallback, file_path));
        }

        Ok(build_overview_response(&nodes, &abs_path))
    }
}

fn build_overview_response(nodes: &[crate::types::Node], file_path: &str) -> Value {
    use std::collections::HashMap;

    let mut by_kind: HashMap<String, Vec<Value>> = HashMap::new();

    for node in nodes {
        let kind_str = format!("{:?}", node.kind).to_lowercase();
        by_kind.entry(kind_str).or_default().push(json!({
            "id": node.id,
            "name": node.name,
            "qualified_name": node.qualified_name,
            "start_line": node.start_line,
            "end_line": node.end_line,
            "signature": node.signature,
            "is_exported": node.is_exported,
        }));
    }

    let symbols: Vec<Value> = nodes
        .iter()
        .map(|n| {
            json!({
                "id": n.id,
                "kind": n.kind,
                "name": n.name,
                "start_line": n.start_line,
                "end_line": n.end_line,
                "signature": n.signature,
            })
        })
        .collect();

    json!({
        "file_path": file_path,
        "symbol_count": nodes.len(),
        "by_kind": by_kind,
        "symbols": symbols,
    })
}

/// Tool for finding all references to a node
pub struct FindReferencesTool {
    project_root: PathBuf,
}

impl FindReferencesTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for FindReferencesTool {
    fn name(&self) -> &'static str {
        "coraline_find_references"
    }

    fn description(&self) -> &'static str {
        "Find all nodes that reference (call, import, extend, implement, etc.) a given symbol."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "node_id": {
                    "type": "string",
                    "description": "ID of the node to find references to"
                },
                "edge_kind": {
                    "type": "string",
                    "description": "Filter by edge kind (calls, imports, extends, implements, references)",
                    "enum": ["calls", "imports", "extends", "implements", "references"]
                },
                "limit": {
                    "type": "number",
                    "description": "Maximum number of references to return",
                    "default": 50
                }
            },
            "required": ["node_id"]
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let node_id = params
            .get("node_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_params("node_id must be a string"))?;

        let edge_kind = params
            .get("edge_kind")
            .and_then(Value::as_str)
            .and_then(|s| match s {
                "calls" => Some(EdgeKind::Calls),
                "imports" => Some(EdgeKind::Imports),
                "extends" => Some(EdgeKind::Extends),
                "implements" => Some(EdgeKind::Implements),
                "references" => Some(EdgeKind::References),
                _ => None,
            });

        let limit = usize::try_from(params.get("limit").and_then(Value::as_u64).unwrap_or(50))
            .unwrap_or(usize::MAX);

        let conn = db::open_database(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to open database: {e}")))?;

        let edges = db::get_edges_by_target(&conn, node_id, edge_kind, limit)
            .map_err(|e| ToolError::internal_error(format!("Failed to get edges: {e}")))?;

        let mut references = Vec::new();
        for edge in &edges {
            if let Some(node) = db::get_node_by_id(&conn, &edge.source)
                .map_err(|e| ToolError::internal_error(format!("Failed to get node: {e}")))?
            {
                references.push(json!({
                    "id": node.id,
                    "kind": node.kind,
                    "name": node.name,
                    "qualified_name": node.qualified_name,
                    "file_path": node.file_path,
                    "start_line": node.start_line,
                    "edge_kind": edge.kind,
                    "edge_line": edge.line,
                }));
            }
        }

        Ok(json!({
            "node_id": node_id,
            "references": references,
            "count": references.len(),
        }))
    }
}

/// Tool for getting full node details including source code
pub struct GetNodeTool {
    project_root: PathBuf,
}

impl GetNodeTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for GetNodeTool {
    fn name(&self) -> &'static str {
        "coraline_node"
    }

    fn description(&self) -> &'static str {
        "Get complete details for a specific node by ID, including its source code body."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "node_id": {
                    "type": "string",
                    "description": "The node ID to retrieve"
                },
                "include_edges": {
                    "type": "boolean",
                    "description": "Whether to include outgoing and incoming edge counts",
                    "default": false
                }
            },
            "required": ["node_id"]
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let node_id = params
            .get("node_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_params("node_id must be a string"))?;

        let include_edges = params
            .get("include_edges")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let conn = db::open_database(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to open database: {e}")))?;

        let node = db::get_node_by_id(&conn, node_id)
            .map_err(|e| ToolError::internal_error(format!("Failed to get node: {e}")))?
            .ok_or_else(|| ToolError::not_found(format!("Node not found: {node_id}")))?;

        let body = read_node_source(&self.project_root, &node);

        let mut result = json!({
            "id": node.id,
            "kind": node.kind,
            "name": node.name,
            "qualified_name": node.qualified_name,
            "file_path": node.file_path,
            "language": node.language,
            "start_line": node.start_line,
            "end_line": node.end_line,
            "start_column": node.start_column,
            "end_column": node.end_column,
            "signature": node.signature,
            "docstring": node.docstring,
            "visibility": node.visibility,
            "is_exported": node.is_exported,
            "is_async": node.is_async,
            "is_static": node.is_static,
            "is_abstract": node.is_abstract,
            "decorators": node.decorators,
            "type_parameters": node.type_parameters,
            "body": body,
        });

        if include_edges {
            let out_edges = db::get_edges_by_source(&conn, node_id, None, 200)
                .map_err(|e| ToolError::internal_error(format!("Failed to get edges: {e}")))?;
            let in_edges = db::get_edges_by_target(&conn, node_id, None, 200)
                .map_err(|e| ToolError::internal_error(format!("Failed to get edges: {e}")))?;
            if let Some(obj) = result.as_object_mut() {
                obj.insert("outgoing_edge_count".to_string(), json!(out_edges.len()));
                obj.insert("incoming_edge_count".to_string(), json!(in_edges.len()));
            }
        }

        Ok(result)
    }
}

/// Tool for the outgoing dependency graph — everything a node depends on.
pub struct DependenciesTool {
    project_root: PathBuf,
}

impl DependenciesTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for DependenciesTool {
    fn name(&self) -> &'static str {
        "coraline_dependencies"
    }

    fn description(&self) -> &'static str {
        "Get the outgoing dependency graph for a node — everything this symbol \
         depends on (calls, imports, references, etc.), traversed up to a given depth. \
         Broader than coraline_callees: follows all edge kinds, multiple hops."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "node_id": {
                    "type": "string",
                    "description": "ID of the node to find dependencies for"
                },
                "depth": {
                    "type": "number",
                    "description": "Traversal depth (default 2)",
                    "default": 2
                },
                "limit": {
                    "type": "number",
                    "description": "Maximum number of nodes to return (default 50)",
                    "default": 50
                }
            },
            "required": ["node_id"]
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let node_id = params
            .get("node_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_params("node_id must be a string"))?;

        let depth = params
            .get("depth")
            .and_then(Value::as_u64)
            .map(|n| usize::try_from(n).unwrap_or(usize::MAX));
        let limit = params
            .get("limit")
            .and_then(Value::as_u64)
            .map(|n| usize::try_from(n).unwrap_or(usize::MAX));

        let conn = db::open_database(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to open database: {e}")))?;

        let options = TraversalOptions {
            max_depth: depth.or(Some(2)),
            edge_kinds: None,
            node_kinds: None,
            direction: Some(TraversalDirection::Outgoing),
            limit: limit.or(Some(50)),
            include_start: Some(false),
        };

        let subgraph = graph::build_subgraph(&conn, &[node_id.to_string()], &options)
            .map_err(|e| ToolError::internal_error(format!("Graph traversal failed: {e}")))?;

        let nodes: Vec<Value> = subgraph
            .nodes
            .values()
            .map(|n| {
                json!({
                    "id": n.id,
                    "kind": n.kind,
                    "name": n.name,
                    "qualified_name": n.qualified_name,
                    "file_path": n.file_path,
                    "start_line": n.start_line,
                })
            })
            .collect();

        let edges: Vec<Value> = subgraph
            .edges
            .iter()
            .map(|e| {
                json!({
                    "source": e.source,
                    "target": e.target,
                    "kind": e.kind,
                    "line": e.line,
                })
            })
            .collect();

        Ok(json!({
            "node_id": node_id,
            "dependencies": nodes,
            "edges": edges,
            "count": nodes.len(),
        }))
    }
}

/// Tool for the incoming dependency graph — everything that depends on a node.
pub struct DependentsTool {
    project_root: PathBuf,
}

impl DependentsTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for DependentsTool {
    fn name(&self) -> &'static str {
        "coraline_dependents"
    }

    fn description(&self) -> &'static str {
        "Get the incoming dependency graph for a node — everything that depends on this \
         symbol (all callers, importers, referencers), traversed up to a given depth. \
         Broader than coraline_callers: follows all edge kinds, multiple hops."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "node_id": {
                    "type": "string",
                    "description": "ID of the node"
                },
                "depth": {
                    "type": "number",
                    "description": "Traversal depth (default 2)",
                    "default": 2
                },
                "limit": {
                    "type": "number",
                    "description": "Maximum number of nodes to return (default 50)",
                    "default": 50
                }
            },
            "required": ["node_id"]
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let node_id = params
            .get("node_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_params("node_id must be a string"))?;

        let depth = params
            .get("depth")
            .and_then(Value::as_u64)
            .map(|n| usize::try_from(n).unwrap_or(usize::MAX));
        let limit = params
            .get("limit")
            .and_then(Value::as_u64)
            .map(|n| usize::try_from(n).unwrap_or(usize::MAX));

        let conn = db::open_database(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to open database: {e}")))?;

        let options = TraversalOptions {
            max_depth: depth.or(Some(2)),
            edge_kinds: None,
            node_kinds: None,
            direction: Some(TraversalDirection::Incoming),
            limit: limit.or(Some(50)),
            include_start: Some(false),
        };

        let subgraph = graph::build_subgraph(&conn, &[node_id.to_string()], &options)
            .map_err(|e| ToolError::internal_error(format!("Graph traversal failed: {e}")))?;

        let nodes: Vec<Value> = subgraph
            .nodes
            .values()
            .map(|n| {
                json!({
                    "id": n.id,
                    "kind": n.kind,
                    "name": n.name,
                    "qualified_name": n.qualified_name,
                    "file_path": n.file_path,
                    "start_line": n.start_line,
                })
            })
            .collect();

        let edges: Vec<Value> = subgraph
            .edges
            .iter()
            .map(|e| {
                json!({
                    "source": e.source,
                    "target": e.target,
                    "kind": e.kind,
                    "line": e.line,
                })
            })
            .collect();

        Ok(json!({
            "node_id": node_id,
            "dependents": nodes,
            "edges": edges,
            "count": nodes.len(),
        }))
    }
}

/// Tool for finding the shortest directed path between two nodes.
pub struct PathTool {
    project_root: PathBuf,
}

impl PathTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for PathTool {
    fn name(&self) -> &'static str {
        "coraline_path"
    }

    fn description(&self) -> &'static str {
        "Find the shortest directed path through the call/reference graph between two symbols. \
         Useful for understanding indirect dependencies — how does symbol A transitively lead to B?"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "from_id": {
                    "type": "string",
                    "description": "Starting node ID"
                },
                "to_id": {
                    "type": "string",
                    "description": "Target node ID"
                },
                "max_depth": {
                    "type": "number",
                    "description": "Maximum path length to search (default 6)",
                    "default": 6
                }
            },
            "required": ["from_id", "to_id"]
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        use std::collections::{HashMap, VecDeque};

        let from_id = params
            .get("from_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_params("from_id must be a string"))?;

        let to_id = params
            .get("to_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_params("to_id must be a string"))?;

        let max_depth =
            usize::try_from(params.get("max_depth").and_then(Value::as_u64).unwrap_or(6))
                .unwrap_or(usize::MAX);

        let conn = db::open_database(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to open database: {e}")))?;

        // BFS following outgoing edges, recording parents for path reconstruction.

        // Maps node_id → parent_id (empty string for the root).
        let mut parent: HashMap<String, String> = HashMap::new();
        parent.insert(from_id.to_string(), String::new());

        let mut queue: VecDeque<(String, usize)> = VecDeque::new();
        queue.push_back((from_id.to_string(), 0));

        let mut found = false;
        'bfs: while let Some((current, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }
            let edges = db::get_edges_by_source(&conn, &current, None, 500)
                .map_err(|e| ToolError::internal_error(format!("Edge query failed: {e}")))?;
            for edge in edges {
                if parent.contains_key(&edge.target) {
                    continue;
                }
                parent.insert(edge.target.clone(), current.clone());
                if edge.target == to_id {
                    found = true;
                    break 'bfs;
                }
                queue.push_back((edge.target.clone(), depth + 1));
            }
        }

        if !found {
            return Ok(json!({
                "from_id": from_id,
                "to_id": to_id,
                "path_found": false,
                "path": [],
                "message": format!(
                    "No directed path found from {from_id} to {to_id} within depth {max_depth}"
                ),
            }));
        }

        // Reconstruct path by walking parents backward from to_id.
        let mut path_ids: Vec<String> = Vec::new();
        let mut cursor = to_id.to_string();
        while !cursor.is_empty() {
            path_ids.push(cursor.clone());
            cursor = parent.get(&cursor).cloned().unwrap_or_default();
        }
        path_ids.reverse();

        let path: Vec<Value> = path_ids
            .iter()
            .filter_map(|id| db::get_node_by_id(&conn, id).ok().flatten())
            .map(|n| {
                json!({
                    "id": n.id,
                    "kind": n.kind,
                    "name": n.name,
                    "qualified_name": n.qualified_name,
                    "file_path": n.file_path,
                    "start_line": n.start_line,
                })
            })
            .collect();

        Ok(json!({
            "from_id": from_id,
            "to_id": to_id,
            "path_found": true,
            "path": path,
            "length": path.len(),
        }))
    }
}

/// Tool for detailed graph statistics broken down by language, node kind, and edge kind.
pub struct StatsTool {
    project_root: PathBuf,
}

impl StatsTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for StatsTool {
    fn name(&self) -> &'static str {
        "coraline_stats"
    }

    fn description(&self) -> &'static str {
        "Return detailed graph statistics: total counts, per-language file breakdown, node kind breakdown, and edge kind breakdown."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    fn execute(&self, _params: Value) -> ToolResult {
        let conn = db::open_database(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to open database: {e}")))?;

        // Basic counts
        let node_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))
            .map_err(|e| ToolError::internal_error(format!("Query failed: {e}")))?;
        let edge_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
            .map_err(|e| ToolError::internal_error(format!("Query failed: {e}")))?;
        let file_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
            .map_err(|e| ToolError::internal_error(format!("Query failed: {e}")))?;
        let unresolved_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM unresolved_refs", [], |r| r.get(0))
            .map_err(|e| ToolError::internal_error(format!("Query failed: {e}")))?;
        let vector_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM vectors", [], |r| r.get(0))
            .map_err(|e| ToolError::internal_error(format!("Query failed: {e}")))?;

        // Files by language
        let mut by_language = serde_json::Map::new();
        {
            let mut stmt = conn
                .prepare("SELECT language, COUNT(*) FROM files GROUP BY language ORDER BY 2 DESC")
                .map_err(|e| ToolError::internal_error(format!("Query failed: {e}")))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .map_err(|e| ToolError::internal_error(format!("Query failed: {e}")))?;
            for row in rows.flatten() {
                by_language.insert(row.0, Value::Number(row.1.into()));
            }
        }

        // Nodes by kind
        let mut by_kind = serde_json::Map::new();
        {
            let mut stmt = conn
                .prepare("SELECT kind, COUNT(*) FROM nodes GROUP BY kind ORDER BY 2 DESC")
                .map_err(|e| ToolError::internal_error(format!("Query failed: {e}")))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .map_err(|e| ToolError::internal_error(format!("Query failed: {e}")))?;
            for row in rows.flatten() {
                by_kind.insert(row.0, Value::Number(row.1.into()));
            }
        }

        // Edges by kind
        let mut by_edge_kind = serde_json::Map::new();
        {
            let mut stmt = conn
                .prepare("SELECT kind, COUNT(*) FROM edges GROUP BY kind ORDER BY 2 DESC")
                .map_err(|e| ToolError::internal_error(format!("Query failed: {e}")))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .map_err(|e| ToolError::internal_error(format!("Query failed: {e}")))?;
            for row in rows.flatten() {
                by_edge_kind.insert(row.0, Value::Number(row.1.into()));
            }
        }

        Ok(json!({
            "totals": {
                "nodes": node_count,
                "edges": edge_count,
                "files": file_count,
                "unresolved_references": unresolved_count,
                "vectors": vector_count,
            },
            "files_by_language": by_language,
            "nodes_by_kind": by_kind,
            "edges_by_kind": by_edge_kind,
        }))
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Read the source lines for a node from its file on disk.
fn read_node_source(project_root: &std::path::Path, node: &crate::types::Node) -> Option<String> {
    let path = if std::path::Path::new(&node.file_path).is_absolute() {
        std::path::PathBuf::from(&node.file_path)
    } else {
        project_root.join(&node.file_path)
    };

    let text = std::fs::read_to_string(&path).ok()?;
    let lines: Vec<&str> = text.lines().collect();

    let start = usize::try_from(node.start_line)
        .unwrap_or(0)
        .saturating_sub(1);
    let end = usize::try_from(node.end_line).unwrap_or(0).min(lines.len());

    lines.get(start..end).map(|l| l.join("\n"))
}

/// Convert a string to a [`NodeKind`] (shared helper).
fn str_to_node_kind(s: &str) -> Option<NodeKind> {
    match s {
        "function" => Some(NodeKind::Function),
        "method" => Some(NodeKind::Method),
        "class" => Some(NodeKind::Class),
        "struct" => Some(NodeKind::Struct),
        "interface" => Some(NodeKind::Interface),
        "trait" => Some(NodeKind::Trait),
        "module" => Some(NodeKind::Module),
        _ => None,
    }
}

/// Tool for batch fetching multiple nodes by ID (60% token savings)
pub struct BatchGetNodesTool {
    project_root: PathBuf,
}

impl BatchGetNodesTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for BatchGetNodesTool {
    fn name(&self) -> &'static str {
        "coraline_batch_get_nodes"
    }

    fn description(&self) -> &'static str {
        "Fetch multiple nodes by ID in a single call. Reduces round-trip overhead for bulk lookups.\n\
         \n\
         Examples:\n\
         - Get 5 nodes: {\"node_ids\": [\"id1\", \"id2\", \"id3\", \"id4\", \"id5\"]}\n\
         - With code bodies: {\"node_ids\": [\"id1\", \"id2\"], \"include_body\": true}"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "node_ids": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Array of node IDs to fetch"
                },
                "include_body": {
                    "type": "boolean",
                    "description": "Include source code body for each node",
                    "default": false
                },
                "output_format": {
                    "type": "string",
                    "description": "Output format: 'full' (verbose) or 'compact' (65% token reduction)",
                    "enum": ["full", "compact"],
                    "default": "full"
                }
            },
            "required": ["node_ids"]
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let node_ids = params
            .get("node_ids")
            .and_then(Value::as_array)
            .ok_or_else(|| ToolError::invalid_params("node_ids must be an array"))?;

        let include_body = params
            .get("include_body")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let output_format = params
            .get("output_format")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<super::OutputFormat>().ok())
            .unwrap_or_default();

        let conn = db::open_database(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to open database: {e}")))?;

        let mut results = Vec::new();
        let mut not_found = Vec::new();

        for id_val in node_ids {
            let id = id_val
                .as_str()
                .ok_or_else(|| ToolError::invalid_params("All node_ids must be strings"))?;

            match db::get_node_by_id(&conn, id) {
                Ok(Some(node)) => {
                    let mut node_json = super::serialize_node(&node, output_format);

                    if include_body
                        && let (Some(body), Some(obj)) = (
                            read_node_source(&self.project_root, &node),
                            node_json.as_object_mut(),
                        )
                    {
                        let key = if output_format == super::OutputFormat::Compact {
                            "b"
                        } else {
                            "body"
                        };
                        obj.insert(key.to_string(), json!(body));
                    }

                    results.push(node_json);
                }
                Ok(None) => {
                    not_found.push(id.to_string());
                }
                Err(e) => {
                    return Err(ToolError::internal_error(format!(
                        "Failed to fetch node {id}: {e}"
                    )));
                }
            }
        }

        Ok(json!({
            "nodes": results,
            "count": results.len(),
            "not_found": not_found,
        }))
    }
}

/// Tool for batch fetching callers of multiple symbols (60% token savings)
pub struct BatchCallersTool {
    project_root: PathBuf,
}

impl BatchCallersTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for BatchCallersTool {
    fn name(&self) -> &'static str {
        "coraline_batch_callers"
    }

    fn description(&self) -> &'static str {
        "Get callers for multiple symbols in a single call. Eliminates round-trip overhead.\n\
         \n\
         Examples:\n\
         - Get callers for 3 functions: {\"node_ids\": [\"func1\", \"func2\", \"func3\"]}\n\
         - Limit per symbol: {\"node_ids\": [\"id1\", \"id2\"], \"limit_per_node\": 10}"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "node_ids": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Array of node IDs to find callers for"
                },
                "limit_per_node": {
                    "type": "number",
                    "description": "Maximum callers to return per node",
                    "default": 20
                }
            },
            "required": ["node_ids"]
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let node_ids = params
            .get("node_ids")
            .and_then(Value::as_array)
            .ok_or_else(|| ToolError::invalid_params("node_ids must be an array"))?;

        let limit = usize::try_from(
            params
                .get("limit_per_node")
                .and_then(Value::as_u64)
                .unwrap_or(20),
        )
        .unwrap_or(usize::MAX);

        let conn = db::open_database(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to open database: {e}")))?;

        let mut results = serde_json::Map::new();

        for id_val in node_ids {
            let id = id_val
                .as_str()
                .ok_or_else(|| ToolError::invalid_params("All node_ids must be strings"))?;

            let edges =
                db::get_edges_by_target(&conn, id, Some(EdgeKind::Calls), limit).map_err(|e| {
                    ToolError::internal_error(format!("Failed to get callers for {id}: {e}"))
                })?;

            let callers: Vec<Value> = edges
                .iter()
                .filter_map(|edge| {
                    db::get_node_by_id(&conn, &edge.source)
                        .ok()
                        .and_then(|opt| opt)
                        .map(|node| {
                            json!({
                                "id": node.id,
                                "kind": node.kind,
                                "name": node.name,
                                "qualified_name": node.qualified_name,
                                "file_path": node.file_path,
                                "start_line": node.start_line,
                            })
                        })
                })
                .collect();

            results.insert(id.to_string(), json!(callers));
        }

        Ok(json!({
            "callers": results,
            "node_count": results.len(),
        }))
    }
}

/// Tool for batch fetching callees of multiple symbols (60% token savings)
pub struct BatchCalleesTool {
    project_root: PathBuf,
}

impl BatchCalleesTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for BatchCalleesTool {
    fn name(&self) -> &'static str {
        "coraline_batch_callees"
    }

    fn description(&self) -> &'static str {
        "Get callees for multiple symbols in a single call. Eliminates round-trip overhead.\n\
         \n\
         Examples:\n\
         - Get callees for 3 functions: {\"node_ids\": [\"func1\", \"func2\", \"func3\"]}\n\
         - Limit per symbol: {\"node_ids\": [\"id1\", \"id2\"], \"limit_per_node\": 10}"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "node_ids": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Array of node IDs to find callees for"
                },
                "limit_per_node": {
                    "type": "number",
                    "description": "Maximum callees to return per node",
                    "default": 20
                }
            },
            "required": ["node_ids"]
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let node_ids = params
            .get("node_ids")
            .and_then(Value::as_array)
            .ok_or_else(|| ToolError::invalid_params("node_ids must be an array"))?;

        let limit = usize::try_from(
            params
                .get("limit_per_node")
                .and_then(Value::as_u64)
                .unwrap_or(20),
        )
        .unwrap_or(usize::MAX);

        let conn = db::open_database(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to open database: {e}")))?;

        let mut results = serde_json::Map::new();

        for id_val in node_ids {
            let id = id_val
                .as_str()
                .ok_or_else(|| ToolError::invalid_params("All node_ids must be strings"))?;

            let edges =
                db::get_edges_by_source(&conn, id, Some(EdgeKind::Calls), limit).map_err(|e| {
                    ToolError::internal_error(format!("Failed to get callees for {id}: {e}"))
                })?;

            let callees: Vec<Value> = edges
                .iter()
                .filter_map(|edge| {
                    db::get_node_by_id(&conn, &edge.target)
                        .ok()
                        .and_then(|opt| opt)
                        .map(|node| {
                            json!({
                                "id": node.id,
                                "kind": node.kind,
                                "name": node.name,
                                "qualified_name": node.qualified_name,
                                "file_path": node.file_path,
                                "start_line": node.start_line,
                            })
                        })
                })
                .collect();

            results.insert(id.to_string(), json!(callees));
        }

        Ok(json!({
            "callees": results,
            "node_count": results.len(),
        }))
    }
}

/// Tool for searching by type signature patterns (60% token savings for type lookups)
pub struct SearchBySignatureTool {
    project_root: PathBuf,
}

impl SearchBySignatureTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for SearchBySignatureTool {
    fn name(&self) -> &'static str {
        "coraline_search_by_signature"
    }

    fn description(&self) -> &'static str {
        "Find symbols by type signature pattern. Faster than text search for finding functions by return type or parameters.\n\
         \n\
         Examples:\n\
         - Find functions returning Result: {\"pattern\": \"Result<\"}\n\
         - Find async functions: {\"pattern\": \"async fn\"}\n\
         - Find generic functions: {\"pattern\": \"<T>\"}\n\
         - Filter by kind: {\"pattern\": \"-> bool\", \"kind\": \"function\"}"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Signature pattern to search for (case-insensitive substring match)"
                },
                "kind": {
                    "type": "string",
                    "description": "Node kind filter",
                    "enum": ["function", "method", "class", "struct", "interface", "trait"]
                },
                "limit": {
                    "type": "number",
                    "description": "Maximum results to return",
                    "default": 20
                },
                "output_format": {
                    "type": "string",
                    "description": "Output format: 'full' or 'compact' (65% token reduction)",
                    "enum": ["full", "compact"],
                    "default": "full"
                }
            },
            "required": ["pattern"]
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let pattern = params
            .get("pattern")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_params("pattern must be a string"))?;

        let kind_filter = params
            .get("kind")
            .and_then(Value::as_str)
            .and_then(str_to_node_kind);

        let limit = usize::try_from(params.get("limit").and_then(Value::as_u64).unwrap_or(20))
            .unwrap_or(usize::MAX);
        let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);

        let output_format = params
            .get("output_format")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<super::OutputFormat>().ok())
            .unwrap_or_default();

        let conn = db::open_database(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to open database: {e}")))?;

        // SQL query to search signatures (case-insensitive)
        let mut sql = "SELECT id, kind, name, qualified_name, file_path, language, start_line, end_line, \
                       start_column, end_column, docstring, signature, visibility, is_exported, is_async, \
                       is_static, is_abstract, decorators, type_parameters, updated_at \
                       FROM nodes WHERE signature IS NOT NULL AND LOWER(signature) LIKE LOWER(?)".to_string();

        if kind_filter.is_some() {
            sql.push_str(" AND kind = ?");
        }

        sql.push_str(" LIMIT ?");

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| ToolError::internal_error(format!("Failed to prepare query: {e}")))?;

        let pattern_param = format!("%{pattern}%");
        let rows = if let Some(kind) = kind_filter {
            stmt.query_map(
                rusqlite::params![&pattern_param, db::kind_to_string(kind), limit_i64],
                db::row_to_node,
            )
        } else {
            stmt.query_map(
                rusqlite::params![&pattern_param, limit_i64],
                db::row_to_node,
            )
        }
        .map_err(|e| ToolError::internal_error(format!("Query failed: {e}")))?;

        let mut results = Vec::new();
        for row in rows {
            let node =
                row.map_err(|e| ToolError::internal_error(format!("Row parsing failed: {e}")))?;
            results.push(super::serialize_node(&node, output_format));
        }

        Ok(json!({
            "results": results,
            "count": results.len(),
        }))
    }
}

/// Tool for searching by docstring content (60% token savings for documentation lookups)
pub struct SearchByDocstringTool {
    project_root: PathBuf,
}

impl SearchByDocstringTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for SearchByDocstringTool {
    fn name(&self) -> &'static str {
        "coraline_search_by_docstring"
    }

    fn description(&self) -> &'static str {
        "Search symbols by documentation/comment content. Find functions by what they do, not just their name.\n\
         \n\
         Examples:\n\
         - Find database operations: {\"query\": \"database connection\"}\n\
         - Find validation logic: {\"query\": \"validates input\"}\n\
         - Find deprecated code: {\"query\": \"deprecated\"}\n\
         - Filter by kind: {\"query\": \"calculates\", \"kind\": \"function\"}"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Text to search for in docstrings/comments"
                },
                "kind": {
                    "type": "string",
                    "description": "Node kind filter",
                    "enum": ["function", "method", "class", "struct", "interface", "trait"]
                },
                "limit": {
                    "type": "number",
                    "description": "Maximum results to return",
                    "default": 20
                },
                "output_format": {
                    "type": "string",
                    "description": "Output format: 'full' or 'compact' (65% token reduction)",
                    "enum": ["full", "compact"],
                    "default": "full"
                }
            },
            "required": ["query"]
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let query = params
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_params("query must be a string"))?;

        let kind_filter = params
            .get("kind")
            .and_then(Value::as_str)
            .and_then(str_to_node_kind);

        let limit = usize::try_from(params.get("limit").and_then(Value::as_u64).unwrap_or(20))
            .unwrap_or(usize::MAX);
        let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);

        let output_format = params
            .get("output_format")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<super::OutputFormat>().ok())
            .unwrap_or_default();

        let conn = db::open_database(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to open database: {e}")))?;

        // SQL query to search docstrings (case-insensitive)
        let mut sql = "SELECT id, kind, name, qualified_name, file_path, language, start_line, end_line, \
                       start_column, end_column, docstring, signature, visibility, is_exported, is_async, \
                       is_static, is_abstract, decorators, type_parameters, updated_at \
                       FROM nodes WHERE docstring IS NOT NULL AND LOWER(docstring) LIKE LOWER(?)".to_string();

        if kind_filter.is_some() {
            sql.push_str(" AND kind = ?");
        }

        sql.push_str(" LIMIT ?");

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| ToolError::internal_error(format!("Failed to prepare query: {e}")))?;

        let query_param = format!("%{query}%");
        let rows = if let Some(kind) = kind_filter {
            stmt.query_map(
                rusqlite::params![&query_param, db::kind_to_string(kind), limit_i64],
                db::row_to_node,
            )
        } else {
            stmt.query_map(rusqlite::params![&query_param, limit_i64], db::row_to_node)
        }
        .map_err(|e| ToolError::internal_error(format!("Query failed: {e}")))?;

        let mut results = Vec::new();
        for row in rows {
            let node =
                row.map_err(|e| ToolError::internal_error(format!("Row parsing failed: {e}")))?;
            results.push(super::serialize_node(&node, output_format));
        }

        Ok(json!({
            "results": results,
            "count": results.len(),
        }))
    }
}

/// Tool for searching exported/public symbols only (60% token savings for API exploration)
pub struct SearchExportedSymbolsTool {
    project_root: PathBuf,
}

impl SearchExportedSymbolsTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for SearchExportedSymbolsTool {
    fn name(&self) -> &'static str {
        "coraline_search_exported_symbols"
    }

    fn description(&self) -> &'static str {
        "Search only public/exported symbols (public API). Filters out internal implementation details.\n\
         \n\
         Examples:\n\
         - Find public functions: {\"query\": \"create\", \"kind\": \"function\"}\n\
         - Find public classes: {\"query\": \"User\", \"kind\": \"class\"}\n\
         - Browse public API: {\"query\": \"*\", \"limit\": 50}"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query (symbol name pattern, use '*' for all)"
                },
                "kind": {
                    "type": "string",
                    "description": "Node kind filter",
                    "enum": ["function", "method", "class", "struct", "interface", "trait", "module"]
                },
                "limit": {
                    "type": "number",
                    "description": "Maximum results to return",
                    "default": 20
                },
                "output_format": {
                    "type": "string",
                    "description": "Output format: 'full' or 'compact' (65% token reduction)",
                    "enum": ["full", "compact"],
                    "default": "full"
                }
            },
            "required": ["query"]
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let query = params
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_params("query must be a string"))?;

        let kind_filter = params
            .get("kind")
            .and_then(Value::as_str)
            .and_then(str_to_node_kind);

        let limit = usize::try_from(params.get("limit").and_then(Value::as_u64).unwrap_or(20))
            .unwrap_or(usize::MAX);
        let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);

        let output_format = params
            .get("output_format")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<super::OutputFormat>().ok())
            .unwrap_or_default();

        let conn = db::open_database(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to open database: {e}")))?;

        // Search only exported symbols
        let kind = kind_filter;
        let use_wildcard = query == "*";

        let results = if use_wildcard {
            // Get all exported symbols of the specified kind
            let mut sql = "SELECT id, kind, name, qualified_name, file_path, language, start_line, end_line, \
                           start_column, end_column, docstring, signature, visibility, is_exported, is_async, \
                           is_static, is_abstract, decorators, type_parameters, updated_at \
                           FROM nodes WHERE is_exported = 1".to_string();

            if kind.is_some() {
                sql.push_str(" AND kind = ?");
            }

            sql.push_str(" LIMIT ?");

            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| ToolError::internal_error(format!("Failed to prepare query: {e}")))?;

            let rows = if let Some(k) = kind {
                stmt.query_map(
                    rusqlite::params![db::kind_to_string(k), limit_i64],
                    db::row_to_node,
                )
            } else {
                stmt.query_map(rusqlite::params![limit_i64], db::row_to_node)
            }
            .map_err(|e| ToolError::internal_error(format!("Query failed: {e}")))?;

            let mut nodes = Vec::new();
            for row in rows {
                let node =
                    row.map_err(|e| ToolError::internal_error(format!("Row parsing failed: {e}")))?;
                nodes.push(node);
            }
            nodes
        } else {
            // Use the existing search but filter to exported only
            let all_results = db::search_nodes(&conn, query, kind, limit * 2)
                .map_err(|e| ToolError::internal_error(format!("Search failed: {e}")))?;

            all_results
                .into_iter()
                .filter(|r| r.node.is_exported)
                .take(limit)
                .map(|r| r.node)
                .collect()
        };

        let serialized: Vec<Value> = results
            .iter()
            .map(|node| super::serialize_node(node, output_format))
            .collect();

        Ok(json!({
            "results": serialized,
            "count": serialized.len(),
        }))
    }
}

/// Tool for finding all symbols of a specific kind in a file (60% token savings for file exploration)
pub struct FindByKindInFileTool {
    project_root: PathBuf,
}

impl FindByKindInFileTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for FindByKindInFileTool {
    fn name(&self) -> &'static str {
        "coraline_find_by_kind_in_file"
    }

    fn description(&self) -> &'static str {
        "Get all symbols of a specific kind in a file. Fast file-scoped exploration.\n\
         \n\
         Examples:\n\
         - All functions in a file: {\"file_path\": \"src/main.rs\", \"kind\": \"function\"}\n\
         - All classes in a file: {\"file_path\": \"lib/user.rb\", \"kind\": \"class\"}\n\
         - All methods in a file: {\"file_path\": \"api.ts\", \"kind\": \"method\"}"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file (relative to project root)"
                },
                "kind": {
                    "type": "string",
                    "description": "Node kind to filter",
                    "enum": ["function", "method", "class", "struct", "interface", "trait", "module", "constant", "variable"]
                },
                "output_format": {
                    "type": "string",
                    "description": "Output format: 'full' or 'compact' (65% token reduction)",
                    "enum": ["full", "compact"],
                    "default": "full"
                }
            },
            "required": ["file_path", "kind"]
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let file_path = params
            .get("file_path")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_params("file_path must be a string"))?;

        let kind = params
            .get("kind")
            .and_then(Value::as_str)
            .and_then(str_to_node_kind)
            .ok_or_else(|| ToolError::invalid_params("Invalid or missing kind"))?;

        let output_format = params
            .get("output_format")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<super::OutputFormat>().ok())
            .unwrap_or_default();

        let conn = db::open_database(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to open database: {e}")))?;

        // Query nodes in the file with the specified kind
        let sql = "SELECT id, kind, name, qualified_name, file_path, language, start_line, end_line, \
                   start_column, end_column, docstring, signature, visibility, is_exported, is_async, \
                   is_static, is_abstract, decorators, type_parameters, updated_at \
                   FROM nodes WHERE file_path = ? AND kind = ? ORDER BY start_line";

        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| ToolError::internal_error(format!("Failed to prepare query: {e}")))?;

        let rows = stmt
            .query_map(
                rusqlite::params![file_path, db::kind_to_string(kind)],
                db::row_to_node,
            )
            .map_err(|e| ToolError::internal_error(format!("Query failed: {e}")))?;

        let mut results = Vec::new();
        for row in rows {
            let node =
                row.map_err(|e| ToolError::internal_error(format!("Row parsing failed: {e}")))?;
            results.push(super::serialize_node(&node, output_format));
        }

        Ok(json!({
            "results": results,
            "count": results.len(),
            "file_path": file_path,
            "kind": db::kind_to_string(kind),
        }))
    }
}
