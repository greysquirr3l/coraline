#![forbid(unsafe_code)]

//! Context building tools for creating code context for tasks

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::context;
use crate::types::{BuildContextOptions, ContextFormat};

use super::{Tool, ToolError, ToolResult};

/// Tool for building context for a task or query
pub struct BuildContextTool {
    project_root: PathBuf,
}

impl BuildContextTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for BuildContextTool {
    fn name(&self) -> &'static str {
        "codegraph_context"
    }

    fn description(&self) -> &'static str {
        "Build relevant code context for a task or issue description. Returns structured context with relevant symbols, code blocks, and file references."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "Task or issue description to build context for"
                },
                "max_nodes": {
                    "type": "number",
                    "description": "Maximum number of relevant nodes to include",
                    "default": 20
                },
                "max_code_blocks": {
                    "type": "number",
                    "description": "Maximum number of code blocks to include",
                    "default": 5
                },
                "max_code_block_size": {
                    "type": "number",
                    "description": "Maximum size of each code block in characters",
                    "default": 1500
                },
                "include_code": {
                    "type": "boolean",
                    "description": "Whether to include actual code blocks",
                    "default": true
                },
                "traversal_depth": {
                    "type": "number",
                    "description": "Depth for graph traversal from entry points",
                    "default": 1
                },
                "format": {
                    "type": "string",
                    "description": "Output format",
                    "enum": ["markdown", "json"],
                    "default": "markdown"
                }
            },
            "required": ["task"]
        })
    }

    #[allow(clippy::cast_possible_truncation)]
    fn execute(&self, params: Value) -> ToolResult {
        let task = params
            .get("task")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_params("task must be a string"))?;

        let max_nodes = params
            .get("max_nodes")
            .and_then(Value::as_u64)
            .map(|n| n as usize);
        let max_code_blocks = params
            .get("max_code_blocks")
            .and_then(Value::as_u64)
            .map(|n| n as usize);
        let max_code_block_size = params
            .get("max_code_block_size")
            .and_then(Value::as_u64)
            .map(|n| n as usize);
        let include_code = params.get("include_code").and_then(Value::as_bool);
        let traversal_depth = params
            .get("traversal_depth")
            .and_then(Value::as_u64)
            .map(|n| n as usize);

        let format = match params.get("format").and_then(Value::as_str) {
            Some("json") => Some(ContextFormat::Json),
            Some("markdown") | None => Some(ContextFormat::Markdown),
            _ => None,
        };

        let options = BuildContextOptions {
            max_nodes,
            max_code_blocks,
            max_code_block_size,
            include_code,
            traversal_depth,
            format,
            search_limit: params
                .get("search_limit")
                .and_then(Value::as_u64)
                .map(|n| n as usize),
            min_score: params
                .get("min_score")
                .and_then(Value::as_f64)
                .map(|f| f as f32),
        };

        let context = context::build_context(&self.project_root, task, &options)
            .map_err(|e| ToolError::internal_error(format!("Failed to build context: {e}")))?;

        // If format is JSON, return structured data; otherwise return as text
        match format {
            Some(ContextFormat::Json) => {
                // Parse the JSON string back to Value
                serde_json::from_str(&context).map_err(|e| {
                    ToolError::internal_error(format!("Failed to parse context JSON: {e}"))
                })
            }
            _ => {
                // Return markdown as text content
                Ok(json!({
                    "context": context,
                    "format": "markdown"
                }))
            }
        }
    }
}
