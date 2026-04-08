#![forbid(unsafe_code)]
#![allow(
    clippy::assigning_clones,
    clippy::cast_precision_loss,
    clippy::collapsible_if,
    clippy::manual_ok_err,
    clippy::map_unwrap_or,
    clippy::missing_const_for_fn,
    clippy::needless_pass_by_ref_mut,
    clippy::needless_pass_by_value,
    clippy::option_if_let_else,
    clippy::or_fun_call,
    clippy::redundant_clone,
    clippy::significant_drop_tightening,
    clippy::uninlined_format_args,
    clippy::unused_self
)]

//! MCP (Model Context Protocol) server implementation for Coraline.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::tools::{ToolRegistry, create_default_registry};

const LATEST_PROTOCOL_VERSION: &str = "2025-11-25";
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &[LATEST_PROTOCOL_VERSION, "2024-11-05"];
const TOOLS_LIST_PAGE_SIZE: usize = 100;

#[derive(Default)]
pub struct McpServer {
    project_root: Option<PathBuf>,
    init_error: Option<String>,
    tool_registry: Option<ToolRegistry>,
    initialize_completed: bool,
    client_initialized: bool,
    negotiated_protocol_version: String,
}

#[derive(Debug, Serialize)]
struct ServerInfo {
    name: &'static str,
    version: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitializeParams {
    protocol_version: Option<String>,
    capabilities: Option<Value>,
    client_info: Option<Value>,
    root_uri: Option<String>,
    workspace_folders: Option<Vec<WorkspaceFolder>>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceFolder {
    uri: String,
}

#[derive(Debug, Deserialize)]
struct ToolCallParams {
    name: String,
    #[serde(default)]
    arguments: HashMap<String, Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolsListParams {
    cursor: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolResult {
    content: Vec<ToolContent>,
    is_error: bool,
}

#[derive(Debug, Serialize)]
struct ToolContent {
    r#type: &'static str,
    text: String,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum JsonRpcId {
    String(String),
    Number(i64),
}

fn json_rpc_id_from_value(value: &Value) -> Option<JsonRpcId> {
    match value {
        Value::String(s) => Some(JsonRpcId::String(s.clone())),
        Value::Number(n) => n.as_i64().map(JsonRpcId::Number),
        _ => None,
    }
}

impl McpServer {
    pub fn new(project_root: Option<PathBuf>) -> Self {
        let mut server = Self {
            project_root,
            init_error: None,
            tool_registry: None,
            initialize_completed: false,
            client_initialized: false,
            negotiated_protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
        };
        if let Some(ref root) = server.project_root {
            server.initialize_tools(root.clone());
        }
        server
    }

    pub fn start(&mut self) -> io::Result<()> {
        let stdin = io::stdin();
        let mut handle = stdin.lock();
        let mut line = String::new();

        loop {
            line.clear();
            let bytes = handle.read_line(&mut line)?;
            if bytes == 0 {
                break;
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            match serde_json::from_str::<Value>(trimmed) {
                Ok(message) => {
                    if let Err(err) = self.handle_message(message) {
                        self.send_error(None, -32603, &format!("Internal error: {err}"), None)?;
                    }
                }
                Err(_) => {
                    self.send_error(None, -32700, "Parse error: invalid JSON", None)?;
                }
            }
        }

        Ok(())
    }

    fn handle_message(&mut self, message: Value) -> io::Result<()> {
        let method = message.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let id = message.get("id").and_then(json_rpc_id_from_value);

        if method != "initialize" && method != "ping" && !self.initialize_completed {
            if let Some(id) = id {
                return self.send_error(
                    Some(id),
                    -32002,
                    "Server not initialized. Call initialize first.",
                    None,
                );
            }
            return Ok(());
        }

        if method != "initialize" && method != "ping" && method != "notifications/initialized" && !self.client_initialized {
            if let Some(id) = id {
                return self.send_error(
                    Some(id),
                    -32002,
                    "Client not initialized. Send notifications/initialized before normal requests.",
                    None,
                );
            }
            return Ok(());
        }

        match method {
            "initialize" => {
                if let Some(id) = id {
                    self.handle_initialize(id, message.get("params"))?;
                }
            }
            "tools/list" => {
                if let Some(id) = id {
                    self.handle_tools_list(id, message.get("params"))?;
                }
            }
            "tools/call" => {
                if let Some(id) = id {
                    self.handle_tools_call(id, message.get("params"))?;
                }
            }
            "notifications/initialized" => {
                self.client_initialized = true;
            }
            "ping" => {
                if let Some(id) = id {
                    self.send_result(id, serde_json::json!({}))?;
                }
            }
            _ => {
                if let Some(id) = id {
                    self.send_error(
                        Some(id),
                        -32601,
                        &format!("Method not found: {method}"),
                        None,
                    )?;
                }
            }
        }

        Ok(())
    }

    fn handle_initialize(&mut self, id: JsonRpcId, params: Option<&Value>) -> io::Result<()> {
        let negotiated_protocol_version;
        let mut project_root = self.project_root.clone();

        if let Some(params) = params.cloned() {
            let Ok(parsed) = serde_json::from_value::<InitializeParams>(params) else {
                return self.send_error(Some(id), -32602, "Invalid initialize params", None);
            };

            let Some(requested_version) = parsed.protocol_version.as_deref() else {
                return self.send_error(
                    Some(id),
                    -32602,
                    "Missing required initialize param: protocolVersion",
                    Some(serde_json::json!({ "supported": SUPPORTED_PROTOCOL_VERSIONS })),
                );
            };

            negotiated_protocol_version = negotiate_protocol_version(requested_version);

            if let Some(root_uri) = parsed.root_uri {
                if let Some(root_path) = parse_project_root(&root_uri) {
                    project_root = Some(root_path);
                }
            } else if let Some(folders) = parsed.workspace_folders {
                if let Some(folder) = folders.first() {
                    if let Some(root_path) = parse_project_root(&folder.uri) {
                        project_root = Some(root_path);
                    }
                }
            }

            let _client_capabilities = parsed.capabilities;
            let _client_info = parsed.client_info;
        } else {
            return self.send_error(
                Some(id),
                -32602,
                "Missing required initialize params",
                Some(serde_json::json!({ "supported": SUPPORTED_PROTOCOL_VERSIONS })),
            );
        }

        if project_root.is_none() {
            project_root = std::env::current_dir().ok();
        }

        self.project_root = project_root.clone();
        self.initialize_codegraph();

        if let Some(root) = project_root {
            self.initialize_tools(root);
        }

        self.initialize_completed = true;
        self.client_initialized = false;
        self.negotiated_protocol_version = negotiated_protocol_version;

        let response = serde_json::json!({
            "protocolVersion": self.negotiated_protocol_version,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": ServerInfo {
                name: "coraline",
                version: env!("CARGO_PKG_VERSION"),
            }
        });

        self.send_result(id, response)
    }

    fn handle_tools_list(&mut self, id: JsonRpcId, params: Option<&Value>) -> io::Result<()> {
        self.ensure_tools_initialized();

        let list_params = match params {
            Some(value) => match serde_json::from_value::<ToolsListParams>(value.clone()) {
                Ok(parsed) => parsed,
                Err(_) => {
                    return self.send_error(Some(id), -32602, "Invalid tools/list params", None);
                }
            },
            None => ToolsListParams { cursor: None },
        };

        let start_index = match parse_tools_list_cursor(list_params.cursor.as_deref()) {
            Ok(index) => index,
            Err(message) => {
                return self.send_error(Some(id), -32602, &message, None);
            }
        };

        let mut tools = match &self.tool_registry {
            Some(registry) => registry.get_tool_metadata(),
            None => Vec::new(),
        };

        tools.sort_by(|left, right| {
            let left_name = left.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let right_name = right.get("name").and_then(|v| v.as_str()).unwrap_or("");
            left_name.cmp(right_name)
        });

        if start_index > tools.len() {
            return self.send_error(Some(id), -32602, "Invalid cursor", None);
        }

        let end_index = (start_index + TOOLS_LIST_PAGE_SIZE).min(tools.len());
        let page = tools[start_index..end_index].to_vec();
        let next_cursor = (end_index < tools.len()).then(|| end_index.to_string());

        let mut result = serde_json::json!({ "tools": page });
        if let Some(cursor) = next_cursor {
            result["nextCursor"] = Value::String(cursor);
        }

        self.send_result(id, result)
    }

    fn handle_tools_call(&mut self, id: JsonRpcId, params: Option<&Value>) -> io::Result<()> {
        let Some(params) = params else {
            return self.send_error(Some(id), -32602, "Missing tool params", None);
        };

        let Ok(parsed) = serde_json::from_value::<ToolCallParams>(params.clone()) else {
            return self.send_error(Some(id), -32602, "Invalid tool params", None);
        };

        if let Some(error) = &self.init_error {
            return self.send_error(Some(id), -32603, error, None);
        }

        let Some(registry) = &self.tool_registry else {
            return self.send_error(Some(id), -32603, "Tool registry not initialized", None);
        };

        let args_json = serde_json::to_value(&parsed.arguments)
            .unwrap_or(Value::Object(serde_json::Map::new()));

        debug!(tool = %parsed.name, "dispatching tool call");
        match registry.execute(&parsed.name, args_json) {
            Ok(result) => {
                info!(tool = %parsed.name, "tool call ok");
                let tool_result = ToolResult {
                    content: vec![ToolContent {
                        r#type: "text",
                        text: result.to_string(),
                    }],
                    is_error: false,
                };
                self.send_result(id, serde_json::to_value(tool_result).unwrap_or_default())
            }
            Err(err) => {
                if err.code == "not_found" {
                    return self.send_error(
                        Some(id),
                        -32602,
                        &format!("Unknown tool: {}", parsed.name),
                        None,
                    );
                }

                warn!(tool = %parsed.name, error = %err.message, "tool call failed");
                let tool_result = ToolResult {
                    content: vec![ToolContent {
                        r#type: "text",
                        text: format!("Error: {}", err.message),
                    }],
                    is_error: true,
                };
                self.send_result(id, serde_json::to_value(tool_result).unwrap_or_default())
            }
        }
    }

    fn initialize_codegraph(&mut self) {
        let Some(project_root) = &self.project_root else {
            self.init_error = Some("No project path provided".to_string());
            return;
        };

        if !is_initialized(project_root) {
            self.init_error = Some(format!(
                "Coraline not initialized in {}. Run 'coraline init' first.",
                project_root.display()
            ));
            return;
        }

        self.init_error = None;
    }

    fn initialize_tools(&mut self, project_root: PathBuf) {
        self.tool_registry = Some(create_default_registry(&project_root));
    }

    fn ensure_tools_initialized(&mut self) {
        if self.tool_registry.is_some() {
            return;
        }

        if self.project_root.is_none() {
            self.project_root = std::env::current_dir().ok();
        }

        if let Some(project_root) = self.project_root.clone() {
            self.initialize_tools(project_root);
        }
    }

    fn send_result(&self, id: JsonRpcId, result: Value) -> io::Result<()> {
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        });
        send_response(response)
    }

    fn send_error(
        &self,
        id: Option<JsonRpcId>,
        code: i64,
        message: &str,
        data: Option<Value>,
    ) -> io::Result<()> {
        let error = match data {
            Some(data) => serde_json::json!({ "code": code, "message": message, "data": data }),
            None => serde_json::json!({ "code": code, "message": message }),
        };

        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": error,
        });
        send_response(response)
    }
}

fn parse_project_root(root: &str) -> Option<PathBuf> {
    if let Some(raw_path) = root.strip_prefix("file://") {
        #[cfg(windows)]
        {
            // VS Code provides Windows file URIs as /C:/path. Strip the leading
            // slash so PathBuf parses a drive-qualified absolute path.
            let windows_path = match raw_path.as_bytes() {
                [b'/', drive, b':', ..] if drive.is_ascii_alphabetic() => &raw_path[1..],
                _ => raw_path,
            };
            return Some(PathBuf::from(windows_path));
        }

        #[cfg(not(windows))]
        {
            return Some(PathBuf::from(raw_path));
        }
    }

    let path = Path::new(root);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }

