#![forbid(unsafe_code)]

//! Structured tool error types with optional recovery payloads.
//!
//! The plain [`ToolError`](super::ToolError) carries a `code` + `message` pair.
//! When a tool knows exactly what the agent (or user) should do next to
//! recover, it attaches a [`RecoverInfo`] via the `recover` field. The MCP
//! layer surfaces the full struct (code, message, recover) in
//! `structured_content` so the agent can branch on it without parsing
//! free-form text.

use serde::{Deserialize, Serialize};

/// Recovery hint attached to a [`ToolError`](super::ToolError) when the
/// agent can do something to recover from the failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoverInfo {
    /// Command the agent (or user) can run to recover.
    pub command: String,
    /// URL to the relevant documentation section.
    pub docs: String,
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolError;

    #[test]
    fn recover_info_serializes_with_command_and_docs() -> Result<(), serde_json::Error> {
        let info = RecoverInfo {
            command: "coraline model download".to_string(),
            docs: "https://github.com/greysquirr3l/coraline#embeddings".to_string(),
        };
        let json = serde_json::to_string(&info)?;
        assert!(json.contains("\"command\":\"coraline model download\""));
        assert!(json.contains("\"docs\":\"https://github.com/greysquirr3l/coraline#embeddings\""));
        Ok(())
    }

    #[test]
    fn embedding_model_missing_creates_recoverable_error() -> Result<(), &'static str> {
        let err = ToolError::embedding_model_missing(RecoverInfo {
            command: "coraline model download".to_string(),
            docs: "https://github.com/greysquirr3l/coraline#embeddings".to_string(),
        });
        assert_eq!(err.code, "EMBEDDING_MODEL_MISSING");
        let recover = err.recover.ok_or("expected recover to be present")?;
        assert_eq!(recover.command, "coraline model download");
        assert_eq!(
            recover.docs,
            "https://github.com/greysquirr3l/coraline#embeddings"
        );
        Ok(())
    }

    #[test]
    fn tool_error_omits_recover_when_none() -> Result<(), serde_json::Error> {
        let err = ToolError::internal_error("oops");
        let json = serde_json::to_string(&err)?;
        assert!(!json.contains("recover"));
        Ok(())
    }

    #[test]
    fn tool_error_includes_recover_when_present() -> Result<(), serde_json::Error> {
        let err = ToolError::embedding_model_missing(RecoverInfo {
            command: "coraline model download".to_string(),
            docs: "https://example.com".to_string(),
        });
        let json = serde_json::to_string(&err)?;
        assert!(json.contains("\"code\":\"EMBEDDING_MODEL_MISSING\""));
        assert!(json.contains("\"recover\":"));
        assert!(json.contains("\"command\":\"coraline model download\""));
        Ok(())
    }
}
