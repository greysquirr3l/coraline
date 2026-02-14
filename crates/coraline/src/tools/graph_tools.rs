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
        "codegraph_search"
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
                }
            },
            "required": ["query"]
        })
    }

    #[allow(clippy::cast_possible_truncation)]
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

        let limit = params.get("limit").and_then(Value::as_u64).unwrap_or(10) as usize;

        let conn = db::open_database(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to open database: {e}")))?;

        let results = db::search_nodes(&conn, query, kind, limit)
            .map_err(|e| ToolError::internal_error(format!("Search failed: {e}")))?;

        let results_json: Vec<Value> = results
            .into_iter()
            .map(|r| {
                json!({
                    "node": {
                        "id": r.node.id,
                        "kind": r.node.kind,
                        "name": r.node.name,
                        "qualified_name": r.node.qualified_name,
                        "file_path": r.node.file_path,
                        "start_line": r.node.start_line,
                        "end_line": r.node.end_line,
                        "language": r.node.language,
                        "signature": r.node.signature,
                    },
                    "score": r.score,
                })
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
        "codegraph_callers"
    }

    fn description(&self) -> &'static str {
        "Find all functions/methods that call a given symbol"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "node_id": {
                    "type": "string",
                    "description": "ID of the node to find callers for"
                },
                "limit": {
                    "type": "number",
                    "description": "Maximum number of callers to return",
                    "default": 20
                }
            },
            "required": ["node_id"]
        })
    }

    #[allow(clippy::cast_possible_truncation)]
    fn execute(&self, params: Value) -> ToolResult {
        let node_id = params
            .get("node_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_params("node_id must be a string"))?;

        let limit = params.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize;

        let conn = db::open_database(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to open database: {e}")))?;

        // Get edges where this node is the target and edge kind is "calls"
        let edges = db::get_edges_by_target(&conn, node_id, Some(EdgeKind::Calls), limit)
            .map_err(|e| ToolError::internal_error(format!("Failed to get edges: {e}")))?;

        let mut callers = Vec::new();
        for edge in edges {
            if let Some(caller) = db::get_node_by_id(&conn, &edge.source)
                .map_err(|e| ToolError::internal_error(format!("Failed to get node: {e}")))?
            {
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
        "codegraph_callees"
    }

    fn description(&self) -> &'static str {
        "Find all functions/methods that a given symbol calls"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "node_id": {
                    "type": "string",
                    "description": "ID of the node to find callees for"
                },
                "limit": {
                    "type": "number",
                    "description": "Maximum number of callees to return",
                    "default": 20
                }
            },
            "required": ["node_id"]
        })
    }

    #[allow(clippy::cast_possible_truncation)]
    fn execute(&self, params: Value) -> ToolResult {
        let node_id = params
            .get("node_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_params("node_id must be a string"))?;

        let limit = params.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize;

        let conn = db::open_database(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to open database: {e}")))?;

        // Get edges where this node is the source and edge kind is "calls"
        let edges = db::get_edges_by_source(&conn, node_id, Some(EdgeKind::Calls), limit)
            .map_err(|e| ToolError::internal_error(format!("Failed to get edges: {e}")))?;

        let mut callees = Vec::new();
        for edge in edges {
            if let Some(callee) = db::get_node_by_id(&conn, &edge.target)
                .map_err(|e| ToolError::internal_error(format!("Failed to get node: {e}")))?
            {
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
        "codegraph_impact"
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

    #[allow(clippy::cast_possible_truncation)]
    fn execute(&self, params: Value) -> ToolResult {
        let node_id = params
            .get("node_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_params("node_id must be a string"))?;

        let max_depth = params
            .get("max_depth")
            .and_then(Value::as_u64)
            .map(|n| n as usize);
        let max_nodes = params
            .get("max_nodes")
            .and_then(Value::as_u64)
            .map(|n| n as usize);

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
}
