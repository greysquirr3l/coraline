#![forbid(unsafe_code)]

//! MCP protocol version negotiation and per-request `_meta` parsing.
//!
//! Coraline ships as a *dual-era* server: it speaks the modern per-request
//! metadata protocol introduced in the upcoming draft spec (`2026-07-28` and
//! later) **and** the legacy handshake-based protocol (`2025-11-25` and
//! earlier, including the `2025-06-18` revision the codebase was originally
//! written against).
//!
//! See <https://modelcontextprotocol.io/specification/draft/basic/versioning>
//! for the full era model.

use serde_json::Value;

/// The modern protocol version this server implements.
///
/// Matches the version string used as the running example in the draft
/// specification.  Bump this when the spec is finalised.
pub const PROTOCOL_VERSION_MODERN: &str = "2026-07-28";

/// Legacy version published before the draft (`2025-11-25`).
pub const PROTOCOL_VERSION_LEGACY_2025_11_25: &str = "2025-11-25";

/// The legacy version the codebase was originally targeting (`2025-06-18`).
///
/// Kept in the supported list so existing clients that pinned to the old
/// revision keep working.
pub const PROTOCOL_VERSION_LEGACY_2025_06_18: &str = "2025-06-18";

/// Every protocol version this server is willing to speak, newest first.
pub const SUPPORTED_VERSIONS: &[&str] = &[
    PROTOCOL_VERSION_MODERN,
    PROTOCOL_VERSION_LEGACY_2025_11_25,
    PROTOCOL_VERSION_LEGACY_2025_06_18,
];

/// `_meta` key carrying the protocol version for a single request.
pub const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";

/// `_meta` key carrying the client identity (`{name, version}`).
pub const META_CLIENT_INFO: &str = "io.modelcontextprotocol/clientInfo";

/// `_meta` key carrying the client capabilities for this request.
pub const META_CLIENT_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";

/// `_meta` key carrying the per-request minimum log level.
pub const META_LOG_LEVEL: &str = "io.modelcontextprotocol/logLevel";

/// `_meta` key carrying MRTR `inputResponses` on a retry.
pub const META_INPUT_RESPONSES: &str = "io.modelcontextprotocol/inputResponses";

/// `_meta` key carrying MRTR opaque `requestState` on a retry.
pub const META_REQUEST_STATE: &str = "io.modelcontextprotocol/requestState";

/// The protocol era of a single request.
///
/// A dual-era server decides which shape to respond with based on the era of
/// each *individual* request — no per-connection state is consulted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Era {
    /// Modern per-request-metadata protocol (`2026-07-28` and later).
    Modern,
    /// Legacy handshake-based protocol (`2025-11-25` and earlier).
    Legacy,
}

/// JSON-RPC error codes used by MCP.
///
/// Legacy clients see the pre-draft values; modern clients see the renumbered
/// values from the `-32020`..=`-32099` range.
pub mod codes {
    // Standard JSON-RPC codes — identical across eras.
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;

    /// Resource not found — modern clients only.
    ///
    /// Draft spec renumbers this from the legacy `-32002` to invalid-params
    /// (`-32602`).  Modern clients SHOULD accept the legacy code from older
    /// servers.
    pub const RESOURCE_NOT_FOUND_MODERN: i64 = -32602;

    /// Resource not found — legacy code, kept for clients on the 2025
    /// revisions.
    pub const RESOURCE_NOT_FOUND_LEGACY: i64 = -32002;

    /// Header mismatch — modern code, from `HeaderMismatchError`.
    pub const HEADER_MISMATCH_MODERN: i64 = -32020;

    /// Missing required client capability — modern code, from
    /// `MissingRequiredClientCapabilityError`.
    pub const MISSING_REQUIRED_CLIENT_CAPABILITY: i64 = -32021;

    /// Unsupported protocol version — modern code, from
    /// `UnsupportedProtocolVersionError`.
    pub const UNSUPPORTED_PROTOCOL_VERSION_MODERN: i64 = -32022;
}

/// Pick the error code to surface for "resource not found" in the given era.
#[must_use]
pub const fn resource_not_found_code(era: Era) -> i64 {
    match era {
        Era::Modern => codes::RESOURCE_NOT_FOUND_MODERN,
        Era::Legacy => codes::RESOURCE_NOT_FOUND_LEGACY,
    }
}

