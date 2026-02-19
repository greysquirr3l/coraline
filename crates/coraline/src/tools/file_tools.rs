#![forbid(unsafe_code)]

//! File system tools for reading files and listing directory contents.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::db;

use super::{Tool, ToolError, ToolResult};

/// Tool for reading file contents with optional line range
pub struct ReadFileTool {
    project_root: PathBuf,
}

impl ReadFileTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for ReadFileTool {
    fn name(&self) -> &'static str {
        "coraline_read_file"
    }

    fn description(&self) -> &'static str {
        "Read the contents of a file, optionally limited to a line range."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file (relative to project root or absolute)"
                },
                "start_line": {
                    "type": "number",
                    "description": "First line to read (1-indexed, inclusive). Defaults to 1."
                },
                "limit": {
                    "type": "number",
                    "description": "Maximum number of lines to return. Defaults to 200."
                }
            },
            "required": ["path"]
        })
    }

    #[allow(clippy::cast_possible_truncation)]
    fn execute(&self, params: Value) -> ToolResult {
        let raw_path = params
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_params("path must be a string"))?;

        let path = resolve_path(&self.project_root, raw_path);

        let start_line = params
            .get("start_line")
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .unwrap_or(1)
            .max(1);

        let limit = params
            .get("limit")
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .unwrap_or(200);

        let text = std::fs::read_to_string(&path)
            .map_err(|e| ToolError::not_found(format!("Cannot read file {:?}: {e}", path)))?;

        let all_lines: Vec<&str> = text.lines().collect();
        let total_lines = all_lines.len();

        let start_idx = start_line.saturating_sub(1).min(total_lines);
        let end_idx = (start_idx + limit).min(total_lines);

        let content = all_lines[start_idx..end_idx].join("\n");

        Ok(json!({
            "path": path,
            "content": content,
            "start_line": start_line,
            "end_line": start_idx + (end_idx - start_idx),
            "total_lines": total_lines,
            "truncated": end_idx < total_lines,
        }))
    }
}

/// Tool for listing directory contents
pub struct ListDirTool {
    project_root: PathBuf,
}

impl ListDirTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for ListDirTool {
    fn name(&self) -> &'static str {
        "coraline_list_dir"
    }

    fn description(&self) -> &'static str {
        "List the contents of a directory within the project."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path (relative to project root or absolute). Defaults to project root."
                }
            }
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let raw_path = params.get("path").and_then(Value::as_str).unwrap_or(".");

        let dir = resolve_path(&self.project_root, raw_path);

        let entries = std::fs::read_dir(&dir)
            .map_err(|e| ToolError::not_found(format!("Cannot read directory {:?}: {e}", dir)))?;

        let mut items = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // Skip hidden files/dirs and common noise dirs
            if name.starts_with('.') {
                continue;
            }
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let display = if is_dir {
                format!("{name}/")
            } else {
                name.clone()
            };
            items.push(json!({
                "name": display,
                "is_dir": is_dir,
            }));
        }

        items.sort_by(|a, b| {
            let a_dir = a["is_dir"].as_bool().unwrap_or(false);
            let b_dir = b["is_dir"].as_bool().unwrap_or(false);
            // Directories first, then alphabetical
            b_dir
                .cmp(&a_dir)
                .then_with(|| a["name"].as_str().cmp(&b["name"].as_str()))
        });

        Ok(json!({
            "path": dir,
            "entries": items,
            "count": items.len(),
        }))
    }
}

/// Tool for getting all indexed nodes in a file
pub struct GetFileNodesTool {
    project_root: PathBuf,
}

impl GetFileNodesTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for GetFileNodesTool {
    fn name(&self) -> &'static str {
        "coraline_get_file_nodes"
    }

    fn description(&self) -> &'static str {
        "Get all indexed code symbols (nodes) in a specific file, ordered by line number."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file (relative to project root or absolute)"
                },
                "kind": {
                    "type": "string",
                    "description": "Optional node kind filter",
                    "enum": ["function", "method", "class", "struct", "interface", "trait", "module"]
                }
            },
            "required": ["file_path"]
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let raw_path = params
            .get("file_path")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_params("file_path must be a string"))?;

        let kind = params
            .get("kind")
            .and_then(Value::as_str)
            .and_then(str_to_node_kind);

        let abs_path = resolve_path(&self.project_root, raw_path)
            .to_string_lossy()
            .to_string();

        let conn = db::open_database(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to open database: {e}")))?;

        // Try absolute path first, fall back to raw_path (in case stored relative)
        let nodes = {
            let mut n = db::get_nodes_by_file(&conn, &abs_path, kind.clone())
                .map_err(|e| ToolError::internal_error(format!("Failed to query nodes: {e}")))?;
            if n.is_empty() {
                n = db::get_nodes_by_file(&conn, raw_path, kind).map_err(|e| {
                    ToolError::internal_error(format!("Failed to query nodes: {e}"))
                })?;
            }
            n
        };

        let symbols: Vec<Value> = nodes
            .iter()
            .map(|n| {
                json!({
                    "id": n.id,
                    "kind": n.kind,
                    "name": n.name,
                    "qualified_name": n.qualified_name,
                    "start_line": n.start_line,
                    "end_line": n.end_line,
                    "signature": n.signature,
                    "is_exported": n.is_exported,
                })
            })
            .collect();

        Ok(json!({
            "file_path": abs_path,
            "nodes": symbols,
            "count": symbols.len(),
        }))
    }
}

