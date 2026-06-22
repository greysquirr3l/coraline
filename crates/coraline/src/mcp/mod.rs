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
//!
//! Coraline ships as a **dual-era** server:
//!
//! - **Modern** — per-request-metadata protocol from the upcoming draft
//!   (`2026-07-28` and later).  Every request carries
//!   `params._meta["io.modelcontextprotocol/protocolVersion"]`.
//! - **Legacy** — handshake-based protocol (`2025-11-25` and earlier,
//!   including the `2025-06-18` revision this codebase originally targeted).
//!   Clients open the conversation with `initialize`.
//!
//! Each request is routed to the right code path based on the presence of
//! `_meta.protocolVersion` (modern) or the `initialize` method (legacy).
//! No per-connection state is consulted.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tracing::{debug, info, warn};

use crate::tools::{RequestContext, ToolRegistry, create_default_registry};

pub mod protocol;

pub use protocol::{
    Era, MetaValidation, PROTOCOL_VERSION_LEGACY_2025_06_18, PROTOCOL_VERSION_LEGACY_2025_11_25,
    PROTOCOL_VERSION_MODERN, SUPPORTED_VERSIONS, codes, detect_era, validate_modern_meta,
};

/// Default per-tool timeout in milliseconds for modern-era `tools/list` results.
const TOOLS_LIST_TTL_MS: u64 = 300_000; // 5 minutes — tool metadata rarely changes
/// Default per-tool timeout in milliseconds for `server/discover` results.
const DISCOVER_TTL_MS: u64 = 3_600_000; // 1 hour — capabilities are stable

/// Server identity payload returned to clients during `initialize` and
/// `server/discover`.
#[derive(Debug, Serialize)]
struct ServerInfo {
    name: &'static str,
    version: &'static str,
}

