#![forbid(unsafe_code)]

//! MCP tool for doc-accuracy auditing.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::audit;

use super::{Tool, ToolError, ToolResult};

/// MCP tool that audits documentation coverage and accuracy against the
/// indexed code graph.
///
/// Returns:
/// * **`stale_refs`** — inline backtick references in Markdown files that could
///   not be resolved to any code symbol (renamed / deleted / moved).
/// * **`undocumented_exports`** — exported functions, types, etc. with no
///   inbound `references` edge from any Markdown node.
pub struct AuditDocsTool {
    project_root: PathBuf,
}

impl AuditDocsTool {
    pub const fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl Tool for AuditDocsTool {
    fn name(&self) -> &'static str {
        "coraline_audit_docs"
    }

    fn description(&self) -> &'static str {
        "Audit documentation accuracy and coverage against the indexed code graph. \
         Returns two lists: (1) stale_refs — inline `code_span` references in \
         Markdown files that do not resolve to any known symbol, indicating \
         renamed/deleted/moved items; (2) undocumented_exports — exported public \
         symbols with no documentation reference. Requires that Markdown doc files \
         are included in the index path."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "show_undocumented": {
                    "type": "boolean",
                    "description": "Include undocumented public exports in the output (default: true).",
                    "default": true
                },
                "show_stale": {
                    "type": "boolean",
                    "description": "Include stale doc references in the output (default: true).",
                    "default": true
                },
                "limit": {
                    "type": "number",
                    "description": "Maximum number of items to return per category (default: 50).",
                    "default": 50
                }
            },
            "required": []
        })
    }

    fn execute(&self, params: Value) -> ToolResult {
        let show_undocumented = params
            .get("show_undocumented")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let show_stale = params
            .get("show_stale")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let limit = params
            .get("limit")
            .and_then(Value::as_u64)
            .map_or(50, |n| usize::try_from(n).unwrap_or(50));

        let report = audit::audit_docs(&self.project_root)
            .map_err(|e| ToolError::internal_error(e.to_string()))?;

        let stale = if show_stale {
            report
                .stale_refs
                .iter()
                .take(limit)
                .map(|r| {
                    json!({
                        "reference": r.reference_name,
                        "doc_file": r.doc_file,
                        "section": r.doc_section,
                        "line": r.line,
                        "column": r.column
                    })
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        let undoc = if show_undocumented {
            report
                .undocumented_exports
                .iter()
                .take(limit)
                .map(|u| {
                    json!({
                        "name": u.name,
                        "qualified_name": u.qualified_name,
                        "kind": u.kind,
                        "file": u.file_path,
                        "line": u.start_line
                    })
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        Ok(json!({
            "summary": {
                "doc_files_indexed": report.doc_files_indexed,
                "doc_sections_indexed": report.doc_sections_indexed,
                "stale_refs_count": report.stale_refs.len(),
                "undocumented_exports_count": report.undocumented_exports.len()
            },
            "stale_refs": stale,
            "undocumented_exports": undoc
        }))
    }
}