    None
}

fn is_initialized(project_root: &Path) -> bool {
    project_root.join(".coraline").is_dir()
}

fn negotiate_protocol_version(requested: &str) -> String {
    if SUPPORTED_PROTOCOL_VERSIONS.contains(&requested) {
        return requested.to_string();
    }

    LATEST_PROTOCOL_VERSION.to_string()
}

fn parse_tools_list_cursor(cursor: Option<&str>) -> Result<usize, String> {
    match cursor {
        None => Ok(0),
        Some(raw) => raw
            .parse::<usize>()
            .map_err(|_| "Invalid cursor".to_string()),
    }
}

fn send_response(response: Value) -> io::Result<()> {
    let mut stdout = io::stdout();
    writeln!(stdout, "{}", response)?;
    stdout.flush()
}

#[cfg(test)]
mod tests {
    use super::{McpServer, ToolContent, ToolResult, parse_project_root};

    #[test]
    fn tools_are_initialized_without_explicit_path() {
        let mut server = McpServer::new(None);
        assert!(server.tool_registry.is_none());

        server.ensure_tools_initialized();
        assert!(server.tool_registry.is_some());

        let metadata_is_non_empty = server
            .tool_registry
            .as_ref()
            .map(|registry| !registry.get_tool_metadata().is_empty())
            .unwrap_or(false);
        assert!(metadata_is_non_empty);
    }