/// Legacy `initialize` request params.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitializeParams {
    root_uri: Option<String>,
    workspace_folders: Option<Vec<WorkspaceFolder>>,
    protocol_version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceFolder {
    uri: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolCallParams {
    name: String,
    #[serde(default)]
    arguments: HashMap<String, Value>,
}

/// Legacy tool-call result envelope.
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

pub struct McpServer {
    project_root: Option<PathBuf>,
    init_error: Option<String>,
    tool_registry: Option<ToolRegistry>,
    timeout_ms: u64,
    /// Writer for JSON-RPC responses.  Defaults to `io::stdout()`; tests
    /// swap in a buffer via [`set_writer_for_testing`](Self::set_writer_for_testing)
    /// to capture output without polluting test logs.
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl Default for McpServer {
    fn default() -> Self {
        Self {
            project_root: None,
            init_error: None,
            tool_registry: None,
            timeout_ms: 120_000,
            writer: Arc::new(Mutex::new(Box::new(io::stdout()))),
        }
    }
}

impl McpServer {
    pub fn new(project_root: Option<PathBuf>) -> Self {
        Self::with_timeout(project_root, 120_000)
    }

    pub fn with_timeout(project_root: Option<PathBuf>, timeout_ms: u64) -> Self {
        let mut server = Self {
            project_root,
            init_error: None,
            tool_registry: None,
            timeout_ms,
            writer: Arc::new(Mutex::new(Box::new(io::stdout()))),
        };
        if let Some(ref root) = server.project_root {
            server.initialize_tools(root.clone());
        }
        server
    }

    /// Replace the response writer.  Intended for tests that want to capture
    /// JSON-RPC output without polluting stdout.
    #[doc(hidden)]
    pub fn set_writer_for_testing(&mut self, writer: Box<dyn Write + Send>) {
        self.writer = Arc::new(Mutex::new(writer));
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
                        self.send_error(
                            None,
                            protocol::Era::Legacy,
                            protocol::codes::INTERNAL_ERROR,
                            &format!("Internal error: {err}"),
                            None,
                        )?;
                    }
                }
                Err(_) => {
                    self.send_error(
                        None,
                        protocol::Era::Legacy,
                        protocol::codes::PARSE_ERROR,
                        "Parse error: invalid JSON",
                        None,
                    )?;
                }
            }
        }

        Ok(())
    }

    /// Process a single parsed MCP JSON-RPC message.
    ///
    /// Exposed publicly so external harnesses (notably the `coraline-fuzz`
    /// targets) can drive the dispatch logic without going through stdin.
    pub fn handle_message(&mut self, message: Value) -> io::Result<()> {
        let method = message.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let id = message.get("id").and_then(json_rpc_id_from_value);
        let era = detect_era(&message);

        match method {
            "initialize" => {
                if let Some(id) = id {
                    self.handle_initialize(id, message.get("params"))?;
                }
            }
            "server/discover" => {
                if let Some(id) = id {
                    self.handle_server_discover(id, &message)?;
                }
            }
            "tools/list" => {
                if let Some(id) = id {
                    self.handle_tools_list(id, &message, era)?;
                }
            }
            "tools/call" => {
                if let Some(id) = id {
                    self.handle_tools_call(id, &message, era)?;
                }
            }
            "ping" => {
                // `ping` was removed in the draft spec but legacy clients
                // still rely on it — answer for legacy requests only.
                if matches!(era, protocol::Era::Legacy) {
                    if let Some(id) = id {
                        self.send_result(id, era, Value::Object(Map::new()))?;
                    }
                } else if let Some(id) = id {
                    self.send_error(
                        Some(id),
                        era,
                        protocol::codes::METHOD_NOT_FOUND,
                        "Method not found: ping (removed in modern protocol)",
                        None,
                    )?;
                }
            }
            _ => {
                if let Some(id) = id {
                    self.send_error(
                        Some(id),
                        era,
                        protocol::codes::METHOD_NOT_FOUND,
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
        let mut client_protocol_version: Option<String> = None;

        if let Some(params) = params {
            if let Ok(parsed) = serde_json::from_value::<InitializeParams>(params.clone()) {
                if let Some(root_uri) = parsed.root_uri {
                    project_root = Some(PathBuf::from(strip_file_uri(&root_uri)));
                } else if let Some(folders) = parsed.workspace_folders {
                    if let Some(folder) = folders.first() {
                        project_root = Some(PathBuf::from(strip_file_uri(&folder.uri)));
                    }
                }
                client_protocol_version = parsed.protocol_version;
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

        // Legacy clients pin to the version they declared in `initialize`.
        // If they sent a version we recognise, echo it back; otherwise
        // advertise our most-capable legacy revision so they have a stable
        // contract.
        let protocol_version = client_protocol_version
            .as_deref()
            .filter(|v| SUPPORTED_VERSIONS.contains(v))
            .unwrap_or(PROTOCOL_VERSION_LEGACY_2025_11_25);

        let response = serde_json::json!({
            "protocolVersion": protocol_version,
            "capabilities": legacy_capabilities(),
            "serverInfo": ServerInfo {
                name: "coraline",
                version: env!("CARGO_PKG_VERSION"),
            }
        });

        self.send_result(id, protocol::Era::Legacy, response)
    }

    fn handle_server_discover(&mut self, id: JsonRpcId, message: &Value) -> io::Result<()> {
        match validate_modern_meta(message) {
            MetaValidation::Ok => {}
            MetaValidation::MissingFields(_) => {
                return self.send_error(
                    Some(id),
                    protocol::Era::Modern,
                    protocol::codes::INVALID_PARAMS,
                    "Missing required _meta fields: io.modelcontextprotocol/protocolVersion, \
                     io.modelcontextprotocol/clientInfo, io.modelcontextprotocol/clientCapabilities",
                    None,
                );
            }
            MetaValidation::UnsupportedVersion => {
                let data = serde_json::json!({
                    "supported": SUPPORTED_VERSIONS,
                    "requested": protocol::request_protocol_version(message).unwrap_or(""),
                });
                return self.send_error(
                    Some(id),
                    protocol::Era::Modern,
                    protocol::codes::UNSUPPORTED_PROTOCOL_VERSION_MODERN,
                    "Unsupported protocol version",
                    Some(data),
                );
            }
        }

        let result = serde_json::json!({
            "resultType": "complete",
            "supportedVersions": SUPPORTED_VERSIONS,
            "capabilities": modern_capabilities(),
            "serverInfo": {
                "name": "coraline",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "instructions": "Coraline exposes a code knowledge graph for the project at the working directory. Use coraline_search to find symbols, coraline_context to gather code for a task, coraline_callers/callees to trace references, and the memory tools to persist project notes.",
            "ttlMs": DISCOVER_TTL_MS,
            "cacheScope": "public",
        });

        self.send_result(id, protocol::Era::Modern, result)
    }

    fn handle_tools_list(&mut self, id: JsonRpcId, message: &Value, era: Era) -> io::Result<()> {
        if matches!(era, protocol::Era::Modern) {
            match validate_modern_meta(message) {
                MetaValidation::Ok => {}
                MetaValidation::MissingFields(_) => {
                    return self.send_error(
                        Some(id),
                        era,
                        protocol::codes::INVALID_PARAMS,
                        "Missing required _meta fields for modern tools/list",
                        None,
                    );
                }
                MetaValidation::UnsupportedVersion => {
                    let data = serde_json::json!({
                        "supported": SUPPORTED_VERSIONS,
                        "requested": protocol::request_protocol_version(message).unwrap_or(""),
                    });
                    return self.send_error(
                        Some(id),
                        era,
                        protocol::codes::UNSUPPORTED_PROTOCOL_VERSION_MODERN,
                        "Unsupported protocol version",
                        Some(data),
                    );
                }
            }
        }

        let tools = match &self.tool_registry {
            Some(registry) => registry.get_tool_metadata_with_timeout(self.timeout_ms),
            None => Vec::new(),
        };
        // Deterministic order so clients can cache by slice and so prompt
        // caches hit reliably.
        let mut tools = tools;
        tools.sort_by(|a, b| {
            let an = a.get("name").and_then(Value::as_str).unwrap_or("");
            let bn = b.get("name").and_then(Value::as_str).unwrap_or("");
            an.cmp(bn)
        });

        match era {
            protocol::Era::Modern => {
                let mut obj = Map::new();
                obj.insert("tools".into(), Value::Array(tools));
                obj.insert("ttlMs".into(), Value::Number(TOOLS_LIST_TTL_MS.into()));
                obj.insert("cacheScope".into(), Value::String("public".into()));
                self.send_result(id, era, Value::Object(obj))
            }
            protocol::Era::Legacy => {
                self.send_result(id, era, serde_json::json!({ "tools": tools }))
            }
        }
    }

    fn handle_tools_call(&mut self, id: JsonRpcId, message: &Value, era: Era) -> io::Result<()> {
        let params = message.get("params");

        if matches!(era, protocol::Era::Modern) {
            match validate_modern_meta(message) {
                MetaValidation::Ok => {}
                MetaValidation::MissingFields(_) => {
                    return self.send_error(
                        Some(id),
                        era,
                        protocol::codes::INVALID_PARAMS,
                        "Missing required _meta fields for modern tools/call",
                        None,
                    );
                }
                MetaValidation::UnsupportedVersion => {
                    let data = serde_json::json!({
                        "supported": SUPPORTED_VERSIONS,
                        "requested": protocol::request_protocol_version(message).unwrap_or(""),
                    });
                    return self.send_error(
                        Some(id),
                        era,
                        protocol::codes::UNSUPPORTED_PROTOCOL_VERSION_MODERN,
                        "Unsupported protocol version",
                        Some(data),
                    );
                }
            }
        }

        let Some(params) = params else {
            return self.send_error(
                Some(id),
                era,
                protocol::codes::INVALID_PARAMS,
                "Missing tool params",
                None,
            );
        };

        let Ok(parsed) = serde_json::from_value::<ToolCallParams>(params.clone()) else {
            return self.send_error(
                Some(id),
                era,
                protocol::codes::INVALID_PARAMS,
                "Invalid tool params",
                None,
            );
        };

        if let Some(error) = &self.init_error {
            return self.send_error(Some(id), era, protocol::codes::INTERNAL_ERROR, error, None);
        }

        let Some(registry) = &self.tool_registry else {
            return self.send_error(
                Some(id),
                era,
                protocol::codes::INTERNAL_ERROR,
                "Tool registry not initialized",
                None,
            );
        };

        let args_json = serde_json::to_value(&parsed.arguments)
            .unwrap_or(Value::Object(serde_json::Map::new()));
        let ctx = RequestContext::from_message(message);

        debug!(tool = %parsed.name, era = ?era, mrtr_retry = ctx.is_mrtr_retry(), "dispatching tool call");
        let dispatch_result = registry.get(&parsed.name).map_or_else(
            || {
                Err(crate::tools::ToolError::not_found(format!(
                    "Tool not found: {}",
                    parsed.name
                )))
            },
            |tool| tool.execute_with_context(args_json, &ctx),
        );

        match dispatch_result {
            Ok(value) => {
                info!(tool = %parsed.name, "tool call ok");
                let tool_result = ToolResult {
                    content: vec![ToolContent {
                        r#type: "text",
                        text: value.to_string(),
                    }],
                    is_error: None,
                };
                let legacy_payload = serde_json::to_value(tool_result).unwrap_or_default();
                self.send_result(id, era, legacy_payload)
            }
            Err(err) => {
                warn!(tool = %parsed.name, error = %err.message, "tool call failed");
                let tool_result = ToolResult {
                    content: vec![ToolContent {
                        r#type: "text",
                        text: format!("Error: {}", err.message),
                    }],
                    is_error: Some(true),
                };
                let legacy_payload = serde_json::to_value(tool_result).unwrap_or_default();
                self.send_result(id, era, legacy_payload)
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

    fn send_result(&self, id: JsonRpcId, era: Era, result: Value) -> io::Result<()> {
        let payload = match era {
            protocol::Era::Modern => wrap_modern_result(result),
            protocol::Era::Legacy => result,
        };
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": payload,
        });
        self.write_response(response)
    }

    fn send_error(
        &self,
        id: Option<JsonRpcId>,
        _era: Era,
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
        self.write_response(response)
    }

    fn write_response(&self, response: Value) -> io::Result<()> {
        let serialized = serde_json::to_string(&response)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut guard = self
            .writer
            .lock()
            .map_err(|e| io::Error::other(e.to_string()))?;
        writeln!(*guard, "{serialized}")?;
        guard.flush()
    }
}

/// Wrap a tool/server result with `resultType: "complete"` so it conforms to
/// the modern schema.  Object payloads get the key merged in; non-object
/// payloads are wrapped under `value` to keep `resultType` at the result
/// root.
fn wrap_modern_result(result: Value) -> Value {
    match result {
        Value::Object(mut map) => {
            // Don't clobber an existing `resultType` if a future tool opts
            // into MRTR (`input_required`).  Tools that want a different
            // `resultType` set it explicitly.
            if !map.contains_key("resultType") {
                map.insert("resultType".into(), Value::String("complete".into()));
            }
            Value::Object(map)
        }
        other => {
            let mut map = Map::new();
            map.insert("resultType".into(), Value::String("complete".into()));
            map.insert("value".into(), other);
            Value::Object(map)
        }
    }
}

/// Capabilities advertised by `initialize` (legacy clients).
fn legacy_capabilities() -> Value {
    serde_json::json!({ "tools": {} })
}

/// Capabilities advertised by `server/discover` (modern clients).
///
/// Includes the new `extensions` field per the draft spec — Coraline currently
/// supports no extensions beyond the core protocol, so the map is empty.
fn modern_capabilities() -> Value {
    serde_json::json!({
        "tools": {},
        "extensions": {},
    })
}

fn strip_file_uri(uri: &str) -> String {
    uri.strip_prefix("file://").unwrap_or(uri).to_string()
}

fn is_initialized(project_root: &Path) -> bool {
    project_root.join(".coraline").is_dir()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    fn server() -> McpServer {
        McpServer::with_timeout(None, 1_000)
    }

    /// Returns a server whose responses go to a shared `Vec<u8>` plus a
    /// clone of that buffer's `Arc` so the test can read what was written.
    fn server_with_capture() -> (McpServer, Arc<Mutex<Vec<u8>>>) {
        let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let mut s = server();
        s.set_writer_for_testing(Box::new(std::io::Cursor::new(Vec::new())));
        // Replace the cursor with one backed by our shared vec so we can
        // inspect what was written.
        let writer: Box<dyn Write + Send> = Box::new(SharedBufWriter(buf.clone()));
        s.set_writer_for_testing(writer);
        (s, buf)
    }

    struct SharedBufWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedBufWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let mut g = self.0.lock().unwrap();
            g.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn captured_string(buf: &Arc<Mutex<Vec<u8>>>) -> String {
        let g = buf.lock().unwrap();
        String::from_utf8(g.clone()).unwrap_or_default()
    }

    #[test]
    fn legacy_initialize_echoes_protocol_version_and_capabilities() {
        let (mut s, buf) = server_with_capture();
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION_LEGACY_2025_11_25,
                "rootUri": "file:///tmp/x",
            }
        });
        s.handle_message(msg).expect("handle");
        let out = captured_string(&buf);
        let parsed: Value = serde_json::from_str(out.trim()).expect("valid JSON");
        assert_eq!(parsed["id"], json!(1));
        assert_eq!(
            parsed["result"]["protocolVersion"],
            json!(PROTOCOL_VERSION_LEGACY_2025_11_25)
        );
        assert_eq!(parsed["result"]["capabilities"]["tools"], json!({}));
        // Legacy responses MUST NOT carry resultType.
        assert!(parsed["result"].get("resultType").is_none());
    }

    #[test]
    fn modern_server_discover_returns_result_type_and_cache_metadata() {
        let (mut s, buf) = server_with_capture();
        let msg = json!({
            "jsonrpc": "2.0",
            "id": "discover-1",
            "method": "server/discover",
            "params": {
                "_meta": {
                    "io.modelcontextprotocol/protocolVersion": PROTOCOL_VERSION_MODERN,
                    "io.modelcontextprotocol/clientInfo": { "name": "t", "version": "0" },
                    "io.modelcontextprotocol/clientCapabilities": {}
                }
            }
        });
        s.handle_message(msg).expect("handle");
        let out = captured_string(&buf);
        let parsed: Value = serde_json::from_str(out.trim()).expect("valid JSON");
        assert_eq!(parsed["id"], json!("discover-1"));
        assert_eq!(parsed["result"]["resultType"], json!("complete"));
        assert!(parsed["result"]["supportedVersions"].is_array());
        assert!(
            parsed["result"]["supportedVersions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == PROTOCOL_VERSION_MODERN)
        );
        assert_eq!(parsed["result"]["capabilities"]["extensions"], json!({}));
        assert_eq!(parsed["result"]["cacheScope"], json!("public"));
        assert!(parsed["result"]["ttlMs"].is_number());
    }

    #[test]
    fn modern_server_discover_rejects_unsupported_version() {
        let (mut s, buf) = server_with_capture();
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "server/discover",
            "params": {
                "_meta": {
                    "io.modelcontextprotocol/protocolVersion": "1900-01-01",
                    "io.modelcontextprotocol/clientInfo": { "name": "t", "version": "0" },
                    "io.modelcontextprotocol/clientCapabilities": {}
                }
            }
        });
        s.handle_message(msg).expect("handle");
        let out = captured_string(&buf);
        let parsed: Value = serde_json::from_str(out.trim()).expect("valid JSON");
        assert_eq!(parsed["error"]["code"], json!(-32022));
        assert!(parsed["error"]["data"]["supported"].is_array());
        assert_eq!(parsed["error"]["data"]["requested"], json!("1900-01-01"));
    }

    #[test]
    fn modern_server_discover_rejects_missing_meta() {
        let (mut s, buf) = server_with_capture();
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "server/discover",
            "params": {}
        });
        s.handle_message(msg).expect("handle");
        let parsed: Value = serde_json::from_str(captured_string(&buf).trim()).expect("valid JSON");
        assert_eq!(parsed["error"]["code"], json!(-32602));
    }

    #[test]
    fn legacy_tools_list_does_not_emit_result_type() {
        let (mut s, buf) = server_with_capture();
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        });
        s.handle_message(msg).expect("handle");
        let parsed: Value = serde_json::from_str(captured_string(&buf).trim()).expect("valid JSON");
        assert!(parsed["result"]["tools"].is_array());
        assert!(parsed["result"].get("resultType").is_none());
        assert!(parsed["result"].get("ttlMs").is_none());
    }

    #[test]
    fn modern_tools_list_emits_result_type_and_deterministic_order() {
        let (mut s, buf) = server_with_capture();
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
            "params": {
                "_meta": {
                    "io.modelcontextprotocol/protocolVersion": PROTOCOL_VERSION_MODERN,
                    "io.modelcontextprotocol/clientInfo": { "name": "t", "version": "0" },
                    "io.modelcontextprotocol/clientCapabilities": {}
                }
            }
        });
        s.handle_message(msg).expect("handle");
        let parsed: Value = serde_json::from_str(captured_string(&buf).trim()).expect("valid JSON");
        assert_eq!(parsed["result"]["resultType"], json!("complete"));
        assert_eq!(parsed["result"]["cacheScope"], json!("public"));
        assert!(parsed["result"]["ttlMs"].is_number());
        let tools = parsed["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools
            .iter()
            .map(|t| t.get("name").and_then(Value::as_str).unwrap_or(""))
            .collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(
            names, sorted,
            "tools must be sorted by name for cache stability"
        );
    }

    #[test]
    fn legacy_ping_still_works() {
        let (mut s, buf) = server_with_capture();
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "ping",
            "params": {}
        });
        s.handle_message(msg).expect("handle");
        let parsed: Value = serde_json::from_str(captured_string(&buf).trim()).expect("valid JSON");
        assert_eq!(parsed["result"], json!({}));
    }

    #[test]
    fn modern_ping_is_rejected_with_method_not_found() {
        let (mut s, buf) = server_with_capture();
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "ping",
            "params": {
                "_meta": {
                    "io.modelcontextprotocol/protocolVersion": PROTOCOL_VERSION_MODERN,
                    "io.modelcontextprotocol/clientInfo": { "name": "t", "version": "0" },
                    "io.modelcontextprotocol/clientCapabilities": {}
                }
            }
        });
        s.handle_message(msg).expect("handle");
        let parsed: Value = serde_json::from_str(captured_string(&buf).trim()).expect("valid JSON");
        assert_eq!(parsed["error"]["code"], json!(-32601));
    }

    #[test]
    fn mrtr_retry_passes_protocol_validation_with_input_responses() {
        // Without an initialized project root the tool registry is empty,
        // so the call falls through to a -32603 (internal error) — but the
        // point of this test is that the *protocol layer* doesn't reject the
        // retry because of `_meta.inputResponses`/`requestState`.  An older
        // server that ignored those fields would still emit -32603; the
        // difference is in what gets threaded to tools (covered separately).
        let (mut s, buf) = server_with_capture();
        let retry = json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/call",
            "params": {
                "name": "coraline_search",
                "arguments": { "query": "x" },
                "_meta": {
                    "io.modelcontextprotocol/protocolVersion": PROTOCOL_VERSION_MODERN,
                    "io.modelcontextprotocol/clientInfo": { "name": "t", "version": "0" },
                    "io.modelcontextprotocol/clientCapabilities": {},
                    "io.modelcontextprotocol/requestState": "opaque-blob",
                    "io.modelcontextprotocol/inputResponses": {
                        "github_login": { "action": "accept", "content": { "name": "octocat" } }
                    }
                }
            }
        });
        s.handle_message(retry).expect("handle");
        let parsed: Value = serde_json::from_str(captured_string(&buf).trim()).expect("valid JSON");
        // The protocol-level rejection (missing meta) would be -32602; the
        // server should NOT emit that for a properly-formed MRTR retry.
        assert_ne!(
            parsed["error"]["code"],
            json!(-32602),
            "MRTR retry with valid _meta must not be rejected as missing-params"
        );
        assert_eq!(
            parsed["error"]["code"],
            json!(-32603),
            "expected internal-error (no tool registry) for unrooted test server"
        );
    }

    #[test]
    fn wrap_modern_result_merges_into_object() {
        let r = wrap_modern_result(json!({ "tools": [] }));
        assert_eq!(
            r.get("resultType").and_then(Value::as_str),
            Some("complete")
        );
        assert!(r.get("tools").is_some());
    }

    #[test]
    fn wrap_modern_result_wraps_non_object() {
        let r = wrap_modern_result(json!("hello"));
        assert_eq!(
            r.get("resultType").and_then(Value::as_str),
            Some("complete")
        );
        assert_eq!(r.get("value").and_then(Value::as_str), Some("hello"));
    }

    #[test]
    fn wrap_modern_result_preserves_explicit_result_type() {
        // Lets future MRTR-capable tools emit `input_required` without being
        // clobbered to `complete`.
        let r = wrap_modern_result(json!({
            "resultType": "input_required",
            "inputRequests": {}
        }));
        assert_eq!(
            r.get("resultType").and_then(Value::as_str),
            Some("input_required")
        );
    }
}
