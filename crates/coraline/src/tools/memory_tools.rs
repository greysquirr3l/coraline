#![forbid(unsafe_code)]

//! Memory tools for MCP server.
//!
//! These tools provide access to the project-specific memory system,
//! allowing persistent knowledge storage across sessions.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::memory::MemoryManager;
use crate::tools::{Tool, ToolError, ToolResult};

/// Tool for writing/updating memories.
pub struct WriteMemoryTool {
    manager: MemoryManager,
}

impl WriteMemoryTool {
    pub fn new(project_root: PathBuf) -> std::io::Result<Self> {
        Ok(Self {
            manager: MemoryManager::new(&project_root)?,
        })
    }
}

impl Tool for WriteMemoryTool {
    fn name(&self) -> &'static str {
        "codegraph_write_memory"
    }

    fn description(&self) -> &'static str {
        "Write or update a project memory. Memories persist across sessions and help maintain project context."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Memory name (without .md extension). Use descriptive names like 'project_overview', 'architecture_notes', etc."
                },
                "content": {
                    "type": "string",
                    "description": "Memory content in markdown format."
                }
            },
            "required": ["name", "content"]
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let name = params["name"]
            .as_str()
            .ok_or_else(|| ToolError::invalid_params("Missing or invalid 'name' parameter"))?;

        let content = params["content"]
            .as_str()
            .ok_or_else(|| ToolError::invalid_params("Missing or invalid 'content' parameter"))?;

        let result = self
            .manager
            .write_memory(name, content)
            .map_err(|e| ToolError::internal_error(format!("Failed to write memory: {}", e)))?;

        Ok(json!({ "message": result }))
    }
}

/// Tool for reading memories.
pub struct ReadMemoryTool {
    manager: MemoryManager,
}

impl ReadMemoryTool {
    pub fn new(project_root: PathBuf) -> std::io::Result<Self> {
        Ok(Self {
            manager: MemoryManager::new(&project_root)?,
        })
    }
}

impl Tool for ReadMemoryTool {
    fn name(&self) -> &'static str {
        "codegraph_read_memory"
    }

    fn description(&self) -> &'static str {
        "Read a project memory by name. Only use when the memory is relevant to the current task."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Memory name to read (without .md extension)."
                }
            },
            "required": ["name"]
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let name = params["name"]
            .as_str()
            .ok_or_else(|| ToolError::invalid_params("Missing or invalid 'name' parameter"))?;

        let content = self
            .manager
            .read_memory(name)
            .map_err(|e| ToolError::internal_error(format!("Failed to read memory: {}", e)))?;

        Ok(json!({ "content": content }))
    }
}

/// Tool for listing all memories.
pub struct ListMemoriesTool {
    manager: MemoryManager,
}

impl ListMemoriesTool {
    pub fn new(project_root: PathBuf) -> std::io::Result<Self> {
        Ok(Self {
            manager: MemoryManager::new(&project_root)?,
        })
    }
}

impl Tool for ListMemoriesTool {
    fn name(&self) -> &'static str {
        "codegraph_list_memories"
    }

    fn description(&self) -> &'static str {
        "List all available project memories. Use to discover what knowledge is stored."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    fn execute(&self, _params: Value) -> ToolResult {
        let memories = self
            .manager
            .list_memories()
            .map_err(|e| ToolError::internal_error(format!("Failed to list memories: {}", e)))?;

        Ok(json!({ "memories": memories }))
    }
}

/// Tool for deleting memories.
pub struct DeleteMemoryTool {
    manager: MemoryManager,
}

impl DeleteMemoryTool {
    pub fn new(project_root: PathBuf) -> std::io::Result<Self> {
        Ok(Self {
            manager: MemoryManager::new(&project_root)?,
        })
    }
}

impl Tool for DeleteMemoryTool {
    fn name(&self) -> &'static str {
        "codegraph_delete_memory"
    }

    fn description(&self) -> &'static str {
        "Delete a project memory. Only use when explicitly requested or when information is outdated."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Memory name to delete (without .md extension)."
                }
            },
            "required": ["name"]
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let name = params["name"]
            .as_str()
            .ok_or_else(|| ToolError::invalid_params("Missing or invalid 'name' parameter"))?;

        let result = self
            .manager
            .delete_memory(name)
            .map_err(|e| ToolError::internal_error(format!("Failed to delete memory: {}", e)))?;

        Ok(json!({ "message": result }))
    }
}

/// Tool for editing memories using regex replacement.
pub struct EditMemoryTool {
    manager: MemoryManager,
}

impl EditMemoryTool {
    pub fn new(project_root: PathBuf) -> std::io::Result<Self> {
        Ok(Self {
            manager: MemoryManager::new(&project_root)?,
        })
    }
}