    #[test]
    fn parse_project_root_rejects_non_file_uri() {
        let root = parse_project_root("zed://workspace/foo");
        assert!(root.is_none());
    }

    #[test]
    fn parse_project_root_accepts_file_uri() {
        let uri = if cfg!(windows) {
            "file:///C:/tmp/coraline"
        } else {
            "file:///tmp/coraline"
        };

        let root = parse_project_root(uri);
        assert!(root.is_some());

        let path_is_valid = root
            .as_ref()
            .map(|path| {
                let has_expected_leaf = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name == "coraline")
                    .unwrap_or(false);

                let is_absolute_like = if cfg!(windows) {
                    path.is_absolute() || path.has_root()
                } else {
                    path.is_absolute()
                };

                is_absolute_like && has_expected_leaf
            })
            .unwrap_or(false);
        assert!(path_is_valid);
    }

    #[test]
    fn tool_result_serializes_is_error_camel_case() {
        let result = ToolResult {
            content: vec![ToolContent {
                r#type: "text",
                text: "ok".to_string(),
            }],
            is_error: true,
        };

        let json = serde_json::to_value(result).unwrap_or_default();
        assert!(json.get("isError").is_some());
        assert!(json.get("is_error").is_none());
    }

    #[test]
    fn negotiate_protocol_version_returns_requested_when_supported() {
        let version = super::negotiate_protocol_version("2024-11-05");
        assert_eq!(version, "2024-11-05");
    }

    #[test]
    fn negotiate_protocol_version_falls_back_to_latest() {
        let version = super::negotiate_protocol_version("2099-01-01");
        assert_eq!(version, super::LATEST_PROTOCOL_VERSION);
    }

    #[test]
    fn parse_tools_list_cursor_defaults_to_zero() {
        let cursor = super::parse_tools_list_cursor(None);
        assert!(matches!(cursor, Ok(0)));
    }

    #[test]
    fn parse_tools_list_cursor_rejects_non_numeric_values() {
        let cursor = super::parse_tools_list_cursor(Some("abc"));
        assert!(cursor.is_err());
    }
}
