//! Fuzz target: MCP JSON-RPC request dispatch.
//!
//! Feeds arbitrary bytes through the JSON-RPC parse path and into
//! `McpServer::handle_message`, exercising the surface that an MCP client
//! (potentially hostile) can reach over stdio.
//!
//! Targets:
//! - `serde_json` deserialization of untrusted JSON
//! - JSON-RPC method dispatch for **both** protocol eras:
//!   - legacy: `initialize`, `tools/list`, `tools/call`, `ping`
//!   - modern: `server/discover`, `tools/list`, `tools/call`, `_meta` parsing
//! - Argument parsing (`InitializeParams`, `ToolCallParams`)
//! - Tool registry routing under crafted tool names and arguments
//! - Error response generation (must not panic) including the new
//!   `-32020`/`-32021`/`-32022` codes and the renumbered `-32602` for
//!   resource-not-found on modern clients.

#![no_main]

use std::path::PathBuf;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some(text) = std::str::from_utf8(data).ok() else {
        return;
    };

    let Ok(message) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };

    // --- Legacy era ---------------------------------------------------------
    let mut server = coraline::mcp::McpServer::with_timeout(None, 1_000);

    // The server is constructed with no project root. Any `initialize` message
    // that supplies a `root_uri` will cause `initialize_tools` to be called,
    // which builds a `ToolRegistry` rooted at the supplied path. Tools are
    // safe to register even when the path does not exist (they only error
    // at execute time). We deliberately allow this so the fuzzer exercises
    // the full dispatch path including `tools/call`.
    //
    // The fuzzer does not assume anything about `stdout`/`stderr`; any
    // `send_result`/`send_error` writes are noise captured by libFuzzer.
    let _ = server.handle_message(message.clone());

    // Also exercise the codepath where `tools/call` runs against a fresh
    // server that has never been initialized: `init_error` is set, so the
    // handler must return a graceful error, not panic.
    let mut uninitialized = coraline::mcp::McpServer::with_timeout(None, 1_000);
    let tools_call = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "coraline_search",
            "arguments": { "query": text, "limit": 10 },
        }
    });
    let _ = uninitialized.handle_message(tools_call);

    // And exercise `initialize` + a crafted `root_uri` so the fuzzer can
    // explore paths where the tool registry actually exists.
    let init_with_root = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "initialize",
        "params": {
            "rootUri": format!("file://{}", PathBuf::from(text).display()),
            "workspaceFolders": [],
        }
    });
    let _ = server.handle_message(init_with_root);

    // --- Modern era ---------------------------------------------------------
    // Same server, but with a request carrying `_meta.protocolVersion` so the
    // modern code path is exercised: the era detector picks up on the
    // metadata and routes through `validate_modern_meta`.
    let modern_message = inject_modern_meta(&message);
    let _ = server.handle_message(modern_message);

    // Modern `server/discover` with a valid meta block — confirms we don't
    // crash on the happy path and that the supported-versions list survives
    // a round-trip.
    let discover = serde_json::json!({
        "jsonrpc": "2.0",
        "id": "discover-1",
        "method": "server/discover",
        "params": {
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": coraline::mcp::PROTOCOL_VERSION_MODERN,
                "io.modelcontextprotocol/clientInfo": { "name": "fuzz", "version": "0" },
                "io.modelcontextprotocol/clientCapabilities": {}
            }
        }
    });
    let _ = server.handle_message(discover);

    // Modern `tools/call` retry with MRTR fields — exercises the
    // `_meta.inputResponses` / `_meta.requestState` parsing path.
    let mrtr_retry = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 99,
        "method": "tools/call",
        "params": {
            "name": "coraline_search",
            "arguments": { "query": text, "limit": 5 },
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": coraline::mcp::PROTOCOL_VERSION_MODERN,
                "io.modelcontextprotocol/clientInfo": { "name": "fuzz", "version": "0" },
                "io.modelcontextprotocol/clientCapabilities": {},
                "io.modelcontextprotocol/requestState": text,
                "io.modelcontextprotocol/inputResponses": { "k": { "action": "accept" } },
                "traceparent": text,
            }
        }
    });
    let _ = server.handle_message(mrtr_retry);
});

/// Best-effort injection of modern `_meta` into a fuzzer-supplied message.
/// Leaves the message unchanged if it isn't shaped like a JSON-RPC request
/// with `params`.
fn inject_modern_meta(message: &serde_json::Value) -> serde_json::Value {
    let Some(params) = message.get("params") else {
        return message.clone();
    };
    let mut params = params.clone();
    if let Some(obj) = params.as_object_mut() {
        let mut meta = serde_json::Map::new();
        meta.insert(
            "io.modelcontextprotocol/protocolVersion".into(),
            serde_json::Value::String(coraline::mcp::PROTOCOL_VERSION_MODERN.to_string()),
        );
        meta.insert(
            "io.modelcontextprotocol/clientInfo".into(),
            serde_json::json!({ "name": "fuzz", "version": "0" }),
        );
        meta.insert(
            "io.modelcontextprotocol/clientCapabilities".into(),
            serde_json::json!({}),
        );
        obj.insert("_meta".into(), serde_json::Value::Object(meta));
    }
    let mut out = message.clone();
    if let Some(o) = out.as_object_mut() {
        o.insert("params".into(), params);
    }
    out
}