/// Detect the protocol era of a single JSON-RPC message.
///
/// Modern requests carry `params._meta["io.modelcontextprotocol/protocolVersion"]`.
/// `initialize` is always legacy because legacy clients start the conversation
/// with it; `server/discover` is always modern.
#[must_use]
pub fn detect_era(message: &Value) -> Era {
    if let Some(method) = message.get("method").and_then(Value::as_str) {
        if method == "initialize" {
            return Era::Legacy;
        }
        if method == "server/discover" {
            return Era::Modern;
        }
    }

    if let Some(version) = request_protocol_version(message) {
        if !version.is_empty() {
            return Era::Modern;
        }
    }

    Era::Legacy
}

/// Extract the protocol version declared in `_meta` (modern era only).
#[must_use]
pub fn request_protocol_version(message: &Value) -> Option<&str> {
    message
        .get("params")
        .and_then(|p| p.get("_meta"))
        .and_then(|m| m.get(META_PROTOCOL_VERSION))
        .and_then(Value::as_str)
}

/// Extract the client identity declared in `_meta` (modern era only).
#[must_use]
pub fn request_client_info(message: &Value) -> Option<&Value> {
    message
        .get("params")
        .and_then(|p| p.get("_meta"))
        .and_then(|m| m.get(META_CLIENT_INFO))
}

/// Extract the client capabilities declared in `_meta` (modern era only).
#[must_use]
pub fn request_client_capabilities(message: &Value) -> Option<&Value> {
    message
        .get("params")
        .and_then(|p| p.get("_meta"))
        .and_then(|m| m.get(META_CLIENT_CAPABILITIES))
}

/// Extract the per-request log level (modern era only).
#[must_use]
pub fn request_log_level(message: &Value) -> Option<&str> {
    message
        .get("params")
        .and_then(|p| p.get("_meta"))
        .and_then(|m| m.get(META_LOG_LEVEL))
        .and_then(Value::as_str)
}

/// Extract MRTR `inputResponses` from a retry request (modern era only).
#[must_use]
pub fn request_input_responses(message: &Value) -> Option<&Value> {
    message
        .get("params")
        .and_then(|p| p.get("_meta"))
        .and_then(|m| m.get(META_INPUT_RESPONSES))
}

/// Extract MRTR `requestState` from a retry request (modern era only).
#[must_use]
pub fn request_state(message: &Value) -> Option<&str> {
    message
        .get("params")
        .and_then(|p| p.get("_meta"))
        .and_then(|m| m.get(META_REQUEST_STATE))
        .and_then(Value::as_str)
}

/// Per-request context surfaced to tools lives in [`crate::tools`] so the
/// `Tool` trait can be referenced without dragging in the `mcp` module.
///
/// The `mcp` dispatch layer constructs one of these per request from the
/// parsed JSON-RPC message and hands it to
/// [`Tool::execute_with_context`](crate::tools::Tool::execute_with_context).
///
/// Validation result for a modern `_meta` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaValidation {
    /// All required fields are present and the version is supported.
    Ok,
    /// The protocol version is not in [`SUPPORTED_VERSIONS`].
    UnsupportedVersion,
    /// One or more required `_meta` fields are missing.
    MissingFields(Vec<&'static str>),
}

