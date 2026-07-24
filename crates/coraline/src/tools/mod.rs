#![forbid(unsafe_code)]

//! Tool abstraction layer for Coraline MCP server.
//!
//! This module provides a clean separation between tool implementations and
//! the MCP protocol layer. Tools can be tested independently and reused in
//! CLI, library, and MCP contexts.

use serde_json::Value;
use std::collections::HashMap;

pub mod context_tools;
pub mod file_tools;
pub mod graph_tools;
pub mod memory_tools;

/// Output format for tool responses
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Full verbose JSON with long keys and all fields
    #[default]
    Full,
    /// Compact JSON with short keys, enum-as-int, omit nulls (65% token reduction)
    Compact,
}

impl std::str::FromStr for OutputFormat {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "compact" => Self::Compact,
            _ => Self::Full,
        })
    }
}

/// Serialize a node to compact JSON format
/// Full:    `{"id": "abc", "kind": "function", "name": "foo", "qualified_name": "mod::foo", ...}`
/// Compact: `{"i": "abc", "k": 0, "n": "foo", "q": "mod::foo", ...}`
pub fn node_to_compact_json(node: &crate::types::Node) -> Value {
    use serde_json::json;

    let mut obj = serde_json::Map::new();
    obj.insert("i".to_string(), json!(node.id));
    obj.insert("k".to_string(), json!(node_kind_to_int(node.kind)));
    obj.insert("n".to_string(), json!(node.name));

    if !node.qualified_name.is_empty() && node.qualified_name != node.name {
        obj.insert("q".to_string(), json!(node.qualified_name));
    }

    obj.insert("f".to_string(), json!(node.file_path));
    obj.insert("sl".to_string(), json!(node.start_line));

    if node.end_line > node.start_line {
        obj.insert("el".to_string(), json!(node.end_line));
    }

    obj.insert("l".to_string(), json!(language_to_string(node.language)));

    if let Some(ref sig) = node.signature {
        obj.insert("s".to_string(), json!(sig));
    }

    if let Some(ref doc) = node.docstring {
        obj.insert("d".to_string(), json!(doc));
    }

    Value::Object(obj)
}

/// Serialize a node to full JSON format (current format)
pub fn node_to_full_json(node: &crate::types::Node) -> Value {
    use serde_json::json;

    json!({
        "id": node.id,
        "kind": node.kind,
        "name": node.name,
        "qualified_name": node.qualified_name,
        "file_path": node.file_path,
        "start_line": node.start_line,
        "end_line": node.end_line,
        "language": language_to_string(node.language),
        "signature": node.signature,
        "docstring": node.docstring,
    })
}

/// Convert `Language` enum to string
const fn language_to_string(lang: crate::types::Language) -> &'static str {
    use crate::types::Language;
    match lang {
        Language::Rust => "rust",
        Language::TypeScript => "typescript",
        Language::JavaScript => "javascript",
        Language::Tsx => "tsx",
        Language::Jsx => "jsx",
        Language::Python => "python",
        Language::Go => "go",
        Language::Java => "java",
        Language::CSharp => "csharp",
        Language::Cpp => "cpp",
        Language::C => "c",
        Language::Php => "php",
        Language::Ruby => "ruby",
        Language::Swift => "swift",
        Language::Kotlin => "kotlin",
        Language::Scala => "scala",
        Language::Haskell => "haskell",
        Language::Lua => "lua",
        Language::Julia => "julia",
        Language::Matlab => "matlab",
        Language::R => "r",
        Language::Erlang => "erlang",
        Language::Elixir => "elixir",
        Language::Groovy => "groovy",
        Language::Bash => "bash",
        Language::Powershell => "powershell",
        Language::Nix => "nix",
        Language::Dart => "dart",
        Language::Fortran => "fortran",
        Language::Elm => "elm",
        Language::Perl => "perl",
        Language::Zig => "zig",
        Language::Markdown => "markdown",
        Language::Toml => "toml",
        Language::Yaml => "yaml",
        Language::Liquid => "liquid",
        Language::Blazor => "blazor",
        Language::Unknown => "unknown",
    }
}

/// Serialize a node based on output format
pub fn serialize_node(node: &crate::types::Node, format: OutputFormat) -> Value {
    match format {
        OutputFormat::Compact => node_to_compact_json(node),
        OutputFormat::Full => node_to_full_json(node),
    }
}