impl Tool for EditMemoryTool {
    fn name(&self) -> &'static str {
        "codegraph_edit_memory"
    }

    fn description(&self) -> &'static str {
        "Edit a memory using pattern replacement. Supports both literal and regex patterns."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Memory name to edit (without .md extension)."
                },
                "pattern": {
                    "type": "string",
                    "description": "Pattern to search for (literal string or regex depending on mode)."
                },
                "replacement": {
                    "type": "string",
                    "description": "Replacement text."
                },
                "mode": {
                    "type": "string",
                    "enum": ["literal", "regex"],
                    "description": "Replacement mode: 'literal' for exact string match, 'regex' for regex pattern.",
                    "default": "literal"
                }
            },
            "required": ["name", "pattern", "replacement"]
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let name = params["name"]
            .as_str()
            .ok_or_else(|| ToolError::invalid_params("Missing or invalid 'name' parameter"))?;

        let pattern = params["pattern"]
            .as_str()
            .ok_or_else(|| ToolError::invalid_params("Missing or invalid 'pattern' parameter"))?;

        let replacement = params["replacement"].as_str().ok_or_else(|| {
            ToolError::invalid_params("Missing or invalid 'replacement' parameter")
        })?;

        let mode = params["mode"].as_str().unwrap_or("literal");

        // Read current content
        let content = self
            .manager
            .read_memory(name)
            .map_err(|e| ToolError::internal_error(format!("Failed to read memory: {}", e)))?;

        // Handle "not found" message
        if content.contains("not found") {
            return Err(ToolError::not_found(format!("Memory '{}' not found", name)));
        }

        // Perform replacement
        let new_content = match mode {
            "regex" => {
                let re = regex::Regex::new(pattern).map_err(|e| {
                    ToolError::invalid_params(format!("Invalid regex pattern: {}", e))
                })?;
                re.replace_all(&content, replacement).to_string()
            }
            "literal" => content.replace(pattern, replacement),
            _ => {
                return Err(ToolError::invalid_params(
                    "Mode must be 'literal' or 'regex'",
                ));
            }
        };

        // Write updated content
        let result = self
            .manager
            .write_memory(name, &new_content)
            .map_err(|e| ToolError::internal_error(format!("Failed to write memory: {}", e)))?;

        Ok(json!({ "message": result }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_write_and_read_memory_tool() {
        let temp_dir = TempDir::new().unwrap();
        let write_tool = WriteMemoryTool::new(temp_dir.path().to_path_buf()).unwrap();
        let read_tool = ReadMemoryTool::new(temp_dir.path().to_path_buf()).unwrap();

        let params = json!({
            "name": "test_memory",
            "content": "This is a test memory"
        });

        let result = write_tool.execute(params).unwrap();
        assert!(result["message"].as_str().unwrap().contains("written"));

        let params = json!({ "name": "test_memory" });
        let result = read_tool.execute(params).unwrap();
        assert_eq!(result["content"].as_str().unwrap(), "This is a test memory");
    }

    #[test]
    fn test_list_memories_tool() {
        let temp_dir = TempDir::new().unwrap();
        let write_tool = WriteMemoryTool::new(temp_dir.path().to_path_buf()).unwrap();
        let list_tool = ListMemoriesTool::new(temp_dir.path().to_path_buf()).unwrap();

        write_tool
            .execute(json!({"name": "mem1", "content": "content1"}))
            .unwrap();
        write_tool
            .execute(json!({"name": "mem2", "content": "content2"}))
            .unwrap();

        let result = list_tool.execute(json!({})).unwrap();
        let memories = result["memories"].as_array().unwrap();
        assert_eq!(memories.len(), 2);
    }

    #[test]
    fn test_delete_memory_tool() {
        let temp_dir = TempDir::new().unwrap();
        let write_tool = WriteMemoryTool::new(temp_dir.path().to_path_buf()).unwrap();
        let delete_tool = DeleteMemoryTool::new(temp_dir.path().to_path_buf()).unwrap();

        write_tool
            .execute(json!({"name": "to_delete", "content": "content"}))
            .unwrap();

        let result = delete_tool.execute(json!({"name": "to_delete"})).unwrap();
        assert!(result["message"].as_str().unwrap().contains("deleted"));
    }

    #[test]
    fn test_edit_memory_tool_literal() {
        let temp_dir = TempDir::new().unwrap();
        let write_tool = WriteMemoryTool::new(temp_dir.path().to_path_buf()).unwrap();
        let edit_tool = EditMemoryTool::new(temp_dir.path().to_path_buf()).unwrap();
        let read_tool = ReadMemoryTool::new(temp_dir.path().to_path_buf()).unwrap();

        write_tool
            .execute(json!({"name": "edit_test", "content": "Hello World"}))
            .unwrap();

        edit_tool
            .execute(json!({
                "name": "edit_test",
                "pattern": "World",
                "replacement": "Rust",
                "mode": "literal"
            }))
            .unwrap();

        let result = read_tool.execute(json!({"name": "edit_test"})).unwrap();
        assert_eq!(result["content"].as_str().unwrap(), "Hello Rust");
    }

    #[test]
    fn test_edit_memory_tool_regex() {
        let temp_dir = TempDir::new().unwrap();
        let write_tool = WriteMemoryTool::new(temp_dir.path().to_path_buf()).unwrap();
        let edit_tool = EditMemoryTool::new(temp_dir.path().to_path_buf()).unwrap();
        let read_tool = ReadMemoryTool::new(temp_dir.path().to_path_buf()).unwrap();

        write_tool
            .execute(json!({"name": "regex_test", "content": "version: 1.0.0"}))
            .unwrap();

        edit_tool
            .execute(json!({
                "name": "regex_test",
                "pattern": r"version: \d+\.\d+\.\d+",
                "replacement": "version: 2.0.0",
                "mode": "regex"
            }))
            .unwrap();

        let result = read_tool.execute(json!({"name": "regex_test"})).unwrap();
        assert_eq!(result["content"].as_str().unwrap(), "version: 2.0.0");
    }
}
