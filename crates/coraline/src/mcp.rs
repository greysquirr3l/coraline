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

use crate::tools::{ToolRegistry, create_default_registry};

const PROTOCOL_VERSION: &str = "2024-11-05";

#[derive(Default)]
pub struct McpServer {
    project_root: Option<PathBuf>,
    init_error: Option<String>,
    tool_registry: Option<ToolRegistry>,
}

#[derive(Debug, Serialize)]
struct ServerInfo {
    name: &'static str,
    version: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitializeParams {
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

#[derive(Debug, Serialize)]
struct ToolResult {
    content: Vec<ToolContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_error: Option<bool>,
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

        match method {
            "initialize" => {
                if let Some(id) = id {
                    self.handle_initialize(id, message.get("params"))?;
                }
            }
            "tools/list" => {
                if let Some(id) = id {
                    self.handle_tools_list(id)?;
                }
            }
            "tools/call" => {
                if let Some(id) = id {
                    self.handle_tools_call(id, message.get("params"))?;
                }
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
        let mut project_root = self.project_root.clone();

        if let Some(params) = params {
            if let Ok(parsed) = serde_json::from_value::<InitializeParams>(params.clone()) {
                if let Some(root_uri) = parsed.root_uri {
                    project_root = Some(PathBuf::from(strip_file_uri(&root_uri)));
                } else if let Some(folders) = parsed.workspace_folders {
                    if let Some(folder) = folders.first() {
                        project_root = Some(PathBuf::from(strip_file_uri(&folder.uri)));
                    }
                }
            }
        }

        if project_root.is_none() {
            project_root = std::env::current_dir().ok();
        }

        self.project_root = project_root.clone();
        self.initialize_codegraph();

        if let Some(root) = project_root {
            self.initialize_tools(root);
        }

        let response = serde_json::json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": ServerInfo {
                name: "coraline",
                version: env!("CARGO_PKG_VERSION"),
            }
        });

        self.send_result(id, response)
    }

    fn handle_tools_list(&mut self, id: JsonRpcId) -> io::Result<()> {
        let tools = match &self.tool_registry {
            Some(registry) => registry.get_tool_metadata(),
            None => Vec::new(),
        };
        self.send_result(id, serde_json::json!({ "tools": tools }))
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

        match registry.execute(&parsed.name, args_json) {
            Ok(result) => {
                let tool_result = ToolResult {
                    content: vec![ToolContent {
                        r#type: "text",
                        text: result.to_string(),
                    }],
                    is_error: None,
                };
                self.send_result(id, serde_json::to_value(tool_result).unwrap_or_default())
            }
            Err(err) => {
                let tool_result = ToolResult {
                    content: vec![ToolContent {
                        r#type: "text",
                        text: format!("Error: {}", err.message),
                    }],
                    is_error: Some(true),
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

fn strip_file_uri(uri: &str) -> String {
    uri.strip_prefix("file://").unwrap_or(uri).to_string()
}

fn is_initialized(project_root: &Path) -> bool {
    project_root.join(".coraline").is_dir()
}

fn send_response(response: Value) -> io::Result<()> {
    let mut stdout = io::stdout();
    writeln!(stdout, "{}", response)?;
    stdout.flush()
}