/// Validate that a modern request has all required `_meta` fields and a
/// supported protocol version.
#[must_use]
pub fn validate_modern_meta(message: &Value) -> MetaValidation {
    let params_meta = message
        .get("params")
        .and_then(|p| p.get("_meta"))
        .and_then(Value::as_object);

    let Some(meta) = params_meta else {
        return MetaValidation::MissingFields(vec![
            META_PROTOCOL_VERSION,
            META_CLIENT_INFO,
            META_CLIENT_CAPABILITIES,
        ]);
    };

    let mut missing: Vec<&'static str> = Vec::new();
    if !meta
        .get(META_PROTOCOL_VERSION)
        .is_some_and(Value::is_string)
    {
        missing.push(META_PROTOCOL_VERSION);
    }
    if !meta.get(META_CLIENT_INFO).is_some_and(Value::is_object) {
        missing.push(META_CLIENT_INFO);
    }
    if !meta
        .get(META_CLIENT_CAPABILITIES)
        .is_some_and(Value::is_object)
    {
        missing.push(META_CLIENT_CAPABILITIES);
    }
    if !missing.is_empty() {
        return MetaValidation::MissingFields(missing);
    }

    let version = meta
        .get(META_PROTOCOL_VERSION)
        .and_then(Value::as_str)
        .unwrap_or("");
    if SUPPORTED_VERSIONS.contains(&version) {
        MetaValidation::Ok
    } else {
        MetaValidation::UnsupportedVersion
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detect_modern_when_meta_protocol_version_present() {
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
            "params": {
                "_meta": {
                    META_PROTOCOL_VERSION: PROTOCOL_VERSION_MODERN,
                    META_CLIENT_INFO: { "name": "x", "version": "0" },
                    META_CLIENT_CAPABILITIES: {}
                }
            }
        });
        assert_eq!(detect_era(&msg), Era::Modern);
    }

    #[test]
    fn detect_legacy_when_meta_absent() {
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
            "params": {}
        });
        assert_eq!(detect_era(&msg), Era::Legacy);
    }

    #[test]
    fn initialize_is_always_legacy() {
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "_meta": {
                    META_PROTOCOL_VERSION: PROTOCOL_VERSION_MODERN
                }
            }
        });
        assert_eq!(detect_era(&msg), Era::Legacy);
    }

    #[test]
    fn server_discover_is_always_modern() {
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "server/discover",
            "params": {}
        });
        assert_eq!(detect_era(&msg), Era::Modern);
    }

    #[test]
    fn validate_modern_meta_ok() {
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "_meta": {
                    META_PROTOCOL_VERSION: PROTOCOL_VERSION_MODERN,
                    META_CLIENT_INFO: { "name": "x", "version": "0" },
                    META_CLIENT_CAPABILITIES: {}
                }
            }
        });
        assert_eq!(validate_modern_meta(&msg), MetaValidation::Ok);
    }

    #[test]
    fn validate_modern_meta_missing_fields() {
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "_meta": {} }
        });
        #[allow(clippy::panic)] // test helper — failure is the assertion's job
        match validate_modern_meta(&msg) {
            MetaValidation::MissingFields(fields) => {
                assert!(fields.contains(&META_PROTOCOL_VERSION));
                assert!(fields.contains(&META_CLIENT_INFO));
                assert!(fields.contains(&META_CLIENT_CAPABILITIES));
            }
            other => panic!("expected MissingFields, got {other:?}"),
        }
    }

    #[test]
    fn validate_modern_meta_unsupported_version() {
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "_meta": {
                    META_PROTOCOL_VERSION: "1900-01-01",
                    META_CLIENT_INFO: { "name": "x", "version": "0" },
                    META_CLIENT_CAPABILITIES: {}
                }
            }
        });
        assert_eq!(
            validate_modern_meta(&msg),
            MetaValidation::UnsupportedVersion
        );
    }

    #[test]
    fn resource_not_found_codes_differ_by_era() {
        assert_eq!(
            resource_not_found_code(Era::Modern),
            codes::RESOURCE_NOT_FOUND_MODERN
        );
        assert_eq!(
            resource_not_found_code(Era::Legacy),
            codes::RESOURCE_NOT_FOUND_LEGACY
        );
    }

    #[test]
    fn mrtr_retry_detection() {
        use crate::tools::RequestContext;
        let mut ctx = RequestContext::default();
        assert!(!ctx.is_mrtr_retry());
        ctx.input_responses = Some(json!({}));
        assert!(ctx.is_mrtr_retry());
        ctx.input_responses = None;
        ctx.request_state = Some("opaque".into());
        assert!(ctx.is_mrtr_retry());
    }

    #[test]
    fn request_context_from_message_captures_meta() {
        use crate::tools::RequestContext;
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "_meta": {
                    META_PROTOCOL_VERSION: PROTOCOL_VERSION_MODERN,
                    META_LOG_LEVEL: "debug",
                    META_INPUT_RESPONSES: { "k": { "action": "accept" } },
                    META_REQUEST_STATE: "blob",
                    "traceparent": "00-aaaa-bbbb-01"
                }
            }
        });
        let ctx = RequestContext::from_message(&msg);
        assert_eq!(
            ctx.protocol_version.as_deref(),
            Some(PROTOCOL_VERSION_MODERN)
        );
        assert_eq!(ctx.log_level.as_deref(), Some("debug"));
        assert_eq!(ctx.request_state.as_deref(), Some("blob"));
        assert!(ctx.is_mrtr_retry());
        assert_eq!(
            ctx.meta.get("traceparent").and_then(Value::as_str),
            Some("00-aaaa-bbbb-01")
        );
    }
}
