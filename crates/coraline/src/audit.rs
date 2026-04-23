#![forbid(unsafe_code)]

//! Documentation accuracy auditing.
//!
//! After indexing + resolution, this module queries the knowledge graph for
//! two categories of problems:
//!
//! 1. **Stale references** — inline `` `code_span` `` mentions in Markdown
//!    files that could not be resolved to any symbol in the code graph.
//!    These indicate docs that reference renamed, deleted, or moved symbols.
//!
//! 2. **Undocumented public API** — exported functions, types, structs, etc.
//!    that have no inbound `references` edge from any Markdown node.

use std::path::Path;

use crate::db;

// ─── Report types ─────────────────────────────────────────────────────────────

/// A backtick reference in a doc file that could not be resolved to a code
/// symbol — the symbol may have been renamed, moved, or deleted.
#[derive(Debug, Clone)]
pub struct StaleDocRef {
    /// The name written inside the backticks, e.g. `"my_function"`.
    pub reference_name: String,
    /// Relative path of the Markdown file, e.g. `"docs/book/src/api.md"`.
    pub doc_file: String,
    /// The heading section containing the reference, or the file name if the
    /// reference appears before any heading.
    pub doc_section: String,
    /// 1-based line number inside the Markdown file.
    pub line: i64,
    /// 0-based column.
    pub column: i64,
}

/// An exported code symbol that is not mentioned in any documentation.
#[derive(Debug, Clone)]
pub struct UndocumentedExport {
    /// The symbol's short name, e.g. `"MyStruct"`.
    pub name: String,
    /// Fully-qualified name including file path, e.g. `"src/lib.rs::MyStruct"`.
    pub qualified_name: String,
    /// Node kind as a lowercase string, e.g. `"function"`, `"struct"`.
    pub kind: String,
    /// Relative source file path.
    pub file_path: String,
    /// 1-based line number of the symbol definition.
    pub start_line: i64,
}

/// The full output of a documentation audit run.
#[derive(Debug, Default)]
pub struct DocAuditReport {
    /// References in docs that no longer point to a known symbol.
    pub stale_refs: Vec<StaleDocRef>,
    /// Exported symbols with no documentation coverage.
    pub undocumented_exports: Vec<UndocumentedExport>,
    /// Number of distinct Markdown files that have been indexed with headings.
    pub doc_files_indexed: usize,
    /// Total number of heading sections across all indexed Markdown files.
    pub doc_sections_indexed: usize,
}

// ─── Core audit logic ─────────────────────────────────────────────────────────

/// Run a documentation audit against the indexed knowledge graph at
/// `project_root` and return the findings.
///
/// # Errors
///
/// Returns an `io::Error` if the database cannot be opened or queried.
pub fn audit_docs(project_root: &Path) -> std::io::Result<DocAuditReport> {
    let conn = db::open_database(project_root)?;

    let raw_stale = db::list_doc_unresolved_refs(&conn)?;
    let raw_undoc = db::list_undocumented_exports(&conn)?;
    let (doc_files_indexed, doc_sections_indexed) = db::get_doc_coverage_stats(&conn)?;

    let stale_refs = raw_stale
        .into_iter()
        .map(|r| StaleDocRef {
            reference_name: r.reference_name,
            doc_file: r.doc_file_path,
            doc_section: r.doc_section_name,
            line: r.line,
            column: r.column,
        })
        .collect();

    let undocumented_exports = raw_undoc
        .into_iter()
        .map(|n| UndocumentedExport {
            name: n.name,
            qualified_name: n.qualified_name,
            kind: format!("{:?}", n.kind).to_ascii_lowercase(),
            file_path: n.file_path,
            start_line: n.start_line,
        })
        .collect();

    Ok(DocAuditReport {
        stale_refs,
        undocumented_exports,
        doc_files_indexed,
        doc_sections_indexed,
    })
}
