#![forbid(unsafe_code)]

//! Tool abstraction layer for Coraline MCP server.
//!
//! This module provides a clean separation between tool implementations and
//! the MCP protocol layer. Tools can be tested independently and reused in
//! CLI, library, and MCP contexts.

use serde_json::Value;
use std::collections::HashMap;

pub mod context_tools;
pub mod graph_tools;
pub mod memory_tools;

/// Result type for tool execution
pub type ToolResult = Result<Value, ToolError>;

/// Error type for tool execution failures
#[derive(Debug, Clone)]
pub struct ToolError {
    pub code: String,
    pub message: String,
}

impl ToolError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new("invalid_params", message)
    }

    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new("internal_error", message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("not_found", message)
    }
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ToolError {}

/// Trait for MCP tools
pub trait Tool: Send + Sync {
    /// Tool name (used in MCP protocol)
    fn name(&self) -> &'static str;

    /// Human-readable description
    fn description(&self) -> &'static str;

    /// JSON schema for input parameters
    fn input_schema(&self) -> Value;

    /// Execute the tool with given parameters
    fn execute(&self, params: Value) -> ToolResult;
}

/// Registry for managing available tools
#[derive(Default)]
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tool
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.name().to_string();
        self.tools.insert(name, tool);
    }

    /// Get a tool by name
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    /// List all registered tool names
    pub fn list_tools(&self) -> Vec<&str> {
        self.tools.keys().map(|s| s.as_str()).collect()
    }

    /// Get tool metadata for MCP tools/list
    pub fn get_tool_metadata(&self) -> Vec<Value> {
        self.tools
            .values()
            .map(|tool| {
                serde_json::json!({
                    "name": tool.name(),
                    "description": tool.description(),
                    "inputSchema": tool.input_schema(),
                })
            })
            .collect()
    }

    /// Execute a tool by name
    pub fn execute(&self, name: &str, params: Value) -> ToolResult {
        match self.get(name) {
            Some(tool) => tool.execute(params),
            None => Err(ToolError::not_found(format!("Tool not found: {}", name))),
        }
    }
}

/// Create a default tool registry with all built-in tools
pub fn create_default_registry(project_root: std::path::PathBuf) -> ToolRegistry {
    let mut registry = ToolRegistry::new();

    // Register graph tools
    registry.register(Box::new(graph_tools::SearchTool::new(project_root.clone())));
    registry.register(Box::new(graph_tools::CallersTool::new(
        project_root.clone(),
    )));
    registry.register(Box::new(graph_tools::CalleesTool::new(
        project_root.clone(),
    )));
    registry.register(Box::new(graph_tools::ImpactTool::new(project_root.clone())));

    // Register context tools
    registry.register(Box::new(context_tools::BuildContextTool::new(
        project_root.clone(),
    )));

    // Register memory tools (ignore errors if memory system fails to initialize)
    if let Ok(tool) = memory_tools::WriteMemoryTool::new(project_root.clone()) {
        registry.register(Box::new(tool));
    }
    if let Ok(tool) = memory_tools::ReadMemoryTool::new(project_root.clone()) {
        registry.register(Box::new(tool));
    }
    if let Ok(tool) = memory_tools::ListMemoriesTool::new(project_root.clone()) {
        registry.register(Box::new(tool));
    }
    if let Ok(tool) = memory_tools::DeleteMemoryTool::new(project_root.clone()) {
        registry.register(Box::new(tool));
    }
    if let Ok(tool) = memory_tools::EditMemoryTool::new(project_root.clone()) {
        registry.register(Box::new(tool));
    }

    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockTool;

    impl Tool for MockTool {
        fn name(&self) -> &'static str {
            "mock_tool"
        }

        fn description(&self) -> &'static str {
            "A mock tool for testing"
        }

        fn input_schema(&self) -> Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "value": { "type": "string" }
                }
            })
        }

        fn execute(&self, params: Value) -> ToolResult {
            Ok(serde_json::json!({ "result": params }))
        }
    }

    #[test]
    fn test_registry_register_and_get() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(MockTool));

        assert!(registry.get("mock_tool").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_registry_execute() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(MockTool));

        let result = registry.execute("mock_tool", serde_json::json!({ "value": "test" }));
        assert!(result.is_ok());
    }

    #[test]
    fn test_registry_execute_not_found() {
        let registry = ToolRegistry::new();
        let result = registry.execute("nonexistent", serde_json::json!({}));
        assert!(result.is_err());
    }
}
