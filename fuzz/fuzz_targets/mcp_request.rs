//! Fuzz target: MCP JSON-RPC request dispatch.
//!
//! Feeds arbitrary bytes through the JSON-RPC parse path and into
//! `McpServer::handle_message`, exercising the surface that an MCP client
//! (potentially hostile) can reach over stdio.
//!
//! Targets:
//! - `serde_json` deserialization of untrusted JSON
//! - JSON-RPC method dispatch (`initialize`, `tools/list`, `tools/call`, `ping`)
//! - Argument parsing (`InitializeParams`, `ToolCallParams`)
//! - Tool registry routing under crafted tool names and arguments
//! - Error response generation (must not panic)

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
    let _ = server.handle_message(message);

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
});