/// Convert `NodeKind` to integer for compact format
const fn node_kind_to_int(kind: crate::types::NodeKind) -> u8 {
    use crate::types::NodeKind;
    match kind {
        NodeKind::File => 0,
        NodeKind::Module => 1,
        NodeKind::Namespace => 2,
        NodeKind::Class => 3,
        NodeKind::Struct => 4,
        NodeKind::Interface => 5,
        NodeKind::Trait => 6,
        NodeKind::Protocol => 7,
        NodeKind::Enum => 8,
        NodeKind::EnumMember => 9,
        NodeKind::Function => 10,
        NodeKind::Method => 11,
        NodeKind::Property => 12,
        NodeKind::Field => 13,
        NodeKind::Variable => 14,
        NodeKind::Constant => 15,
        NodeKind::Parameter => 16,
        NodeKind::TypeAlias => 17,
        NodeKind::Import => 18,
        NodeKind::Export => 19,
        NodeKind::Route => 20,
        NodeKind::Component => 21,
    }
}

/// Compact JSON format legend for clients
pub fn compact_format_legend() -> Value {
    use serde_json::json;

    json!({
        "keys": {
            "i": "id",
            "k": "kind (0=file, 1=module, 2=namespace, 3=class, 4=struct, 5=interface, 6=trait, 7=protocol, 8=enum, 9=enum_member, 10=function, 11=method, 12=property, 13=field, 14=variable, 15=constant, 16=parameter, 17=type_alias, 18=import, 19=export, 20=route, 21=component)",
            "n": "name",
            "q": "qualified_name",
            "f": "file_path",
            "sl": "start_line",
            "el": "end_line",
            "l": "language",
            "s": "signature",
            "d": "docstring",
            "b": "body",
            "ln": "line (edge line number)",
            "sc": "score (search relevance)"
        },
        "note": "Fields with null/empty/default values are omitted in compact format"
    })
}

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

    /// Optional timeout hint in milliseconds for this tool
    /// Long-running operations (indexing, impact analysis) should return a hint
    fn timeout_hint(&self) -> Option<u64> {
        None
    }
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
        self.tools.get(name).map(AsRef::as_ref)
    }

    /// List all registered tool names
    pub fn list_tools(&self) -> Vec<&str> {
        self.tools.keys().map(String::as_str).collect()
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

    /// Get tool metadata with timeout hints for MCP tools/list
    pub fn get_tool_metadata_with_timeout(&self, default_timeout_ms: u64) -> Vec<Value> {
        self.tools
            .values()
            .map(|tool| {
                let mut metadata = serde_json::json!({
                    "name": tool.name(),
                    "description": tool.description(),
                    "inputSchema": tool.input_schema(),
                });

                // Add timeout hint if tool provides one, otherwise use default
                let timeout = tool.timeout_hint().unwrap_or(default_timeout_ms);
                if let Some(obj) = metadata.as_object_mut() {
                    obj.insert("timeout_ms".to_string(), serde_json::json!(timeout));
                }

                metadata
            })
            .collect()
    }

    /// Execute a tool by name
    pub fn execute(&self, name: &str, params: Value) -> ToolResult {
        self.get(name).map_or_else(
            || Err(ToolError::not_found(format!("Tool not found: {name}"))),
            |tool| tool.execute(params),
        )
    }
}