/// Tool for project index status and statistics
pub struct StatusTool {
    project_root: PathBuf,
}

impl StatusTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for StatusTool {
    fn name(&self) -> &'static str {
        "coraline_status"
    }

    fn description(&self) -> &'static str {
        "Get the current index status and statistics for the project."
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

        let stats = db::get_db_stats(&conn)
            .map_err(|e| ToolError::internal_error(format!("Failed to get stats: {e}")))?;

        let db_path = db::database_path(&self.project_root);
        let db_size = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);

        Ok(json!({
            "project_root": self.project_root,
            "database": db_path,
            "database_size_bytes": db_size,
            "stats": {
                "nodes": stats.node_count,
                "edges": stats.edge_count,
                "files": stats.file_count,
                "unresolved_references": stats.unresolved_count,
            }
        }))
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn resolve_path(project_root: &std::path::Path, raw: &str) -> PathBuf {
    let p = std::path::Path::new(raw);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        project_root.join(raw)
    }
}

fn str_to_node_kind(s: &str) -> Option<crate::types::NodeKind> {
    use crate::types::NodeKind;
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

/// Tool for reading the current project configuration
pub struct GetConfigTool {
    project_root: PathBuf,
}

impl GetConfigTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for GetConfigTool {
    fn name(&self) -> &'static str {
        "coraline_get_config"
    }

    fn description(&self) -> &'static str {
        "Read the current Coraline project configuration (config.toml). Returns all sections with their effective values."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "section": {
                    "type": "string",
                    "description": "Optional: return only this section (indexing, context, sync, vectors)",
                    "enum": ["indexing", "context", "sync", "vectors"]
                }
            }
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let cfg = crate::config::load_toml_config(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to load config: {e}")))?;

        let full = serde_json::to_value(&cfg)
            .map_err(|e| ToolError::internal_error(format!("Serialization failed: {e}")))?;

        let result = if let Some(section) = params.get("section").and_then(Value::as_str) {
            full.get(section).cloned().unwrap_or(Value::Null)
        } else {
            full
        };

        let config_path = crate::config::toml_config_path(&self.project_root);
        Ok(json!({
            "config_path": config_path,
            "config_exists": config_path.exists(),
            "config": result,
        }))
    }
}

/// Tool for updating a single config value
pub struct UpdateConfigTool {
    project_root: PathBuf,
}

impl UpdateConfigTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for UpdateConfigTool {
    fn name(&self) -> &'static str {
        "coraline_update_config"
    }

    fn description(&self) -> &'static str {
        "Update a single value in the Coraline config.toml. Specify the section and key to update."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "section": {
                    "type": "string",
                    "description": "Config section to update",
                    "enum": ["indexing", "context", "sync", "vectors"]
                },
                "key": {
                    "type": "string",
                    "description": "The config key within the section"
                },
                "value": {
                    "description": "New value (must match the expected type for that key)"
                }
            },
            "required": ["section", "key", "value"]
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let section = params
            .get("section")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_params("section must be a string"))?;

        let key = params
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_params("key must be a string"))?;

        let new_value = params
            .get("value")
            .ok_or_else(|| ToolError::invalid_params("value is required"))?
            .clone();

        // Load current config, mutate it as JSON, write back
        let cfg = crate::config::load_toml_config(&self.project_root)
            .map_err(|e| ToolError::internal_error(format!("Failed to load config: {e}")))?;

        let mut cfg_json = serde_json::to_value(&cfg)
            .map_err(|e| ToolError::internal_error(format!("Serialization failed: {e}")))?;

        let section_obj = cfg_json
            .get_mut(section)
            .ok_or_else(|| ToolError::invalid_params(format!("Unknown section: {section}")))?;

        let obj = section_obj
            .as_object_mut()
            .ok_or_else(|| ToolError::internal_error("Section is not an object"))?;

        if !obj.contains_key(key) {
            return Err(ToolError::invalid_params(format!(
                "Unknown key '{key}' in section '{section}'"
            )));
        }

        obj.insert(key.to_string(), new_value.clone());

        let updated: crate::config::CoralineConfig = serde_json::from_value(cfg_json)
            .map_err(|e| ToolError::invalid_params(format!("Invalid value for {section}.{key}: {e}")))?;

        crate::config::save_toml_config(&self.project_root, &updated)
            .map_err(|e| ToolError::internal_error(format!("Failed to save config: {e}")))?;

        Ok(json!({
            "updated": true,
            "section": section,
            "key": key,
            "new_value": new_value,
        }))
    }
}