/// Create a default tool registry with all built-in tools
#[allow(clippy::too_many_lines)]
pub fn create_default_registry(project_root: &std::path::Path) -> ToolRegistry {
    let mut registry = ToolRegistry::new();

    // Register graph tools
    registry.register(Box::new(graph_tools::SearchTool::new(
        project_root.to_path_buf(),
    )));
    registry.register(Box::new(graph_tools::CallersTool::new(
        project_root.to_path_buf(),
    )));
    registry.register(Box::new(graph_tools::CalleesTool::new(
        project_root.to_path_buf(),
    )));
    registry.register(Box::new(graph_tools::ImpactTool::new(
        project_root.to_path_buf(),
    )));
    registry.register(Box::new(graph_tools::DependenciesTool::new(
        project_root.to_path_buf(),
    )));
    registry.register(Box::new(graph_tools::DependentsTool::new(
        project_root.to_path_buf(),
    )));
    registry.register(Box::new(graph_tools::PathTool::new(
        project_root.to_path_buf(),
    )));
    registry.register(Box::new(graph_tools::StatsTool::new(
        project_root.to_path_buf(),
    )));
    registry.register(Box::new(graph_tools::FindSymbolTool::new(
        project_root.to_path_buf(),
    )));
    registry.register(Box::new(graph_tools::GetSymbolsOverviewTool::new(
        project_root.to_path_buf(),
    )));
    registry.register(Box::new(graph_tools::FindReferencesTool::new(
        project_root.to_path_buf(),
    )));
    registry.register(Box::new(graph_tools::GetNodeTool::new(
        project_root.to_path_buf(),
    )));

    // Register batch query tools (60% token savings)
    registry.register(Box::new(graph_tools::BatchGetNodesTool::new(
        project_root.to_path_buf(),
    )));
    registry.register(Box::new(graph_tools::BatchCallersTool::new(
        project_root.to_path_buf(),
    )));
    registry.register(Box::new(graph_tools::BatchCalleesTool::new(
        project_root.to_path_buf(),
    )));

    // Register advanced search tools (60% token savings for specialized lookups)
    registry.register(Box::new(graph_tools::SearchBySignatureTool::new(
        project_root.to_path_buf(),
    )));
    registry.register(Box::new(graph_tools::SearchByDocstringTool::new(
        project_root.to_path_buf(),
    )));
    registry.register(Box::new(graph_tools::SearchExportedSymbolsTool::new(
        project_root.to_path_buf(),
    )));
    registry.register(Box::new(graph_tools::FindByKindInFileTool::new(
        project_root.to_path_buf(),
    )));

    // Register file tools
    registry.register(Box::new(file_tools::ReadFileTool::new(
        project_root.to_path_buf(),
    )));
    registry.register(Box::new(file_tools::ListDirTool::new(
        project_root.to_path_buf(),
    )));
    registry.register(Box::new(file_tools::GetFileNodesTool::new(
        project_root.to_path_buf(),
    )));
    registry.register(Box::new(file_tools::StatusTool::new(
        project_root.to_path_buf(),
    )));
    registry.register(Box::new(file_tools::GetConfigTool::new(
        project_root.to_path_buf(),
    )));
    registry.register(Box::new(file_tools::UpdateConfigTool::new(
        project_root.to_path_buf(),
    )));
    registry.register(Box::new(file_tools::SyncTool::new(
        project_root.to_path_buf(),
    )));

    // Register context tools
    registry.register(Box::new(context_tools::BuildContextTool::new(
        project_root.to_path_buf(),
    )));

    // Register memory tools (ignore errors if memory system fails to initialize)
    if let Ok(tool) = memory_tools::WriteMemoryTool::new(project_root) {
        registry.register(Box::new(tool));
    }
    if let Ok(tool) = memory_tools::ReadMemoryTool::new(project_root) {
        registry.register(Box::new(tool));
    }
    if let Ok(tool) = memory_tools::ListMemoriesTool::new(project_root) {
        registry.register(Box::new(tool));
    }
    if let Ok(tool) = memory_tools::DeleteMemoryTool::new(project_root) {
        registry.register(Box::new(tool));
    }
    if let Ok(tool) = memory_tools::EditMemoryTool::new(project_root) {
        registry.register(Box::new(tool));
    }

    // Register semantic search only when at least one ONNX model variant is present.
    #[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
    {
        let cfg = crate::config::load_toml_config(project_root).unwrap_or_default();
        let active_model = cfg.vectors.model.clone();
        let model_dir = cfg.vectors.model_dir.as_deref().map_or_else(
            || crate::vectors::default_model_dir_for(project_root, &active_model),
            std::path::PathBuf::from,
        );
        let order = crate::vectors::model_preference_order(&active_model)
            .unwrap_or(crate::vectors::MODEL_PREFERENCE_ORDER);
        if order.iter().any(|name| model_dir.join(name).exists()) {
            registry.register(Box::new(file_tools::SemanticSearchTool::new(
                project_root.to_path_buf(),
            )));
        } else {
            tracing::warn!(
                "Semantic search disabled: no embedding model ({active_model}) found in {}. \
                 Run `coraline model download` then `coraline embed` to enable it.",
                model_dir.display()
            );
        }
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
