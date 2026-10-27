#![deny(unsafe_code)]
#![allow(
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::collapsible_if,
    clippy::equatable_if_let,
    clippy::indexing_slicing,
    clippy::manual_let_else,
    clippy::match_same_arms,
    clippy::missing_const_for_fn,
    clippy::needless_pass_by_value,
    clippy::option_if_let_else,
    clippy::redundant_clone,
    clippy::redundant_closure_for_method_calls,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::uninlined_format_args,
    clippy::used_underscore_binding
)]

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rayon::prelude::*;
use tree_sitter::{Node as TsNode, Parser};

use crate::config::is_language_supported;
use crate::db;
use crate::grammar;
use crate::resolution::ReferenceResolver;
use crate::types::{
    CodeGraphConfig, Edge, EdgeKind, ExtractionError, ExtractionErrorSeverity, FileRecord,
    Language, Node, NodeKind, UnresolvedReference, Visibility,
};
use crate::utils::{hash_sha256, node_id_for_symbol};
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Copy)]
pub enum IndexPhase {
    Scanning,
    Parsing,
    Storing,
    Resolving,
}

#[derive(Debug, Clone)]
pub struct IndexProgress {
    pub phase: IndexPhase,
    pub current: usize,
    pub total: usize,
    pub current_file: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IndexResult {
    pub success: bool,
    pub files_indexed: usize,
    pub files_skipped: usize,
    pub nodes_created: usize,
    pub edges_created: usize,
    pub errors: Vec<ExtractionError>,
    pub duration_ms: u128,
}

#[derive(Debug, Clone)]
pub struct SyncResult {
    pub files_checked: usize,
    pub files_added: usize,
    pub files_modified: usize,
    pub files_removed: usize,
    pub nodes_updated: usize,
    pub duration_ms: u128,
    /// Problems encountered while syncing.
    ///
    /// A sync that fails to resolve its references still produced a
    /// usable index, so the failure is reported rather than raised: the
    /// caller decides whether it matters.
    pub errors: Vec<ExtractionError>,
}

struct ParsedFile {
    file_record: FileRecord,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    unresolved_refs: Vec<UnresolvedReference>,
    node_count: usize,
    edge_count: usize,
}

fn parse_file_only(
    project_root: &Path,
    config: &CodeGraphConfig,
    existing_hashes: &std::collections::HashMap<String, String>,
    relative_path: &str,
) -> Option<ParsedFile> {
    let full_path = project_root.join(relative_path);
    let content = fs::read_to_string(&full_path).ok()?;

    if (content.len() as u64) > config.max_file_size {
        return None;
    }

    let language = detect_language(relative_path);
    if !is_language_supported(&language) {
        return None;
    }

    let content_hash = hash_sha256(&content);
    if existing_hashes
        .get(relative_path)
        .is_some_and(|h| *h == content_hash)
    {
        return None; // unchanged
    }

    let file_name = Path::new(relative_path)
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or(relative_path);
    let qualified_name = relative_path.to_string();
    let node_id = node_id_for_symbol(relative_path, "file", &qualified_name, 1, 0);
    let file_node_id = node_id.clone();

    let now_ms = now_millis();
    let mut nodes = Vec::new();
    let file_node = Node {
        id: node_id,
        kind: NodeKind::File,
        name: file_name.to_string(),
        qualified_name,
        file_path: relative_path.to_string(),
        language,
        start_line: 1,
        end_line: 1,
        start_column: 0,
        end_column: 0,
        docstring: None,
        signature: None,
        visibility: None,
        is_exported: false,
        is_async: false,
        is_static: false,
        is_abstract: false,
        decorators: None,
        type_parameters: None,
        cluster_id: None,
        updated_at: now_ms,
    };
    nodes.push(file_node);

    let (mut extracted_nodes, edges, unresolved_refs) = extract_nodes(
        project_root,
        relative_path,
        &content,
        language,
        now_ms,
        &file_node_id,
    );
    nodes.append(&mut extracted_nodes);

    let metadata = fs::metadata(&full_path).ok()?;
    let file_record = FileRecord {
        path: relative_path.to_string(),
        content_hash,
        language,
        size: metadata.len(),
        modified_at: metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX)),
        indexed_at: now_ms,
        node_count: nodes.len() as i64,
        errors: None,
    };

    let node_count = nodes.len();
    let edge_count = edges.len();
    Some(ParsedFile {
        file_record,
        nodes,
        edges,
        unresolved_refs,
        node_count,
        edge_count,
    })
}

pub fn index_all(
    project_root: &Path,
    config: &CodeGraphConfig,
    force: bool,
    on_progress: Option<&dyn Fn(IndexProgress)>,
) -> std::io::Result<IndexResult> {
    let span = tracing::info_span!("index_all", ?force, root = %project_root.display());
    let _enter = span.enter();
    let start = Instant::now();
    let mut errors = Vec::new();
    let mut files_indexed = 0;
    let mut nodes_created = 0;
    let mut edges_created = 0;

    let files = scan_directory(project_root, config, |current, file| {
        if let Some(cb) = on_progress {
            cb(IndexProgress {
                phase: IndexPhase::Scanning,
                current,
                total: 0,
                current_file: Some(file.to_string()),
            });
        }
    });

    let mut conn = db::open_database(project_root)?;
    if force {
        db::clear_database(&conn)?;
    }

    // Pre-fetch existing file hashes to avoid DB access in the parallel parse phase.
    let existing_hashes: std::collections::HashMap<String, String> = if force {
        std::collections::HashMap::new()
    } else {
        db::list_files(&conn)?
            .into_iter()
            .map(|f| (f.path, f.content_hash))
            .collect()
    };

    if let Some(cb) = on_progress {
        cb(IndexProgress {
            phase: IndexPhase::Parsing,
            current: 0,
            total: files.len(),
            current_file: None,
        });
    }

    info!(total_files = files.len(), "starting parallel parse phase");

    // Phase 1: Parse all files in parallel (CPU-bound, no DB access).
    let parsed: Vec<ParsedFile> = files
        .par_iter()
        .filter_map(|file| parse_file_only(project_root, config, &existing_hashes, file))
        .collect();

    let files_skipped = files.len().saturating_sub(parsed.len());
    info!(
        parsed = parsed.len(),
        skipped = files_skipped,
        "parse phase complete"
    );

    if let Some(cb) = on_progress {
        cb(IndexProgress {
            phase: IndexPhase::Storing,
            current: 0,
            total: parsed.len(),
            current_file: None,
        });
    }

    // Phase 2: Store results sequentially (SQLite does not support concurrent writes).
    for (idx, parsed_file) in parsed.into_iter().enumerate() {
        // Delete the old record before inserting the new batch so foreign keys are clean.
        let _ = db::delete_file(&mut conn, &parsed_file.file_record.path);

        if let Some(cb) = on_progress {
            cb(IndexProgress {
                phase: IndexPhase::Storing,
                current: idx + 1,
                total: files.len(),
                current_file: Some(parsed_file.file_record.path.clone()),
            });
        }

        let path = parsed_file.file_record.path.clone();
        debug!(file = %path, nodes = parsed_file.node_count, edges = parsed_file.edge_count, "storing file");
        match db::store_file_batch(
            &mut conn,
            &parsed_file.file_record,
            &parsed_file.nodes,
            &parsed_file.edges,
            &parsed_file.unresolved_refs,
        ) {
            Ok(()) => {
                files_indexed += 1;
                nodes_created += parsed_file.node_count;
                edges_created += parsed_file.edge_count;
            }
            Err(err) => {
                warn!(file = %path, error = %err, "failed to store file");
                errors.push(ExtractionError {
                    message: err.to_string(),
                    line: None,
                    column: None,
                    severity: ExtractionErrorSeverity::Error,
                    code: None,
                });
            }
        }
    }

    // A full index may have introduced the declaration a previously-failed
    // reference was looking for, so every reference goes back in the queue.
    if let Err(err) = db::reset_unresolved_refs_attempted(&conn) {
        warn!(error = %err, "failed to re-queue unresolved references");
    }

    if let Err(err) = ReferenceResolver::resolve_unresolved(&mut conn, project_root, 10_000) {
        warn!(error = %err, "reference resolver failed");
        errors.push(ExtractionError {
            message: format!("Resolver failed: {err}"),
            line: None,
            column: None,
            severity: ExtractionErrorSeverity::Warning,
            code: Some("resolver_failed".to_string()),
        });
    }

    info!(
        files_indexed,
        files_skipped,
        nodes_created,
        edges_created,
        duration_ms = start.elapsed().as_millis(),
        "index_all complete"
    );

    Ok(IndexResult {
        success: errors
            .iter()
            .all(|e| e.severity != ExtractionErrorSeverity::Error),
        files_indexed,
        files_skipped,
        nodes_created,
        edges_created,
        errors,
        duration_ms: start.elapsed().as_millis(),
    })
}

pub fn sync(
    project_root: &Path,
    config: &CodeGraphConfig,
    on_progress: Option<&dyn Fn(IndexProgress)>,
) -> std::io::Result<SyncResult> {
    let span = tracing::info_span!("sync", root = %project_root.display());
    let _enter = span.enter();
    let start = Instant::now();
    let mut conn = db::open_database(project_root)?;

    let current_files: HashSet<String> = scan_directory(project_root, config, |_current, _file| {})
        .into_iter()
        .collect();
    let tracked_files = db::list_files(&conn)?;

    let mut files_added = 0;
    let mut files_modified = 0;
    let mut files_removed = 0;
    let mut nodes_updated = 0;

    for tracked in &tracked_files {
        if !current_files.contains(&tracked.path) {
            db::delete_file(&mut conn, &tracked.path)?;
            files_removed += 1;
        }
    }

    for (idx, file) in current_files.iter().enumerate() {
        if let Some(cb) = on_progress {
            cb(IndexProgress {
                phase: IndexPhase::Parsing,
                current: idx + 1,
                total: current_files.len(),
                current_file: Some(file.clone()),
            });
        }

        let full_path = project_root.join(file);
        let content = fs::read_to_string(&full_path)?;
        let content_hash = hash_sha256(&content);
        let tracked = tracked_files.iter().find(|f| f.path == *file);

        if let Some(tracked) = tracked {
            if tracked.content_hash != content_hash {
                match index_file(project_root, config, &mut conn, file) {
                    Ok(Some((node_count, _))) => {
                        files_modified += 1;
                        nodes_updated += node_count;
                    }
                    Ok(None) => {}
                    Err(err) => {
                        warn!(file = %file, error = %err, "failed to sync file");
                    }
                }
            }
        } else {
            match index_file(project_root, config, &mut conn, file) {
                Ok(Some((node_count, _))) => {
                    files_added += 1;
                    nodes_updated += node_count;
                }
                Ok(None) => {}
                Err(err) => {
                    warn!(file = %file, error = %err, "failed to sync file");
                }
            }
        }
    }

    // A changed or added file may declare the symbol a previously-failed
    // reference was looking for, so those references go back in the queue.
    // When nothing changed there is nothing new to resolve, and the failed
    // references (calls into the standard library, typically) are left
    // alone rather than retried on every run.
    if files_added > 0 || files_modified > 0 || files_removed > 0 {
        if let Err(err) = db::reset_unresolved_refs_attempted(&conn) {
            warn!(error = %err, "failed to re-queue unresolved references");
        }
    }

    // A failed resolution is reported rather than swallowed: the index is
    // still usable, but the caller has to know that some references were
    // not resolved.
    let mut errors = Vec::new();
    if let Err(err) = ReferenceResolver::resolve_unresolved(&mut conn, project_root, 10_000) {
        warn!(error = %err, "reference resolver failed");
        errors.push(ExtractionError {
            message: format!("Resolver failed: {err}"),
            line: None,
            column: None,
            severity: ExtractionErrorSeverity::Warning,
            code: Some("resolver_failed".to_string()),
        });
    }

    info!(
        files_added,
        files_modified,
        files_removed,
        nodes_updated,
        duration_ms = start.elapsed().as_millis(),
        "sync complete"
    );

    Ok(SyncResult {
        files_checked: current_files.len(),
        files_added,
        files_modified,
        files_removed,
        nodes_updated,
        duration_ms: start.elapsed().as_millis(),
        errors,
    })
}

fn index_file(
    project_root: &Path,
    config: &CodeGraphConfig,
    conn: &mut rusqlite::Connection,
    relative_path: &str,
) -> std::io::Result<Option<(usize, usize)>> {
    let full_path = project_root.join(relative_path);
    let content = fs::read_to_string(&full_path)?;

    if (content.len() as u64) > config.max_file_size {
        return Ok(None);
    }

    let language = detect_language(relative_path);
    if !is_language_supported(&language) {
        return Ok(None);
    }

    let content_hash = hash_sha256(&content);
    if let Some(existing) = db::get_file_record(conn, relative_path)? {
        if existing.content_hash == content_hash {
            return Ok(None);
        }
        db::delete_file(conn, relative_path)?;
    }

    let file_name = Path::new(relative_path)
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or(relative_path);
    let qualified_name = relative_path.to_string();
    let node_id = node_id_for_symbol(relative_path, "file", &qualified_name, 1, 0);
    let file_node_id = node_id.clone();

    let now_ms = now_millis();
    let mut nodes = Vec::new();
    let file_node = Node {
        id: node_id,
        kind: NodeKind::File,
        name: file_name.to_string(),
        qualified_name,
        file_path: relative_path.to_string(),
        language,
        start_line: 1,
        end_line: 1,
        start_column: 0,
        end_column: 0,
        docstring: None,
        signature: None,
        visibility: None,
        is_exported: false,
        is_async: false,
        is_static: false,
        is_abstract: false,
        decorators: None,
        type_parameters: None,
        cluster_id: None,
        updated_at: now_ms,
    };
    nodes.push(file_node);

    let (mut extracted_nodes, extracted_edges, unresolved_refs) = extract_nodes(
        project_root,
        relative_path,
        &content,
        language,
        now_ms,
        &file_node_id,
    );
    nodes.append(&mut extracted_nodes);

    if !nodes.is_empty() {
        db::insert_nodes(conn, &nodes)?;
    }
    if !extracted_edges.is_empty() {
        db::insert_edges(conn, &extracted_edges)?;
    }
    if !unresolved_refs.is_empty() {
        db::insert_unresolved_refs(conn, &unresolved_refs)?;
    }

    let metadata = fs::metadata(&full_path)?;
    let file_record = FileRecord {
        path: relative_path.to_string(),
        content_hash,
        language,
        size: metadata.len(),
        modified_at: metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX)),
        indexed_at: now_ms,
        node_count: nodes.len() as i64,
        errors: None,
    };
    db::upsert_file(conn, &file_record)?;

    Ok(Some((nodes.len(), extracted_edges.len())))
}

fn extract_nodes(
    project_root: &Path,
    file_path: &str,
    source: &str,
    language: Language,
    now_ms: i64,
    root_id: &str,
) -> (Vec<Node>, Vec<Edge>, Vec<UnresolvedReference>) {
    let mut parser = Parser::new();
    let ts_lang = match language_to_parser(language) {
        Some(ts_lang) => ts_lang,
        None => return (Vec::new(), Vec::new(), Vec::new()),
    };

    if parser.set_language(&ts_lang).is_err() {
        return (Vec::new(), Vec::new(), Vec::new());
    }

    let tree = match parser.parse(source, None) {
        Some(tree) => tree,
        None => return (Vec::new(), Vec::new(), Vec::new()),
    };

    // Markdown files use a specialised doc-structure extractor rather than
    // the generic code-symbol walker.
    if language == Language::Markdown {
        return extract_markdown_nodes(file_path, source, tree.root_node(), root_id, now_ms);
    }

    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut symbol_index = SymbolIndex::default();
    let mut stack = Vec::new();
    let mut unresolved_refs = Vec::new();
    walk_tree_collect(
        tree.root_node(),
        source,
        project_root,
        file_path,
        language,
        &mut stack,
        Some(root_id.to_string()),
        &mut nodes,
        &mut edges,
        &mut symbol_index,
        &mut unresolved_refs,
        None,
        now_ms,
    );
    walk_tree_calls(
        tree.root_node(),
        source,
        file_path,
        language,
        &symbol_index,
        root_id,
        &mut edges,
        &mut unresolved_refs,
        &mut Vec::new(),
        &mut HashMap::new(),
    );
    (nodes, edges, unresolved_refs)
}

// ─── Markdown doc-structure extraction ──────────────────────────────────────
//
// Produces:
//  • NodeKind::Module  for every ATX / setext heading  (the doc section)
//  • UnresolvedReference (EdgeKind::References) for every inline `code_span`
//    that looks like a symbol name, so the resolution pass can link them to
//    real code nodes and the audit pass can report the ones that never resolved.

/// Top-level entry point called from `extract_nodes` when language == Markdown.
fn extract_markdown_nodes(
    file_path: &str,
    source: &str,
    root: TsNode,
    root_id: &str,
    now_ms: i64,
) -> (Vec<Node>, Vec<Edge>, Vec<UnresolvedReference>) {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut unresolved = Vec::new();
    // heading_stack: (level 1-6, section_node_id) – used to associate code
    // spans with their nearest enclosing section heading.
    let mut heading_stack: Vec<(u8, String)> = Vec::new();

    walk_markdown_node(
        root,
        source,
        file_path,
        root_id,
        &mut heading_stack,
        &mut nodes,
        &mut edges,
        &mut unresolved,
        now_ms,
    );

    (nodes, edges, unresolved)
}

/// Recursive AST walker for Markdown trees.
///
/// * Headings   → `NodeKind::Module` nodes linked to the file via Contains.
/// * `code_span`  → `UnresolvedReference` (References) from the current section
///   (or the file node when no heading has been seen yet).
/// * Everything else is recursed into without producing nodes.
fn walk_markdown_node(
    node: TsNode,
    source: &str,
    file_path: &str,
    file_node_id: &str,
    heading_stack: &mut Vec<(u8, String)>,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    unresolved: &mut Vec<UnresolvedReference>,
    now_ms: i64,
) {
    match node.kind() {
        "atx_heading" | "setext_heading" => {
            let level = markdown_heading_level(&node);
            if let Some(text) = markdown_heading_text(&node, source) {
                let start = node.start_position();
                let end = node.end_position();
                let qualified = format!("{file_path}::{text}");
                let id = node_id_for_symbol(
                    file_path,
                    "module",
                    &qualified,
                    start.row as i64 + 1,
                    start.column as i64,
                );
                nodes.push(Node {
                    id: id.clone(),
                    kind: NodeKind::Module,
                    name: text,
                    qualified_name: qualified,
                    file_path: file_path.to_string(),
                    language: Language::Markdown,
                    start_line: start.row as i64 + 1,
                    end_line: end.row as i64 + 1,
                    start_column: start.column as i64,
                    end_column: end.column as i64,
                    docstring: None,
                    signature: Some(format!("h{level}")),
                    visibility: None,
                    is_exported: false,
                    is_async: false,
                    is_static: false,
                    is_abstract: false,
                    decorators: None,
                    type_parameters: None,
                    cluster_id: None,
                    updated_at: now_ms,
                });
                edges.push(Edge {
                    source: file_node_id.to_string(),
                    target: id.clone(),
                    kind: EdgeKind::Contains,
                    metadata: None,
                    line: Some(start.row as i64 + 1),
                    column: Some(start.column as i64),
                    confidence: 1.0,
                    process_id: None,
                });
                // Keep the stack tidy: pop any heading at the same or deeper level.
                heading_stack.retain(|(l, _)| *l < level);
                heading_stack.push((level, id));
            }
            // Do NOT recurse into heading children — heading text like
            // `fn foo()` should not be treated as a symbol reference.
        }
        "code_span" => {
            if let Some(raw) = markdown_code_span_text(&node, source) {
                // Strip trailing `()` so `foo()` resolves the same as `foo`.
                let name = raw.trim_end_matches("()").trim().to_string();
                if looks_like_symbol(&name) {
                    let start = node.start_position();
                    let from_id = heading_stack
                        .last()
                        .map_or(file_node_id, |(_, id)| id.as_str());
                    unresolved.push(UnresolvedReference {
                        from_node_id: from_id.to_string(),
                        reference_name: name,
                        reference_kind: EdgeKind::References,
                        line: start.row as i64 + 1,
                        column: start.column as i64,
                        candidates: None,
                        qualifier: None,
                    });
                }
            }
        }
        _ => {
            for child in node.children(&mut node.walk()) {
                walk_markdown_node(
                    child,
                    source,
                    file_path,
                    file_node_id,
                    heading_stack,
                    nodes,
                    edges,
                    unresolved,
                    now_ms,
                );
            }
        }
    }
}

/// Determine ATX / setext heading level (1–6).
fn markdown_heading_level(node: &TsNode) -> u8 {
    if node.kind() == "setext_heading" {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "setext_h1_underline" => return 1,
                "setext_h2_underline" => return 2,
                _ => {}
            }
        }
        return 1;
    }
    // atx_heading: inspect marker children.
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "atx_h1_marker" => return 1,
            "atx_h2_marker" => return 2,
            "atx_h3_marker" => return 3,
            "atx_h4_marker" => return 4,
            "atx_h5_marker" => return 5,
            "atx_h6_marker" => return 6,
            _ => {}
        }
    }
    1
}

/// Extract the plain-text content of a heading node.
///
/// Returns `None` if the heading has no discernible text.
fn markdown_heading_text(node: &TsNode, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "heading_content" {
            return child
                .utf8_text(source.as_bytes())
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
        }
    }
    None
}

/// Extract the text of a `code_span` node (the inline backtick content,
/// including delimiter backticks).
fn markdown_code_span_text(node: &TsNode, source: &str) -> Option<String> {
    let mut text = String::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "text" {
            if let Ok(t) = child.utf8_text(source.as_bytes()) {
                text.push_str(t);
            }
        }
    }
    if !text.is_empty() {
        return Some(text);
    }
    // Fallback: take the full node text and strip surrounding backticks.
    node.utf8_text(source.as_bytes())
        .ok()
        .map(|s| s.trim_matches('`').trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Heuristic: does `s` look like a code symbol worth tracking?
///
/// Rejects:
/// * strings with whitespace (these are phrases, not identifiers)
/// * single characters
/// * pure numeric literals
/// * common value literals that aren't project-specific symbols
fn looks_like_symbol(s: &str) -> bool {
    if s.len() < 2 || s.contains(' ') {
        return false;
    }
    if s.chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c == '_')
    {
        return false;
    }
    let first = s.chars().next().unwrap_or('\0');
    if !first.is_alphabetic() && first != '_' {
        return false;
    }
    !matches!(
        s,
        "true"
            | "false"
            | "null"
            | "nil"
            | "None"
            | "Some"
            | "Ok"
            | "Err"
            | "self"
            | "super"
            | "crate"
            | "pub"
            | "fn"
            | "let"
            | "mut"
            | "use"
            | "mod"
            | "struct"
            | "enum"
            | "impl"
            | "trait"
            | "type"
            | "async"
            | "await"
            | "return"
            | "if"
            | "else"
            | "match"
            | "for"
            | "while"
            | "loop"
            | "break"
            | "continue"
    )
}

fn language_to_parser(language: Language) -> Option<tree_sitter::Language> {
    match language {
        Language::Rust => Some(tree_sitter::Language::new(tree_sitter_rust::LANGUAGE)),
        Language::JavaScript | Language::Jsx => {
            Some(tree_sitter::Language::new(tree_sitter_javascript::LANGUAGE))
        }
        Language::TypeScript => Some(tree_sitter::Language::new(
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        )),
        Language::Tsx => Some(tree_sitter::Language::new(
            tree_sitter_typescript::LANGUAGE_TSX,
        )),
        Language::Python => Some(tree_sitter::Language::new(tree_sitter_python::LANGUAGE)),
        Language::Go => Some(tree_sitter::Language::new(tree_sitter_go::LANGUAGE)),
        Language::Java => Some(tree_sitter::Language::new(tree_sitter_java::LANGUAGE)),
        Language::C => Some(tree_sitter::Language::new(tree_sitter_c::LANGUAGE)),
        Language::Cpp => Some(tree_sitter::Language::new(tree_sitter_cpp::LANGUAGE)),
        Language::CSharp => Some(tree_sitter::Language::new(tree_sitter_c_sharp::LANGUAGE)),
        Language::Blazor => Some(tree_sitter::Language::new(tree_sitter_blazor::LANGUAGE)),
        Language::Ruby => Some(tree_sitter::Language::new(tree_sitter_ruby::LANGUAGE)),
        // New language parsers
        Language::Bash => Some(tree_sitter::Language::new(tree_sitter_bash::LANGUAGE)),
        Language::Dart => Some(tree_sitter::Language::new(tree_sitter_dart::LANGUAGE)),
        Language::Elixir => Some(tree_sitter::Language::new(tree_sitter_elixir::LANGUAGE)),
        Language::Elm => Some(tree_sitter::Language::new(tree_sitter_elm::LANGUAGE)),
        Language::Erlang => Some(tree_sitter::Language::new(tree_sitter_erlang::LANGUAGE)),
        Language::Fortran => Some(tree_sitter::Language::new(tree_sitter_fortran::LANGUAGE)),
        Language::Groovy => Some(tree_sitter::Language::new(tree_sitter_groovy::LANGUAGE)),
        Language::Haskell => Some(tree_sitter::Language::new(tree_sitter_haskell::LANGUAGE)),
        Language::Julia => Some(tree_sitter::Language::new(tree_sitter_julia::LANGUAGE)),
        Language::Lua => Some(tree_sitter::Language::new(tree_sitter_lua::LANGUAGE)),
        Language::Matlab => Some(tree_sitter::Language::new(tree_sitter_matlab::LANGUAGE)),
        Language::Nix => Some(tree_sitter::Language::new(tree_sitter_nix::LANGUAGE)),
        Language::Perl => Some(tree_sitter::Language::new(ts_parser_perl::LANGUAGE)),
        Language::Powershell => Some(tree_sitter::Language::new(tree_sitter_powershell::LANGUAGE)),
        Language::R => Some(tree_sitter::Language::new(tree_sitter_r::LANGUAGE)),
        Language::Scala => Some(tree_sitter::Language::new(tree_sitter_scala::LANGUAGE)),
        Language::Yaml => Some(tree_sitter::Language::new(tree_sitter_yaml::LANGUAGE)),
        Language::Zig => Some(tree_sitter::Language::new(tree_sitter_zig::LANGUAGE)),
        // Recently compatible parsers (updated to work with tree-sitter 0.26)
        Language::Php => Some(tree_sitter::Language::new(tree_sitter_php::LANGUAGE_PHP)),
        Language::Swift => Some(tree_sitter::Language::new(tree_sitter_swift::LANGUAGE)),
        Language::Kotlin => Some(tree_sitter::Language::new(tree_sitter_kotlin_ng::LANGUAGE)),
        Language::Markdown => Some(tree_sitter_markdown_updated::language()),
        Language::Toml => Some(tree_sitter::Language::new(tree_sitter_toml_ng::LANGUAGE)),
        // Unsupported languages
        Language::Liquid | Language::Unknown => None,
    }
}

#[derive(Debug, Default)]
struct SymbolIndex {
    by_name: HashMap<String, Vec<String>>,
    by_key: HashMap<String, String>,
    callable_ids: HashSet<String>,
    /// Ids of nodes that name a type (class, struct, interface, enum), used
    /// to tell `Circle(2.0)` (an instantiation) from `circle.area()` (a call).
    type_ids: HashSet<String>,
    /// Names imported from another module that look like type names.
    ///
    /// Kotlin and Swift spell a constructor call `Circle(2.0)` — the same
    /// shape as a function call — so the callee has to be recognised as a
    /// type for the edge to be an `Instantiates` rather than a `Calls`.
    imported_type_names: HashSet<String>,
    /// Members of each type declared in this file: type name → member name →
    /// node id.
    ///
    /// This is what makes `helper.area()` resolve to `Circle.area` rather
    /// than to whichever `area` happens to be declared nearby.
    type_members: HashMap<String, HashMap<String, String>>,
}

/// Read visibility from a declaration tree-sitter node.
///
/// Returns `(visibility, is_exported)`. `is_exported` is true iff the
/// node is exposed to consumers outside the declaring module — i.e.,
/// reachable from a downstream crate's `use` statement or a library
/// consumer's import. Restricted-visibility keywords (`pub(crate)`,
/// `pub(super)`, `pub(in path)`) are exported = false because they
/// are NOT part of the public API surface, even though they ARE
/// reachable from inside the crate.
///
/// - **Rust** — looks for a `visibility_modifier` child. Plain `pub`
///   maps to `(Visibility::Public, true)`; qualified forms
///   (`pub(crate)`, `pub(super)`, `pub(in path)`) map to
///   `(Visibility::Internal, false)`; missing modifier is `None`.
/// - **TypeScript / JavaScript** — walks ancestors for
///   `export_statement`. Maps to `(Visibility::Public, true)`.
/// - **Java / C# / Blazor** — walks children for a `modifiers` node
///   containing `public`. Maps to `(Visibility::Public, true)`.
///
/// Languages without a keyword-based visibility concept (Python,
/// Ruby, etc.) return `None`. Convention-based visibility (e.g.
/// Python's leading-underscore) is left to a follow-up.
fn read_declaration_visibility(
    node: TsNode,
    language: Language,
    source: &str,
) -> Option<(Visibility, bool)> {
    match language {
        Language::Rust => {
            // tree-sitter-rust's grammar names the visibility child
            // `visibility_modifier`, but the field name has shifted
            // between minor versions; fall back to scanning the direct
            // children by node kind to stay robust.
            let vis = node
                .child_by_field_name("visibility_modifier")
                .or_else(|| {
                    let mut cursor = node.walk();
                    node.children(&mut cursor)
                        .find(|c| c.kind() == "visibility_modifier")
                })?;
            let text = vis.utf8_text(source.as_bytes()).ok()?.trim();
            if text == "pub" {
                Some((Visibility::Public, true))
            } else if text.starts_with("pub(") {
                Some((Visibility::Internal, false))
            } else {
                None
            }
        }
        Language::TypeScript | Language::Tsx | Language::JavaScript => {
            let mut current = node;
            loop {
                if current.kind() == "export_statement" {
                    return Some((Visibility::Public, true));
                }
                let parent = current.parent()?;
                current = parent;
            }
        }
        Language::Java | Language::CSharp | Language::Blazor => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "modifiers" {
                    let text = child.utf8_text(source.as_bytes()).ok()?;
                    if text.contains("public") {
                        return Some((Visibility::Public, true));
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn walk_tree_collect(
    node: TsNode,
    source: &str,
    project_root: &Path,
    file_path: &str,
    language: Language,
    stack: &mut Vec<String>,
    parent_id: Option<String>,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    symbol_index: &mut SymbolIndex,
    unresolved_refs: &mut Vec<UnresolvedReference>,
    enclosing_type_name: Option<&str>,
    now_ms: i64,
) {
    let (kind, is_container) = map_node_kind(&node, language);

    if let Some(NodeKind::Import) = kind {
        if let Some(parent_id) = parent_id.clone() {
            add_import_nodes(
                &node,
                source,
                language,
                file_path,
                parent_id,
                nodes,
                edges,
                symbol_index,
                now_ms,
            );
            return;
        }
    }

    if let Some(NodeKind::Module) = kind {
        if let Some(parent_id) = parent_id.clone() {
            match language {
                // Rust's `mod foo;` declares nothing of its own, so the
                // module node is created here and the walk ends with it.
                Language::Rust => {
                    add_module_node(
                        &node,
                        source,
                        project_root,
                        language,
                        file_path,
                        parent_id,
                        nodes,
                        edges,
                        now_ms,
                    );
                    return;
                }
                // Kotlin's `package` header marks the file's package
                // rather than declaring anything, so it produces no node
                // of its own.
                Language::Kotlin => return,
                // Every other language declares inside its module, so the
                // module node is created by the normal path below and the
                // walk continues into it.
                _ => {}
            }
        }
    }

    let mut handled_export = false;
    if let Some(NodeKind::Export) = kind {
        if let Some(parent_id) = parent_id.clone() {
            add_export_nodes(
                &node, source, language, file_path, parent_id, nodes, edges, now_ms,
            );
            handled_export = true;
        }
    }

    let name = if handled_export {
        None
    } else {
        match kind {
            Some(_) => node_name(&node, source, language),
            None => None,
        }
    };

    let mut next_parent_id = parent_id.clone();

    // The name of the type this node is declared in, if any — used to scope
    // member lookup so `helper.area()` resolves against `Circle`.
    let mut next_enclosing_type: Option<String> = enclosing_type_name.map(str::to_string);

    if let (Some(kind), Some(name)) = (kind, name.clone()) {
        let qualified_name = if stack.is_empty() {
            format!("{}::{}", file_path, name)
        } else {
            format!("{}::{}::{}", file_path, stack.join("::"), name)
        };
        let id = node_id_for_symbol(
            file_path,
            &format!("{:?}", kind).to_ascii_lowercase(),
            &qualified_name,
            node.start_position().row as i64 + 1,
            node.start_position().column as i64,
        );
        let start = node.start_position();
        let end = node.end_position();
        let (visibility, is_exported) = match read_declaration_visibility(node, language, source) {
            Some((text, exported)) => (Some(text), exported),
            None => (None, false),
        };

        nodes.push(Node {
            id: id.clone(),
            kind,
            name: name.clone(),
            qualified_name,
            file_path: file_path.to_string(),
            language,
            start_line: start.row as i64 + 1,
            end_line: end.row as i64 + 1,
            start_column: start.column as i64,
            end_column: end.column as i64,
            docstring: None,
            signature: None,
            visibility,
            is_exported,
            is_async: false,
            is_static: false,
            is_abstract: false,
            decorators: None,
            type_parameters: None,
            updated_at: now_ms,
            cluster_id: None,
        });

        if is_callable_kind(kind) {
            let key = node_key(kind, start, &name);
            symbol_index.by_key.insert(key, id.clone());
            symbol_index
                .by_name
                .entry(name.clone())
                .or_default()
                .push(id.clone());
            symbol_index.callable_ids.insert(id.clone());
        }

        if matches!(
            kind,
            NodeKind::Class | NodeKind::Struct | NodeKind::Interface | NodeKind::Enum
        ) {
            symbol_index
                .by_name
                .entry(name.clone())
                .or_default()
                .push(id.clone());
            symbol_index.type_ids.insert(id.clone());
        }

        // A type declaration becomes the enclosing type for its children.
        if matches!(
            kind,
            NodeKind::Class | NodeKind::Struct | NodeKind::Interface | NodeKind::Enum
        ) {
            next_enclosing_type = Some(name.clone());
        }

        // Record the member under its enclosing type so that a qualified
        // call can be scoped to the right type.
        if let Some(enclosing_type) = enclosing_type_name {
            symbol_index
                .type_members
                .entry(enclosing_type.to_string())
                .or_default()
                .insert(name.clone(), id.clone());
        }

        // Inheritance: link the declaration to its supertypes.
        for (supertype, supertype_kind) in heritage_names(&node, source, language) {
            let edge_kind = match supertype_kind {
                grammar::HeritageKind::Extends => EdgeKind::Extends,
                grammar::HeritageKind::Implements => EdgeKind::Implements,
            };

            match symbol_index.by_name.get(&supertype) {
                Some(targets) if targets.len() == 1 => {
                    edges.push(Edge {
                        source: id.clone(),
                        target: targets[0].clone(),
                        kind: edge_kind,
                        metadata: None,
                        line: Some(start.row as i64 + 1),
                        column: Some(start.column as i64),
                        confidence: 1.0,
                        process_id: None,
                    });
                }
                Some(targets) => {
                    unresolved_refs.push(UnresolvedReference {
                        from_node_id: id.clone(),
                        reference_name: supertype.clone(),
                        reference_kind: edge_kind,
                        line: start.row as i64 + 1,
                        column: start.column as i64,
                        candidates: Some(targets.clone()),
                        qualifier: None,
                    });
                }
                None => {
                    unresolved_refs.push(UnresolvedReference {
                        from_node_id: id.clone(),
                        reference_name: supertype.clone(),
                        reference_kind: edge_kind,
                        line: start.row as i64 + 1,
                        column: start.column as i64,
                        candidates: None,
                        qualifier: None,
                    });
                }
            }
        }

        if let Some(parent_id) = parent_id.clone() {
            edges.push(Edge {
                source: parent_id.clone(),
                target: id.clone(),
                kind: EdgeKind::Contains,
                metadata: None,
                line: Some(start.row as i64 + 1),
                column: Some(start.column as i64),
                confidence: 1.0,
                process_id: None,
            });

            if kind == NodeKind::Import {
                edges.push(Edge {
                    source: parent_id.clone(),
                    target: id.clone(),
                    kind: EdgeKind::Imports,
                    metadata: None,
                    line: Some(start.row as i64 + 1),
                    column: Some(start.column as i64),
                    confidence: 1.0,
                    process_id: None,
                });
            }

            if kind == NodeKind::Export {
                edges.push(Edge {
                    source: parent_id.clone(),
                    target: id.clone(),
                    kind: EdgeKind::Exports,
                    metadata: None,
                    line: Some(start.row as i64 + 1),
                    column: Some(start.column as i64),
                    confidence: 1.0,
                    process_id: None,
                });
            }
        }

        if is_container {
            stack.push(name);
            next_parent_id = Some(id);
        }
    }

    for child in node.children(&mut node.walk()) {
        walk_tree_collect(
            child,
            source,
            project_root,
            file_path,
            language,
            stack,
            next_parent_id.clone(),
            nodes,
            edges,
            symbol_index,
            unresolved_refs,
            next_enclosing_type.as_deref(),
            now_ms,
        );
    }

    if is_container && name.is_some() {
        stack.pop();
    }
}

fn walk_tree_calls(
    node: TsNode,
    source: &str,
    _file_path: &str,
    language: Language,
    symbol_index: &SymbolIndex,
    file_id: &str,
    edges: &mut Vec<Edge>,
    unresolved_refs: &mut Vec<UnresolvedReference>,
    scope_stack: &mut Vec<String>,
    local_types: &mut HashMap<String, String>,
) {
    let (kind, _) = map_node_kind(&node, language);
    let name = if kind.is_some() {
        node_name(&node, source, language)
    } else {
        None
    };

    if let (Some(kind), Some(name)) = (kind, name.clone()) {
        if is_callable_kind(kind) {
            let key = node_key(kind, node.start_position(), &name);
            if let Some(id) = symbol_index.by_key.get(&key) {
                scope_stack.push(id.clone());
            }
        }
    }

    // Track the declared type of local variables so that a qualified call
    // like `helper.area()` can be attributed to the type of `helper`
    // (`Circle`) rather than to a bare local name.
    if let Some((var_name, type_name)) = local_variable_type(&node, source, language) {
        local_types.insert(var_name, type_name);
    }

    if is_call_expression(node.kind(), language) {
        // Calls that appear outside any function scope (property initialisers,
        // `init {}` blocks, top-level statements, unnamed arrow functions)
        // are anchored to the file node so the edge is not silently dropped.
        let source_id = scope_stack
            .last()
            .cloned()
            .unwrap_or_else(|| file_id.to_string());
        if let Some((callee_name, qualifier)) = call_name(&node, source, language) {
            // When the receiver is a local variable with a known type, use
            // the *type* as the qualifier: `helper.area()` on a `Circle`
            // resolves like `Circle.area()`, which is what makes
            // cross-package calls resolvable.
            let qualifier = qualifier.and_then(|q| local_types.get(&q).cloned().or(Some(q)));

            let start = node.start_position();
            // An instantiation is either an explicit construction node
            // (`new Foo()`) or a call whose callee is a bare type name —
            // which is how Kotlin and Swift spell `Circle(2.0)`.
            // Ruby spells construction `Helper.new` — the *receiver* names
            // the type, so the callee for resolution purposes is the
            // receiver, not `new`.
            let is_new_constructor =
                language == Language::Ruby && callee_name == "new" && qualifier.is_some();
            let (callee_name, qualifier) = if is_new_constructor {
                (qualifier.clone().unwrap_or_default(), None)
            } else {
                (callee_name, qualifier)
            };

            let is_instantiation = grammar::is_instantiation_kind(language, node.kind())
                || is_new_constructor
                || (qualifier.is_none()
                    && looks_like_type_name(&callee_name)
                    && (symbol_index.imported_type_names.contains(&callee_name)
                        || symbol_index
                            .by_name
                            .get(&callee_name)
                            .is_some_and(|targets| {
                                targets.iter().any(|id| symbol_index.type_ids.contains(id))
                            })));
            let edge_kind = if is_instantiation {
                EdgeKind::Instantiates
            } else {
                EdgeKind::Calls
            };
            // A qualified call is scoped to its receiver's type: `helper.area()`
            // on a `Circle` resolves to `Circle.area`, not to whichever `area`
            // happens to be declared in the same file.
            let scoped_target = qualifier.as_deref().and_then(|qualifier| {
                symbol_index
                    .type_members
                    .get(qualifier)
                    .and_then(|members| members.get(&callee_name))
                    .cloned()
            });

            let candidates: Option<Vec<String>> = match (&scoped_target, qualifier.as_deref()) {
                (Some(target), _) => Some(vec![target.clone()]),
                (None, Some(qualifier)) => {
                    let members = symbol_index.type_members.get(qualifier);
                    let matching: Vec<String> = members
                        .map(|members| members.get(&callee_name).cloned().into_iter().collect())
                        .unwrap_or_default();
                    if matching.is_empty() {
                        None
                    } else {
                        Some(matching)
                    }
                }
                (None, None) => symbol_index.by_name.get(&callee_name).cloned(),
            };

            match candidates {
                Some(targets) if targets.len() == 1 => {
                    edges.push(Edge {
                        source: source_id.clone(),
                        target: targets[0].clone(),
                        kind: edge_kind,
                        metadata: None,
                        line: Some(start.row as i64 + 1),
                        column: Some(start.column as i64),
                        confidence: 1.0,
                        process_id: None,
                    });
                }
                Some(targets) => {
                    unresolved_refs.push(UnresolvedReference {
                        from_node_id: source_id.clone(),
                        reference_name: callee_name.clone(),
                        reference_kind: edge_kind,
                        line: start.row as i64 + 1,
                        column: start.column as i64,
                        candidates: Some(targets.clone()),
                        qualifier,
                    });
                }
                None => {
                    unresolved_refs.push(UnresolvedReference {
                        from_node_id: source_id.clone(),
                        reference_name: callee_name.clone(),
                        reference_kind: edge_kind,
                        line: start.row as i64 + 1,
                        column: start.column as i64,
                        candidates: None,
                        qualifier,
                    });
                }
            }
        }
    }

    for child in node.children(&mut node.walk()) {
        walk_tree_calls(
            child,
            source,
            _file_path,
            language,
            symbol_index,
            file_id,
            edges,
            unresolved_refs,
            scope_stack,
            local_types,
        );
    }

    if let (Some(kind), Some(name)) = (kind, name) {
        if is_callable_kind(kind) {
            let key = node_key(kind, node.start_position(), &name);
            if symbol_index.by_key.contains_key(&key) {
                scope_stack.pop();
            }
        }
    }
}

/// Recover the name of a declaration node.
///
/// Most grammars expose a `name` field, but several do not:
///
/// - Kotlin declares properties as `property_declaration` → `variable_declaration`
///   → `identifier`.
/// - C and C++ nest the function name inside the declarator chain:
///   `function_definition` → `function_declarator` → `declarator` → `identifier`.
/// - Swift's `property_declaration` uses a `pattern` field whose text is the
///   bound identifier.
fn node_name(node: &TsNode, source: &str, language: Language) -> Option<String> {
    let text_of = |n: &TsNode| {
        n.utf8_text(source.as_bytes())
            .ok()
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
    };

    match language {
        Language::Kotlin => {
            // `property_declaration` has no `name` field: the identifier lives
            // under `variable_declaration`.
            if node.kind() == "property_declaration" {
                let mut cursor = node.walk();
                if let Some(var_decl) = node
                    .named_children(&mut cursor)
                    .find(|c| c.kind() == "variable_declaration")
                {
                    let mut inner = var_decl.walk();
                    if let Some(ident) = var_decl
                        .named_children(&mut inner)
                        .find(|c| c.kind() == "identifier")
                    {
                        return text_of(&ident);
                    }
                }
            }
            if node.kind() == "class_declaration" || node.kind() == "object_declaration" {
                return text_of(&node.child_by_field_name("name")?);
            }
        }
        Language::C | Language::Cpp => {
            // `function_definition` has no `name` field: the name is inside
            // the declarator chain, e.g.
            //   function_definition → function_declarator → declarator → identifier
            if node.kind() == "function_definition" {
                let mut current = node.child_by_field_name("declarator");
                let mut guard = 0;
                while let Some(decl) = current {
                    guard += 1;
                    if guard > 16 {
                        break;
                    }
                    let kind = decl.kind();
                    if kind == "identifier" || kind == "field_identifier" {
                        return text_of(&decl);
                    }
                    if kind == "qualified_identifier" {
                        return text_of(&decl);
                    }
                    current = decl.child_by_field_name("declarator");
                }
            }
        }
        Language::Swift => {
            // `property_declaration` names itself through a `pattern` field
            // whose text is the bound identifier.
            if node.kind() == "property_declaration" {
                if let Some(pattern) = node.child_by_field_name("name") {
                    if let Some(name) = text_of(&pattern) {
                        return Some(name);
                    }
                }
            }
        }
        Language::Php if node.kind() == "name" => {
            // PHP wraps identifiers in a `name` node; its text is the name.
            return text_of(node);
        }
        Language::Fortran => {
            // The grammar wraps a declaration and its body: the `module`
            // node holds the `module_statement` that names it, and the
            // `function` and `subroutine` nodes hold the statement that
            // names them.
            let statement_kind = match node.kind() {
                "module" => Some("module_statement"),
                "program" => Some("program_statement"),
                "function" => Some("function_statement"),
                "subroutine" => Some("subroutine_statement"),
                _ => None,
            };
            if let Some(statement_kind) = statement_kind {
                let mut cursor = node.walk();
                let statement = node
                    .named_children(&mut cursor)
                    .find(|c| c.kind() == statement_kind)?;
                // A function or subroutine statement names itself through a
                // `name` field; a module or program statement holds a plain
                // `name` child instead.
                if let Some(name) = statement.child_by_field_name("name") {
                    return text_of(&name);
                }
                let mut inner = statement.walk();
                let name = statement
                    .named_children(&mut inner)
                    .find(|c| c.kind() == "name")?;
                return text_of(&name);
            }
            // `type, public :: circle` — the type name is the `type_name`
            // child of the nested `derived_type_statement`, not a field of
            // the definition itself.
            if node.kind() == "derived_type_definition" {
                let mut cursor = node.walk();
                let statement = node
                    .named_children(&mut cursor)
                    .find(|c| c.kind() == "derived_type_statement")?;
                let mut inner = statement.walk();
                let type_name = statement
                    .named_children(&mut inner)
                    .find(|c| c.kind() == "type_name")?;
                return text_of(&type_name);
            }
            // `real(real64) :: radius` — the declared name is the
            // `declarator` field.
            if node.kind() == "variable_declaration" {
                return text_of(&node.child_by_field_name("declarator")?);
            }
        }
        Language::Haskell => {
            // The root node of a Haskell file holds the module header, so
            // the module's name is the `module` field of the nested
            // `header`.
            if node.kind() == "haskell" {
                let mut cursor = node.walk();
                let header = node
                    .named_children(&mut cursor)
                    .find(|c| c.kind() == "header")?;
                return text_of(&header.child_by_field_name("module")?);
            }
        }
        Language::Julia => {
            // Julia's declarations carry no `name` field: the name of a
            // function or a macro is the identifier in its `signature`, and
            // the name of a type is the identifier in its `type_head`. In
            // every case it is the first identifier of the declaration.
            if matches!(
                node.kind(),
                "function_definition"
                    | "macro_definition"
                    | "struct_definition"
                    | "abstract_definition"
                    | "primitive_definition"
                    | "const_statement"
            ) {
                return first_identifier_text(node, source);
            }
            // `area(shape) = 3.14` — the short form of a function
            // definition is an assignment whose left-hand side is a call.
            // A plain assignment is not a declaration, so it has no name.
            if node.kind() == "assignment" {
                let mut cursor = node.walk();
                let lhs = node.named_children(&mut cursor).next()?;
                if lhs.kind() == "call_expression" {
                    return first_identifier_text(&lhs, source);
                }
                return None;
            }
        }
        Language::Lua => {
            // `function M.hello(name)` — the name field holds a
            // `dot_index_expression` or a `method_index_expression`, and the
            // function's own name is its last segment.
            if node.kind() == "function_declaration" {
                let raw = text_of(&node.child_by_field_name("name")?)?;
                let last = raw
                    .rsplit(['.', ':'])
                    .next()
                    .unwrap_or(&raw)
                    .trim()
                    .to_string();
                if !last.is_empty() {
                    return Some(last);
                }
            }
        }
        Language::Matlab => {
            // `enum RED` — the member name is the `identifier` child of the
            // `enum` node; the grammar gives it no name field.
            if node.kind() == "enum" {
                let mut cursor = node.walk();
                let identifier = node
                    .named_children(&mut cursor)
                    .find(|c| c.kind() == "identifier")?;
                return text_of(&identifier);
            }
        }
        Language::R if node.kind() == "function_definition" => {
            // `greet <- function(name) name` — the name of the function is
            // the left-hand side of the assignment it is bound to. The
            // `name` field of the `function_definition` holds the
            // `function` keyword, so it must never be used as a fallback.
            let parent = node.parent()?;
            if parent.kind() == "binary_operator" {
                return text_of(&parent.child_by_field_name("lhs")?);
            }
            return None;
        }
        _ => {}
    }

    let name_node = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("identifier"))
        .or_else(|| node.child_by_field_name("property"))
        .or_else(|| node.child_by_field_name("tag_name"));

    name_node.and_then(|n| text_of(&n))
}

/// The text of the first identifier in a subtree, in document order.
///
/// Julia's declarations are the reason this exists: `function greet(name)`
/// puts the name in a `signature` child and `struct Point` puts it in a
/// `type_head` child, and neither is reachable through a field. In both
/// cases the name is the first identifier of the declaration.
fn first_identifier_text(node: &TsNode, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "identifier" {
            return child
                .utf8_text(source.as_bytes())
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
        }
        if let Some(found) = first_identifier_text(&child, source) {
            return Some(found);
        }
    }
    None
}

#[derive(Debug, Clone)]
struct ImportSymbol {
    local_name: String,
    module_path: String,
    export_name: Option<String>,
}

#[derive(Debug, Clone)]
struct ExportSymbol {
    name: String,
    module_path: Option<String>,
}

fn add_import_nodes(
    node: &TsNode,
    source: &str,
    language: Language,
    file_path: &str,
    parent_id: String,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    symbol_index: &mut SymbolIndex,
    now_ms: i64,
) {
    let imports = import_symbols(node, source, language);
    if imports.is_empty() {
        return;
    }

    let start = node.start_position();
    let end = node.end_position();

    for import in imports {
        let qualified_name = format!(
            "{}::import::{}::{}",
            file_path, import.local_name, import.module_path
        );
        let id = node_id_for_symbol(
            file_path,
            "import",
            &qualified_name,
            start.row as i64 + 1,
            start.column as i64,
        );
        let signature = build_import_signature(&import.module_path, import.export_name.as_deref());

        if looks_like_type_name(&import.local_name) {
            symbol_index
                .imported_type_names
                .insert(import.local_name.clone());
        }

        nodes.push(Node {
            id: id.clone(),
            kind: NodeKind::Import,
            name: import.local_name,
            qualified_name,
            file_path: file_path.to_string(),
            language,
            start_line: start.row as i64 + 1,
            end_line: end.row as i64 + 1,
            start_column: start.column as i64,
            end_column: end.column as i64,
            docstring: None,
            signature: Some(signature),
            visibility: None,
            is_exported: false,
            is_async: false,
            is_static: false,
            is_abstract: false,
            decorators: None,
            type_parameters: None,
            updated_at: now_ms,
            cluster_id: None,
        });

        edges.push(Edge {
            source: parent_id.clone(),
            target: id.clone(),
            kind: EdgeKind::Contains,
            metadata: None,
            line: Some(start.row as i64 + 1),
            column: Some(start.column as i64),
            confidence: 1.0,
            process_id: None,
        });
        edges.push(Edge {
            source: parent_id.clone(),
            target: id,
            kind: EdgeKind::Imports,
            metadata: None,
            line: Some(start.row as i64 + 1),
            column: Some(start.column as i64),
            confidence: 1.0,
            process_id: None,
        });
    }
}

fn import_symbols(node: &TsNode, source: &str, language: Language) -> Vec<ImportSymbol> {
    let Some(module_path) = import_module_path(node, source, language) else {
        return Vec::new();
    };

    match language {
        // === JavaScript/TypeScript family ===
        Language::JavaScript | Language::Jsx | Language::TypeScript | Language::Tsx => {
            let mut imports = Vec::new();
            if let Some(clause) = node
                .children(&mut node.walk())
                .find(|c| c.kind() == "import_clause")
            {
                collect_import_symbols(clause, source, &module_path, &mut imports);
            }

            if imports.is_empty() {
                imports.push(ImportSymbol {
                    local_name: module_path.clone(),
                    module_path,
                    export_name: None,
                });
            }
            imports
        }

        // === Rust ===
        Language::Rust => {
            let original_name = module_path
                .rsplit("::")
                .next()
                .unwrap_or(&module_path)
                .to_string();
            let alias = rust_use_alias(node, source);

            vec![ImportSymbol {
                local_name: alias.clone().unwrap_or_else(|| original_name.clone()),
                module_path,
                export_name: alias.map(|_| original_name),
            }]
        }

        // === Python: from X import Y, Z ===
        Language::Python => {
            let mut imports = Vec::new();
            let import_name = node
                .child_by_field_name("name")
                .or_else(|| node.child_by_field_name("alias"))
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                .map(|s| s.to_string());

            if let Some(name) = import_name {
                imports.push(ImportSymbol {
                    local_name: name.clone(),
                    module_path,
                    export_name: Some(name),
                });
            } else {
                // Fallback for plain imports
                imports.push(ImportSymbol {
                    local_name: module_path.clone(),
                    module_path,
                    export_name: None,
                });
            }
            imports
        }

        // === Go ===
        Language::Go => {
            let mut imports = Vec::new();
            let alias = node
                .child_by_field_name("alias")
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                .map(|s| s.to_string());

            imports.push(ImportSymbol {
                local_name: alias.clone().unwrap_or_else(|| {
                    module_path
                        .rsplit('/')
                        .next()
                        .unwrap_or(&module_path)
                        .to_string()
                }),
                module_path,
                export_name: alias,
            });
            imports
        }

        // === Java ===
        Language::Java => {
            let last_part = module_path
                .rsplit('.')
                .next()
                .unwrap_or(&module_path)
                .to_string();
            vec![ImportSymbol {
                local_name: last_part.clone(),
                module_path,
                export_name: Some(last_part),
            }]
        }

        // === C/C++ ===
        Language::C | Language::Cpp => {
            let name = module_path
                .trim_end_matches(".h")
                .trim_end_matches(".hpp")
                .split('/')
                .next_back()
                .unwrap_or(&module_path)
                .to_string();
            vec![ImportSymbol {
                local_name: name.clone(),
                module_path,
                export_name: Some(name),
            }]
        }

        // === C# ===
        Language::CSharp => {
            let last_dot = module_path.rfind('.').unwrap_or(0);
            let name = if last_dot > 0 {
                module_path[last_dot + 1..].to_string()
            } else {
                module_path.clone()
            };
            vec![ImportSymbol {
                local_name: name.clone(),
                module_path,
                export_name: Some(name),
            }]
        }

        // === PHP ===
        Language::Php => {
            let last_part = module_path
                .rsplit('\\')
                .next()
                .unwrap_or(&module_path)
                .to_string();
            vec![ImportSymbol {
                local_name: last_part.clone(),
                module_path,
                export_name: Some(last_part),
            }]
        }

        // === Ruby ===
        Language::Ruby => {
            let name = module_path
                .trim_end_matches(".rb")
                .split('/')
                .next_back()
                .unwrap_or(&module_path)
                .to_string();
            vec![ImportSymbol {
                local_name: name.clone(),
                module_path,
                export_name: Some(name),
            }]
        }

        // === Swift ===
        // `import Foundation` — the local name is the imported module's own
        // name, which is the last path component of the dotted import path.
        Language::Swift => {
            let last_part = module_path
                .rsplit('.')
                .next()
                .unwrap_or(&module_path)
                .to_string();
            vec![ImportSymbol {
                local_name: last_part.clone(),
                module_path,
                export_name: Some(last_part),
            }]
        }

        // === Kotlin ===
        // `import com.example.other.Helper` — the local name is the final
        // segment of the qualified identifier.
        Language::Kotlin => {
            let last_part = module_path
                .rsplit('.')
                .next()
                .unwrap_or(&module_path)
                .to_string();
            vec![ImportSymbol {
                local_name: last_part.clone(),
                module_path,
                export_name: Some(last_part),
            }]
        }

        // === Bash ===
        Language::Bash => {
            let name = module_path
                .trim_end_matches(".sh")
                .split('/')
                .next_back()
                .unwrap_or(&module_path)
                .to_string();
            vec![ImportSymbol {
                local_name: name.clone(),
                module_path,
                export_name: Some(name),
            }]
        }

        // === Lua ===
        Language::Lua => {
            vec![ImportSymbol {
                local_name: module_path.clone(),
                module_path,
                export_name: None,
            }]
        }

        // === Elixir ===
        Language::Elixir => {
            vec![ImportSymbol {
                local_name: module_path.clone(),
                module_path,
                export_name: None,
            }]
        }

        // === Erlang ===
        Language::Erlang => {
            vec![ImportSymbol {
                local_name: module_path.clone(),
                module_path,
                export_name: None,
            }]
        }

        // === Haskell ===
        Language::Haskell => {
            vec![ImportSymbol {
                local_name: module_path.clone(),
                module_path,
                export_name: None,
            }]
        }

        // === Scala ===
        Language::Scala => {
            let last_part = module_path
                .rsplit('.')
                .next()
                .unwrap_or(&module_path)
                .to_string();
            vec![ImportSymbol {
                local_name: last_part.clone(),
                module_path,
                export_name: Some(last_part),
            }]
        }

        // === Groovy ===
        Language::Groovy => {
            let last_part = module_path
                .rsplit('.')
                .next()
                .unwrap_or(&module_path)
                .to_string();
            vec![ImportSymbol {
                local_name: last_part.clone(),
                module_path,
                export_name: Some(last_part),
            }]
        }

        // === Dart ===
        Language::Dart => {
            let name = module_path
                .trim_start_matches("package:")
                .split('/')
                .next_back()
                .unwrap_or(&module_path)
                .trim_end_matches(".dart")
                .to_string();
            vec![ImportSymbol {
                local_name: name.clone(),
                module_path,
                export_name: Some(name),
            }]
        }

        // === Julia ===
        Language::Julia => {
            vec![ImportSymbol {
                local_name: module_path.clone(),
                module_path,
                export_name: None,
            }]
        }

        // === Nix ===
        Language::Nix => {
            vec![ImportSymbol {
                local_name: module_path.clone(),
                module_path,
                export_name: None,
            }]
        }

        // === R ===
        Language::R => {
            vec![ImportSymbol {
                local_name: module_path.clone(),
                module_path,
                export_name: None,
            }]
        }

        // === MATLAB ===
        Language::Matlab => {
            vec![ImportSymbol {
                local_name: module_path.clone(),
                module_path,
                export_name: None,
            }]
        }

        // === Fortran ===
        Language::Fortran => {
            vec![ImportSymbol {
                local_name: module_path.clone(),
                module_path,
                export_name: None,
            }]
        }

        // === Elm ===
        Language::Elm => {
            vec![ImportSymbol {
                local_name: module_path.clone(),
                module_path,
                export_name: None,
            }]
        }

        // === Perl ===
        Language::Perl => {
            vec![ImportSymbol {
                local_name: module_path.clone(),
                module_path,
                export_name: None,
            }]
        }

        // === PowerShell ===
        Language::Powershell => {
            vec![ImportSymbol {
                local_name: module_path.clone(),
                module_path,
                export_name: None,
            }]
        }

        // === Zig ===
        Language::Zig => {
            vec![ImportSymbol {
                local_name: module_path.clone(),
                module_path,
                export_name: None,
            }]
        }

        // === Blazor, Markup (no imports) ===
        Language::Blazor
        | Language::Markdown
        | Language::Toml
        | Language::Yaml
        | Language::Liquid
        | Language::Unknown => Vec::new(),
    }
}

/// Recover the module path of an import statement.
///
/// The field used to reach the path differs per grammar, and several of the
/// old field names did not exist at all (`CSharp` used `qualified_name`,
/// `Kotlin` used `type`, `Swift` used `module_name`, `Java` used `name`).
/// The names below are validated against each grammar by
/// `tests/grammar_contract_test.rs`.
fn import_module_path(node: &TsNode, source: &str, language: Language) -> Option<String> {
    let source_bytes = source.as_bytes();

    // Languages whose import path lives in a named field.
    let field: Option<&str> = match language {
        Language::Rust => Some("argument"),
        Language::JavaScript | Language::Jsx | Language::TypeScript | Language::Tsx => {
            Some("source")
        }
        Language::Python => Some("module_name"),
        Language::Java => None,
        Language::C | Language::Cpp => Some("path"),
        Language::CSharp => None,
        Language::Php => Some("name"),
        Language::Ruby => None,
        Language::Bash => Some("argument"),
        Language::Lua => Some("argument"),
        Language::Elixir => Some("module"),
        Language::Erlang => Some("name"),
        Language::Haskell => Some("module"),
        Language::Scala => Some("path"),
        Language::Groovy => Some("name"),
        Language::Dart => Some("uri"),
        // Julia and Fortran keep the imported module in a child node, not in
        // a field; they are handled in the `None` branch below.
        Language::Julia => None,
        Language::Nix => Some("source"),
        Language::R => Some("argument"),
        Language::Matlab => Some("argument"),
        Language::Fortran => None,
        Language::Elm => Some("module_name"),
        Language::Perl => Some("module"),
        Language::Powershell => Some("name"),
        Language::Zig => Some("path"),
        // These resolve their path from a child node rather than a field.
        Language::Go | Language::Swift | Language::Kotlin => None,
        Language::Markdown
        | Language::Toml
        | Language::Yaml
        | Language::Liquid
        | Language::Blazor
        | Language::Unknown => None,
    };

    let child = if let Some(field) = field {
        node.child_by_field_name(field).or_else(|| {
            // Fallback: get first string-like child
            node.children(&mut node.walk())
                .find(|c| matches!(c.kind(), "string" | "identifier" | "scoped_identifier"))
        })?
    } else {
        match language {
            // Java: `import_declaration` has no fields; the path is in a
            // `scoped_identifier` child (or a bare `identifier` for
            // single-segment imports).
            Language::Java => {
                let mut cursor = node.walk();
                node.named_children(&mut cursor)
                    .find(|c| c.kind() == "scoped_identifier")
                    .or_else(|| {
                        let mut cursor = node.walk();
                        node.named_children(&mut cursor)
                            .find(|c| c.kind() == "identifier")
                    })?
            }
            // Go: `import_declaration` → `import_spec` → `path`.
            Language::Go => {
                let mut cursor = node.walk();
                let spec = node
                    .named_children(&mut cursor)
                    .find(|c| c.kind() == "import_spec")
                    .or_else(|| {
                        let mut cursor = node.walk();
                        node.named_children(&mut cursor)
                            .find(|c| c.kind() == "import_spec_list")
                            .and_then(|l| {
                                let mut inner = l.walk();
                                l.named_children(&mut inner)
                                    .find(|c| c.kind() == "import_spec")
                            })
                    })?;
                spec.child_by_field_name("path")?
            }
            // Swift: `import_declaration` → `identifier` → `simple_identifier`.
            Language::Swift => {
                let mut cursor = node.walk();
                node.named_children(&mut cursor)
                    .find(|c| c.kind() == "identifier")?
            }
            // C#: `using_directive` → `qualified_name` child. A `using
            // static` directive names a member instead: `using static
            // System.Math;`.
            Language::CSharp => {
                let mut cursor = node.walk();
                node.named_children(&mut cursor)
                    .find(|c| c.kind() == "qualified_name")?
            }
            // Kotlin: `import` → `qualified_identifier` (or a bare `identifier`).
            Language::Kotlin => {
                let mut cursor = node.walk();
                node.named_children(&mut cursor)
                    .find(|c| c.kind() == "qualified_identifier")
                    .or_else(|| {
                        let mut cursor = node.walk();
                        node.named_children(&mut cursor)
                            .find(|c| c.kind() == "identifier")
                    })?
            }
            // Julia: `using LinearAlgebra` and `import Base: show` — the
            // module is the `identifier` (or `scoped_identifier`) child, and
            // a selective import keeps it in the first identifier of the
            // `selected_import`.
            Language::Julia => {
                let mut cursor = node.walk();
                let child = node.named_children(&mut cursor).find(|c| {
                    matches!(
                        c.kind(),
                        "scoped_identifier" | "identifier" | "selected_import"
                    )
                })?;
                if child.kind() == "selected_import" {
                    let mut inner = child.walk();
                    child
                        .named_children(&mut inner)
                        .find(|c| c.kind() == "identifier")?
                } else {
                    child
                }
            }
            // Fortran: `use iso_fortran_env, only: real64` — the module is
            // the `module_name` child.
            Language::Fortran => {
                let mut cursor = node.walk();
                node.named_children(&mut cursor)
                    .find(|c| c.kind() == "module_name")?
            }
            _ => return None,
        }
    };

    let raw = child.utf8_text(source_bytes).ok()?.trim().to_string();
    let trimmed = raw
        .trim_matches(['"', '\'', '`'].as_ref())
        .trim()
        .to_string();

    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn collect_import_symbols(
    node: TsNode,
    source: &str,
    module_path: &str,
    imports: &mut Vec<ImportSymbol>,
) {
    for child in node.children(&mut node.walk()) {
        match child.kind() {
            "identifier" => {
                if let Ok(text) = child.utf8_text(source.as_bytes()) {
                    imports.push(ImportSymbol {
                        local_name: text.to_string(),
                        module_path: module_path.to_string(),
                        export_name: None,
                    });
                }
            }
            "namespace_import" => {
                let name = child
                    .child_by_field_name("name")
                    .or_else(|| child.child_by_field_name("alias"))
                    .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                    .map(|s| s.to_string());
                if let Some(name) = name {
                    imports.push(ImportSymbol {
                        local_name: name,
                        module_path: module_path.to_string(),
                        export_name: None,
                    });
                }
            }
            "named_imports" => collect_named_imports(child, source, module_path, imports),
            "import_specifier" => collect_import_specifier(child, source, module_path, imports),
            _ => {}
        }
    }
}

fn collect_named_imports(
    node: TsNode,
    source: &str,
    module_path: &str,
    imports: &mut Vec<ImportSymbol>,
) {
    for child in node.children(&mut node.walk()) {
        if child.kind() == "import_specifier" {
            collect_import_specifier(child, source, module_path, imports);
        }
    }
}

fn collect_import_specifier(
    node: TsNode,
    source: &str,
    module_path: &str,
    imports: &mut Vec<ImportSymbol>,
) {
    let export_name = node
        .child_by_field_name("name")
        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
        .map(|s| s.to_string());
    let alias = node
        .child_by_field_name("alias")
        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
        .map(|s| s.to_string());

    if let Some(export_name) = export_name {
        let local_name = alias.clone().unwrap_or_else(|| export_name.clone());
        imports.push(ImportSymbol {
            local_name,
            module_path: module_path.to_string(),
            export_name: Some(export_name),
        });
    }
}

fn add_module_node(
    node: &TsNode,
    source: &str,
    project_root: &Path,
    language: Language,
    file_path: &str,
    parent_id: String,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    now_ms: i64,
) {
    let Some(name) = module_name(node, source, language) else {
        return;
    };

    let start = node.start_position();
    let end = node.end_position();
    let qualified_name = format!("{}::{}", file_path, name);
    let id = node_id_for_symbol(
        file_path,
        "module",
        &qualified_name,
        start.row as i64 + 1,
        start.column as i64,
    );

    let signature = match language {
        Language::Rust => rust_module_target(project_root, file_path, &name),
        _ => None,
    };

    let (visibility, is_exported) = match read_declaration_visibility(*node, language, source) {
        Some((vis, exported)) => (Some(vis), exported),
        None => (None, false),
    };

    nodes.push(Node {
        id: id.clone(),
        kind: NodeKind::Module,
        name,
        qualified_name,
        file_path: file_path.to_string(),
        language,
        start_line: start.row as i64 + 1,
        end_line: end.row as i64 + 1,
        start_column: start.column as i64,
        end_column: end.column as i64,
        docstring: None,
        signature,
        visibility,
        is_exported,
        is_async: false,
        is_static: false,
        is_abstract: false,
        decorators: None,
        type_parameters: None,
        updated_at: now_ms,
        cluster_id: None,
    });

    edges.push(Edge {
        source: parent_id.clone(),
        target: id.clone(),
        kind: EdgeKind::Contains,
        metadata: None,
        line: Some(start.row as i64 + 1),
        column: Some(start.column as i64),
        confidence: 1.0,
        process_id: None,
    });
}

fn module_name(node: &TsNode, source: &str, language: Language) -> Option<String> {
    match language {
        Language::Rust => node
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
            .map(|s| s.to_string()),
        _ => None,
    }
}

fn rust_module_target(project_root: &Path, file_path: &str, name: &str) -> Option<String> {
    let base_dir = Path::new(file_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let candidate_file = base_dir.join(format!("{name}.rs"));
    let candidate_mod = base_dir.join(name).join("mod.rs");

    if project_root.join(&candidate_file).is_file() {
        Some(candidate_file.to_string_lossy().to_string())
    } else if project_root.join(&candidate_mod).is_file() {
        Some(candidate_mod.to_string_lossy().to_string())
    } else {
        None
    }
}

fn add_export_nodes(
    node: &TsNode,
    source: &str,
    language: Language,
    file_path: &str,
    parent_id: String,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    now_ms: i64,
) {
    let exports = export_symbols(node, source, language);
    if exports.is_empty() {
        return;
    }

    let start = node.start_position();
    let end = node.end_position();

    for export in exports {
        let qualified_name = format!("{}::export::{}", file_path, export.name);
        let id = node_id_for_symbol(
            file_path,
            "export",
            &qualified_name,
            start.row as i64 + 1,
            start.column as i64,
        );

        nodes.push(Node {
            id: id.clone(),
            kind: NodeKind::Export,
            name: export.name,
            qualified_name,
            file_path: file_path.to_string(),
            language,
            start_line: start.row as i64 + 1,
            end_line: end.row as i64 + 1,
            start_column: start.column as i64,
            end_column: end.column as i64,
            docstring: None,
            signature: export.module_path,
            visibility: None,
            is_exported: true,
            is_async: false,
            is_static: false,
            is_abstract: false,
            decorators: None,
            type_parameters: None,
            updated_at: now_ms,
            cluster_id: None,
        });

        edges.push(Edge {
            source: parent_id.clone(),
            target: id.clone(),
            kind: EdgeKind::Contains,
            metadata: None,
            line: Some(start.row as i64 + 1),
            column: Some(start.column as i64),
            confidence: 1.0,
            process_id: None,
        });
        edges.push(Edge {
            source: parent_id.clone(),
            target: id,
            kind: EdgeKind::Exports,
            metadata: None,
            line: Some(start.row as i64 + 1),
            column: Some(start.column as i64),
            confidence: 1.0,
            process_id: None,
        });
    }
}

fn export_symbols(node: &TsNode, source: &str, language: Language) -> Vec<ExportSymbol> {
    match language {
        // === JavaScript/TypeScript family ===
        Language::JavaScript | Language::Jsx | Language::TypeScript | Language::Tsx => {
            let module_path = export_module_path(node, source);
            let mut names = Vec::new();
            collect_export_names(*node, source, &mut names);

            if names.is_empty() {
                return Vec::new();
            }

            names
                .into_iter()
                .map(|name| ExportSymbol {
                    name,
                    module_path: module_path.clone(),
                })
                .collect()
        }

        // === Rust ===
        Language::Rust => {
            let Some(path) = rust_use_path(node, source) else {
                return Vec::new();
            };
            let name = path.rsplit("::").next().unwrap_or(&path).to_string();
            vec![ExportSymbol {
                name,
                module_path: Some(path),
            }]
        }

        // === Python: explicit __all__ or all public names ===
        Language::Python => {
            // Python doesn't use explicit export statements; all public names are exported
            // This would be handled at the symbol level during extraction
            Vec::new()
        }

        // === Go: capitalized identifiers are exported ===
        Language::Go => Vec::new(),

        // === Java: public modifier on class declaration ===
        Language::Java => {
            // Java exports are handled via method visibility
            Vec::new()
        }

        // === C/C++: header file declarations ===
        Language::C | Language::Cpp => {
            // C/C++ exports are implicit via header files
            Vec::new()
        }

        // === C#: public modifier ===
        Language::CSharp => Vec::new(),

        // === PHP: global namespace ===
        Language::Php => Vec::new(),

        // === Ruby: implicit (all top-level) ===
        Language::Ruby => Vec::new(),

        // === Swift: public modifier ===
        Language::Swift => Vec::new(),

        // === Kotlin: implicit (all top-level unless private) ===
        Language::Kotlin => Vec::new(),

        // === Bash: export statement ===
        Language::Bash => {
            if node.kind() == "command" {
                let cmd_text = node.utf8_text(source.as_bytes()).ok().unwrap_or("");
                if cmd_text.starts_with("export ") {
                    let var_name = cmd_text
                        .strip_prefix("export ")
                        .and_then(|s| s.split('=').next())
                        .map(|s| s.trim().to_string());

                    if let Some(name) = var_name {
                        return vec![ExportSymbol {
                            name,
                            module_path: None,
                        }];
                    }
                }
            }
            Vec::new()
        }

        // === Lua: implicit return/assignment ===
        Language::Lua => Vec::new(),

        // === Elixir: defmodule defines exports ===
        Language::Elixir => Vec::new(),

        // === Erlang: -export directive ===
        Language::Erlang => {
            if node.kind() == "attribute" {
                let attr_text = node.utf8_text(source.as_bytes()).ok().unwrap_or("");
                if attr_text.contains("export") {
                    // Parse `-export([func/arity]).`
                    let exports_str = attr_text
                        .split('[')
                        .nth(1)
                        .and_then(|s| s.split(']').next())
                        .unwrap_or("");

                    let names: Vec<ExportSymbol> = exports_str
                        .split(',')
                        .filter_map(|exp| {
                            let func_name = exp.split('/').next().map(|s| s.trim().to_string())?;
                            Some(ExportSymbol {
                                name: func_name,
                                module_path: None,
                            })
                        })
                        .collect();

                    return names;
                }
            }
            Vec::new()
        }

        // === Haskell: module declaration with export list ===
        Language::Haskell => Vec::new(),

        // === Scala: implicit (all top-level unless private) ===
        Language::Scala => Vec::new(),

        // === Groovy: implicit ===
        Language::Groovy => Vec::new(),

        // === Dart: explicit export ===
        Language::Dart => {
            if node.kind() == "import_or_export_statement" {
                let stmt_text = node.utf8_text(source.as_bytes()).unwrap_or("");
                if stmt_text.starts_with("export ") {
                    if let Some(uri) = stmt_text
                        .strip_prefix("export ")
                        .and_then(|s| s.split(['\'', '"']).nth(1))
                        .map(|s| s.to_string())
                    {
                        return vec![ExportSymbol {
                            name: uri.clone(),
                            module_path: Some(uri),
                        }];
                    }
                }
            }
            Vec::new()
        }

        // === Julia: `export greet, other` ===
        Language::Julia => {
            if node.kind() == "export_statement" {
                return identifier_exports(node, source);
            }
            Vec::new()
        }

        // === Nix: implicit (let-in returns) ===
        Language::Nix => Vec::new(),

        // === R: implicit (global assignment) ===
        Language::R => Vec::new(),

        // === MATLAB: implicit (global scope) ===
        Language::Matlab => Vec::new(),

        // === Fortran: `public :: area` ===
        Language::Fortran => {
            if node.kind() == "public_statement" {
                return identifier_exports(node, source);
            }
            Vec::new()
        }

        // === Elm: module declaration with export list ===
        Language::Elm => Vec::new(),

        // === Perl: @EXPORT, @EXPORT_OK ===
        Language::Perl => {
            // The grammar calls an assignment an `assignment_expression`;
            // `our @EXPORT_OK = qw(...)` is one.
            if node.kind() == "assignment_expression" {
                let assign_text = node.utf8_text(source.as_bytes()).ok().unwrap_or("");
                if assign_text.contains("@EXPORT") {
                    // Parse @EXPORT = qw(func1 func2 ...)
                    let funcs_str = assign_text
                        .split('(')
                        .nth(1)
                        .and_then(|s| s.split(')').next())
                        .unwrap_or("");

                    let names: Vec<ExportSymbol> = funcs_str
                        .split_whitespace()
                        .map(|func| ExportSymbol {
                            name: func.to_string(),
                            module_path: None,
                        })
                        .collect();

                    return names;
                }
            }
            Vec::new()
        }

        // === PowerShell: implicit (function names) ===
        Language::Powershell => Vec::new(),

        // === Zig: implicit (public by default) ===
        Language::Zig => Vec::new(),

        // === Blazor, Markup (no exports) ===
        Language::Blazor
        | Language::Markdown
        | Language::Toml
        | Language::Yaml
        | Language::Liquid
        | Language::Unknown => Vec::new(),
    }
}

fn rust_use_path(node: &TsNode, source: &str) -> Option<String> {
    let child = node.child_by_field_name("path")?;
    let raw = child.utf8_text(source.as_bytes()).ok()?.trim().to_string();
    if raw.is_empty() { None } else { Some(raw) }
}

/// Collect the names a declaration exports, listed as plain identifiers.
///
/// Julia's `export greet, other` and Fortran's `public :: area` both spell
/// the exported names as identifiers under the statement.
fn identifier_exports(node: &TsNode, source: &str) -> Vec<ExportSymbol> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|c| c.kind() == "identifier")
        .filter_map(|c| c.utf8_text(source.as_bytes()).ok())
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .map(|name| ExportSymbol {
            name,
            module_path: None,
        })
        .collect()
}

fn rust_use_alias(node: &TsNode, source: &str) -> Option<String> {
    node.child_by_field_name("alias")
        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
        .map(|s| s.to_string())
}

fn export_module_path(node: &TsNode, source: &str) -> Option<String> {
    let child = node.child_by_field_name("source")?;
    let raw = child.utf8_text(source.as_bytes()).ok()?.trim().to_string();
    let trimmed = raw.trim_matches(['"', '\''].as_ref()).to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn collect_export_names(node: TsNode, source: &str, names: &mut Vec<String>) {
    if node.kind() == "export_specifier" {
        let alias = node
            .child_by_field_name("alias")
            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
            .map(|s| s.to_string());
        let name = alias.or_else(|| {
            node.child_by_field_name("name")
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                .map(|s| s.to_string())
        });
        if let Some(name) = name {
            names.push(name);
        }
        return;
    }

    if matches!(
        node.kind(),
        "function_declaration"
            | "class_declaration"
            | "interface_declaration"
            | "type_alias_declaration"
            | "enum_declaration"
            | "variable_declarator"
    ) {
        let name = node
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
            .map(|s| s.to_string());
        if let Some(name) = name {
            names.push(name);
        }
        return;
    }

    for child in node.children(&mut node.walk()) {
        collect_export_names(child, source, names);
    }
}

fn build_import_signature(module_path: &str, export_name: Option<&str>) -> String {
    match export_name {
        Some(name) => format!("{module_path}|export={name}"),
        None => module_path.to_string(),
    }
}

fn node_key(kind: NodeKind, start: tree_sitter::Point, name: &str) -> String {
    format!("{:?}:{}:{}:{}", kind, start.row, start.column, name)
}

fn is_callable_kind(kind: NodeKind) -> bool {
    matches!(kind, NodeKind::Function | NodeKind::Method)
}

/// Return `true` when `kind` is a call site in `language`.
///
/// The set of call kinds is declared per language in
/// [`crate::grammar`], so a grammar change shows up as a failing contract
/// test rather than as silently missing call edges.
fn is_call_expression(kind: &str, language: Language) -> bool {
    grammar::is_call_kind(language, kind)
}

/// Recover the callee name of a call site, plus the qualifier (receiver) when
/// the call is qualified.
///
/// For `Helper.doThing()` the returned name is `doThing` and the qualifier is
/// `Some("Helper")`. The qualifier is what lets the resolver match a call
/// against the local name of an import, which is how cross-package calls
/// resolve without reintroducing the name-collision false positives from #43.
fn call_name(node: &TsNode, source: &str, language: Language) -> Option<(String, Option<String>)> {
    let source_bytes = source.as_bytes();

    // Haskell's applications nest: `f a b` parses as
    // `apply(apply(f, a), b)`, so the callee of an application is the
    // leftmost function in the chain rather than the text of the
    // `function` field.
    if language == Language::Haskell {
        return haskell_call_name(node, source_bytes);
    }

    // Julia: a declaration's signature parses as a call — `function area(x)`
    // is a `call_expression` — and the short form `area(x) = ...` is an
    // assignment whose left-hand side is a call. Neither is a call site,
    // and recording them would give every Julia function a self-call.
    if language == Language::Julia {
        if let Some(parent) = node.parent() {
            if parent.kind() == "signature" {
                return None;
            }
            if parent.kind() == "assignment" {
                let mut cursor = parent.walk();
                let is_left_hand_side = parent.named_children(&mut cursor).next() == Some(*node);
                if is_left_hand_side {
                    return None;
                }
            }
        }
    }

    let callee_strategy = grammar::callee_source(language, node.kind())?;

    let callee = match callee_strategy {
        grammar::CalleeSource::Field { callee, .. } => node.child_by_field_name(callee),
        grammar::CalleeSource::FirstChildLastIdentifier => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor).next()
        }
    }?;

    let (name, qualifier) = match callee_strategy {
        grammar::CalleeSource::Field { receiver, .. } => {
            let raw = callee.utf8_text(source_bytes).ok()?.trim().to_string();
            if raw.is_empty() {
                return None;
            }

            // Some grammars split the receiver into its own field on the call
            // node (Java's `object`, PHP's `object`/`scope`).
            let qualifier = receiver.and_then(|field| {
                node.child_by_field_name(field)
                    .and_then(|n| n.utf8_text(source_bytes).ok())
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .and_then(|t| {
                        t.rsplit(['.', ':', '>'])
                            .next()
                            .unwrap_or(&t)
                            .trim()
                            .to_string()
                            .into()
                    })
                    .filter(|t: &String| !t.is_empty())
            });

            let (text_qualifier, name) = split_qualified_name(&raw);
            (name, qualifier.or(text_qualifier))
        }
        grammar::CalleeSource::FirstChildLastIdentifier => {
            // Kotlin and Swift expose the callee as the first named child. For
            // a qualified call that child is a `navigation_expression`; the
            // method name is its *last* identifier and the receiver is the
            // first one.
            let (qualifier, name) = first_child_last_identifier(&callee, source_bytes);
            (name, qualifier)
        }
    };

    if name.is_empty() {
        None
    } else {
        Some((name, qualifier))
    }
}

/// Recover the callee of a Haskell application.
///
/// `f a b` parses as `apply(apply(f, a), b)`, so the `function` field of
/// the outer application is another application rather than the callee. The
/// callee is found by walking down the chain to the leftmost function, and
/// a qualified name (`Data.List.sortBy`) keeps its module as the qualifier.
fn haskell_call_name(node: &TsNode, source_bytes: &[u8]) -> Option<(String, Option<String>)> {
    let mut current = node.child_by_field_name("function")?;
    loop {
        match current.kind() {
            "apply" => {
                current = current.child_by_field_name("function")?;
            }
            "qualified" => {
                // `Data.List.sortBy` — the name is the `id` field and the
                // module is the `module` field, which keeps the trailing
                // dot of the qualified name.
                let name = current
                    .child_by_field_name("id")
                    .and_then(|n| n.utf8_text(source_bytes).ok())
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())?;
                let qualifier = current
                    .child_by_field_name("module")
                    .and_then(|n| n.utf8_text(source_bytes).ok())
                    .map(|t| t.trim().trim_end_matches('.').to_string())
                    .filter(|t| !t.is_empty());
                return Some((name, qualifier));
            }
            _ => {
                let name = current.utf8_text(source_bytes).ok()?.trim().to_string();
                if name.is_empty() {
                    return None;
                }
                return Some((name, None));
            }
        }
    }
}

/// Recover the supertypes a declaration inherits from or implements.
///
/// Returns `(name, kind)` pairs. Names are the *simple* type names so they
/// can be matched against graph nodes by name; a supertype that is not
/// present in the project is left unresolved and simply produces no edge.
fn heritage_names(
    node: &TsNode,
    source: &str,
    language: Language,
) -> Vec<(String, grammar::HeritageKind)> {
    let Some(contract) = grammar::contract_for(language) else {
        return Vec::new();
    };
    let Some(rules) = contract.heritage else {
        return Vec::new();
    };

    // Only declarations with a heritage clause participate.
    let is_declaration = contract.kinds.iter().any(|(kind, node_kind, _)| {
        *kind == node.kind()
            && matches!(
                node_kind,
                NodeKind::Class
                    | NodeKind::Struct
                    | NodeKind::Interface
                    | NodeKind::Trait
                    | NodeKind::Protocol
                    | NodeKind::Enum
            )
    });
    if !is_declaration {
        return Vec::new();
    }

    let mut found: Vec<(String, grammar::HeritageKind)> = Vec::new();
    let mut push = |name: String, kind: grammar::HeritageKind| {
        if !name.is_empty() && !found.iter().any(|(n, _)| *n == name) {
            found.push((name, kind));
        }
    };

    for rule in rules {
        match rule.source {
            grammar::HeritageSource::Field(field) => {
                // Rust's `impl Trait for Type` — the `type` field is the
                // subject and the `trait` field is what it implements.
                if language == Language::Rust {
                    if let Some(trait_node) = node.child_by_field_name("trait") {
                        if let Some(name) = simple_type_name(&trait_node, source) {
                            push(name, grammar::HeritageKind::Implements);
                        }
                    }
                }

                if let Some(value) = node.child_by_field_name(field) {
                    collect_type_names(&value, source, rule.kind, &mut push);
                }
            }
            grammar::HeritageSource::ChildField(child_kind, field) => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if child.kind() != child_kind {
                        continue;
                    }
                    let Some(value) = child.child_by_field_name(field) else {
                        continue;
                    };
                    collect_type_names(&value, source, rule.kind, &mut push);
                }
            }
            grammar::HeritageSource::ChildIsType(child_kind) => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if child.kind() != child_kind {
                        continue;
                    }
                    // C#'s `base_list` and PHP's `base_clause` list the types
                    // directly as children; TypeScript's `implements_clause`
                    // lists them under a `type` child.
                    let mut inner = child.walk();
                    for entry in child.named_children(&mut inner) {
                        if matches!(entry.kind(), "type" | "identifier" | "name") {
                            if let Some(name) = simple_type_name(&entry, source) {
                                push(name, rule.kind);
                            }
                        }
                    }
                }
            }
            grammar::HeritageSource::ChildInContainer(container, child_kind, nested_kind) => {
                let mut cursor = node.walk();
                for group in node.named_children(&mut cursor) {
                    if group.kind() != container {
                        continue;
                    }
                    let mut group_cursor = group.walk();
                    for child in group.named_children(&mut group_cursor) {
                        if child.kind() != child_kind {
                            continue;
                        }
                        if let Some(name) = find_nested_type_name(&child, nested_kind, source) {
                            push(name, rule.kind);
                        }
                    }
                }
            }
            grammar::HeritageSource::ChildNested(child_kind, nested_kind) => {
                if language == Language::Julia {
                    // Julia: `struct Point <: Shape` — the supertype is the
                    // identifier that follows the `<:` operator, so the
                    // generic "first identifier of the child" search would
                    // find the type's own name instead.
                    if let Some(name) = julia_supertype(node, source) {
                        push(name, rule.kind);
                    }
                    continue;
                }
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if child.kind() != child_kind {
                        continue;
                    }
                    // The type may sit directly under the child (Swift's
                    // `inheritance_specifier` → `user_type`) or one level
                    // deeper (Kotlin's `delegation_specifier` →
                    // `constructor_invocation` → `user_type`).
                    if let Some(name) = find_nested_type_name(&child, nested_kind, source) {
                        push(name, rule.kind);
                    }
                }
            }
        }
    }

    found
}

/// The supertype of a Julia type declaration.
///
/// `struct Point <: Shape` keeps the supertype after the `<:` operator
/// inside the `type_head`; a type without a supertype has no `<:` at all,
/// so there is nothing to report.
fn julia_supertype(node: &TsNode, source: &str) -> Option<String> {
    if !matches!(
        node.kind(),
        "struct_definition" | "abstract_definition" | "primitive_definition"
    ) {
        return None;
    }

    let mut cursor = node.walk();
    let head = node
        .named_children(&mut cursor)
        .find(|c| c.kind() == "type_head")?;

    // The supertype is the operand that follows the `<:` operator inside
    // the `type_head`'s expression.
    let mut inner = head.walk();
    let expression = head
        .named_children(&mut inner)
        .find(|c| c.kind() == "binary_expression")?;

    let mut expression_cursor = expression.walk();
    let children: Vec<TsNode> = expression.named_children(&mut expression_cursor).collect();
    let operator_at = children.iter().position(|c| {
        c.kind() == "operator"
            && c.utf8_text(source.as_bytes())
                .is_ok_and(|text| text.trim() == "<:")
    })?;

    simple_type_name(children.get(operator_at + 1)?, source)
}

/// Find the type named by a descendant of the given kind, searching at
/// most two levels deep.
fn find_nested_type_name(node: &TsNode, nested_kind: &str, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == nested_kind {
            return simple_type_name(&child, source);
        }
        let mut inner = child.walk();
        for grandchild in child.named_children(&mut inner) {
            if grandchild.kind() == nested_kind {
                return simple_type_name(&grandchild, source);
            }
        }
    }
    None
}

/// Collect every type name mentioned under `node`, tagging each with
/// `kind`.
fn collect_type_names(
    node: &TsNode,
    source: &str,
    kind: grammar::HeritageKind,
    push: &mut impl FnMut(String, grammar::HeritageKind),
) {
    let node_kind = node.kind();
    if matches!(
        node_kind,
        "type_identifier" | "identifier" | "simple_identifier" | "name" | "user_type"
    ) {
        if let Some(name) = simple_type_name(node, source) {
            push(name, kind);
        }
        return;
    }

    // A `type_list` (`implements Iface, Other`) holds several types.
    if node_kind == "type_list" {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            collect_type_names(&child, source, kind, push);
        }
        return;
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_type_names(&child, source, kind, push);
    }
}

/// Extract the simple (unqualified) name of a type reference.
///
/// `com.example.Base` → `Base`; `Base<T>` → `Base`.
fn simple_type_name(node: &TsNode, source: &str) -> Option<String> {
    let text = node.utf8_text(source.as_bytes()).ok()?.trim().to_string();
    if text.is_empty() {
        return None;
    }

    // Drop generic arguments.
    let without_generics = text.split('<').next().unwrap_or(&text).trim();

    // Take the last path segment.
    let last = without_generics
        .rsplit(['.', ':'])
        .next()
        .unwrap_or(without_generics)
        .trim()
        .trim_end_matches("()")
        .trim();

    if last.is_empty() {
        None
    } else {
        Some(last.to_string())
    }
}

/// Recover the declared type of a local variable, if the node is a variable
/// declaration with a type we can read.
///
/// Returns `(variable_name, type_name)`.
///
/// The type is taken from an explicit annotation when present, otherwise from
/// the constructor the variable is initialised with:
///
/// - Kotlin: `val helper = Circle(2.0)` → `("helper", "Circle")`
/// - Java: `Circle c = new Circle(2.0)` → `("c", "Circle")`
/// - TypeScript: `const helper = new Circle(2.0)` → `("helper", "Circle")`
/// - Python: `helper = Circle(2.0)` → `("helper", "Circle")`
/// - Go: `helper := Circle(2.0)` → `("helper", "Circle")`
/// - Swift: `let helper = Circle(radius: 2.0)` → `("helper", "Circle")`
fn local_variable_type(
    node: &TsNode,
    source: &str,
    language: Language,
) -> Option<(String, String)> {
    let source_bytes = source.as_bytes();
    let text_of = |n: &TsNode| n.utf8_text(source_bytes).ok().map(|s| s.trim().to_string());

    match language {
        // Kotlin: `property_declaration` → `variable_declaration` → `identifier`,
        // with the type either annotated (`val p: Point`) or inferred from
        // the initialiser call (`val helper = Circle(2.0)`).
        Language::Kotlin => {
            if node.kind() != "property_declaration" {
                return None;
            }
            let mut cursor = node.walk();
            let var_decl = node
                .named_children(&mut cursor)
                .find(|c| c.kind() == "variable_declaration")?;

            let mut inner = var_decl.walk();
            let name_node = var_decl
                .named_children(&mut inner)
                .find(|c| c.kind() == "identifier")?;
            let name = text_of(&name_node)?;

            // Explicit annotation: `val p: Point`.
            let mut ann_cursor = var_decl.walk();
            if let Some(user_type) = var_decl
                .named_children(&mut ann_cursor)
                .find(|c| c.kind() == "user_type")
            {
                if let Some(type_name) = simple_type_name(&user_type, source) {
                    return Some((name, type_name));
                }
            }

            // Inferred: `val helper = Circle(2.0)`.
            let mut value_cursor = node.walk();
            if let Some(init) = node
                .named_children(&mut value_cursor)
                .find(|c| c.kind() == "call_expression")
            {
                let mut init_cursor = init.walk();
                if let Some(callee) = init.named_children(&mut init_cursor).next() {
                    if let Some(type_name) = simple_type_name(&callee, source) {
                        return Some((name, type_name));
                    }
                }
            }
            None
        }

        // Java: `local_variable_declaration` → field `type` and a
        // `variable_declarator` with field `name`.
        Language::Java => {
            if node.kind() != "local_variable_declaration" {
                return None;
            }
            let type_node = node.child_by_field_name("type")?;
            let type_name = simple_type_name(&type_node, source)?;
            let mut cursor = node.walk();
            let declarator = node
                .named_children(&mut cursor)
                .find(|c| c.kind() == "variable_declarator")?;
            let name = text_of(&declarator.child_by_field_name("name")?)?;
            Some((name, type_name))
        }

        // TypeScript / JavaScript: `variable_declarator` → field `name`,
        // value is a `new_expression` whose `constructor` is the type.
        Language::TypeScript | Language::Tsx | Language::JavaScript | Language::Jsx => {
            if node.kind() != "variable_declarator" {
                return None;
            }
            let name = text_of(&node.child_by_field_name("name")?)?;
            let value = node.child_by_field_name("value")?;
            if value.kind() != "new_expression" {
                return None;
            }
            let constructor = value.child_by_field_name("constructor")?;
            let type_name = simple_type_name(&constructor, source)?;
            Some((name, type_name))
        }

        // Python: `assignment` → field `left` and field `right`; the right
        // side is a `call` whose `function` is the type.
        Language::Python => {
            if node.kind() != "assignment" {
                return None;
            }
            let left = node.child_by_field_name("left")?;
            if left.kind() != "identifier" {
                return None;
            }
            let name = text_of(&left)?;
            let right = node.child_by_field_name("right")?;
            if right.kind() != "call" {
                return None;
            }
            let function = right.child_by_field_name("function")?;
            let type_name = simple_type_name(&function, source)?;
            Some((name, type_name))
        }

        // Go: `short_var_declaration` → field `left` and field `right`.
        Language::Go => {
            if node.kind() != "short_var_declaration" {
                return None;
            }
            let left = node.child_by_field_name("left")?;
            let mut cursor = left.walk();
            let name_node = left.named_children(&mut cursor).next()?;
            let name = text_of(&name_node)?;
            let right = node.child_by_field_name("right")?;
            let mut right_cursor = right.walk();
            let callee = right.named_children(&mut right_cursor).next()?;
            let type_name = simple_type_name(&callee, source)?;
            Some((name, type_name))
        }

        // Swift: `property_declaration` → field `name` (the pattern) and
        // field `value` (the initialiser call).
        Language::Swift => {
            if node.kind() != "property_declaration" {
                return None;
            }
            let name = text_of(&node.child_by_field_name("name")?)?;
            let value = node.child_by_field_name("value")?;
            if value.kind() != "call_expression" {
                return None;
            }
            let mut cursor = value.walk();
            let callee = value.named_children(&mut cursor).next()?;
            let type_name = simple_type_name(&callee, source)?;
            Some((name, type_name))
        }

        // Ruby: `helper = Helper.new` → `("helper", "Helper")`.
        Language::Ruby => {
            if node.kind() != "assignment" {
                return None;
            }
            let left = node.child_by_field_name("left")?;
            if left.kind() != "identifier" {
                return None;
            }
            let name = text_of(&left)?;
            let right = node.child_by_field_name("right")?;
            if right.kind() != "call" {
                return None;
            }
            // `Helper.new` — the receiver names the type.
            let method = right.child_by_field_name("method")?;
            if text_of(&method).as_deref() != Some("new") {
                return None;
            }
            let receiver = right.child_by_field_name("receiver")?;
            let type_name = simple_type_name(&receiver, source)?;
            Some((name, type_name))
        }

        // Rust: `let_declaration` → field `pattern` (the name), field
        // `type` (the annotation) and field `value` (the initialiser).
        // The type is either annotated (`let hooks: SyncManager = ...`) or
        // inferred from the constructor (`let hooks = SyncManager::new(...)`).
        Language::Rust => {
            if node.kind() != "let_declaration" {
                return None;
            }
            let pattern = node.child_by_field_name("pattern")?;
            if pattern.kind() != "identifier" {
                return None;
            }
            let name = text_of(&pattern)?;

            if let Some(type_node) = node.child_by_field_name("type")
                && let Some(type_name) = simple_type_name(&type_node, source)
            {
                return Some((name, type_name));
            }

            let value = node.child_by_field_name("value")?;
            if value.kind() != "call_expression" {
                return None;
            }
            let mut cursor = value.walk();
            let callee = value.named_children(&mut cursor).next()?;
            // `SyncManager::new(...)` — the path names the type, the last
            // segment names the constructor.
            let type_name = if callee.kind() == "scoped_identifier" {
                let path = callee
                    .child_by_field_name("path")
                    .or_else(|| callee.child_by_field_name("type"))?;
                simple_type_name(&path, source)?
            } else {
                simple_type_name(&callee, source)?
            };
            Some((name, type_name))
        }

        _ => None,
    }
}

/// Does `name` look like the name of a type?
///
/// Type names conventionally start with an upper-case letter. This is a
/// heuristic used only to distinguish `Circle(2.0)` from `circle.area()`;
/// the actual type is confirmed against the symbol index.
fn looks_like_type_name(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_uppercase())
}

/// Split a dotted/qualified callee text into `(qualifier, name)`.
///
/// `Helper.doThing` → `("Helper", "doThing")`; `doThing` → `(None, "doThing")`.
fn split_qualified_name(raw: &str) -> (Option<String>, String) {
    let mut parts = raw.rsplit(['.', ':', '>']);
    let last = parts.next().unwrap_or(raw).trim();
    if last.is_empty() {
        return (None, raw.to_string());
    }

    // `a::b` splits on ':' producing an empty trailing segment for `::`.
    let receiver = raw
        .strip_suffix(last)
        .map(|prefix| prefix.trim_end_matches([':', '.', '>', ' ']).trim())
        .filter(|prefix| !prefix.is_empty())
        .map(|prefix| {
            prefix
                .rsplit(['.', ':', '>'])
                .next()
                .unwrap_or(prefix)
                .trim()
                .to_string()
        })
        .filter(|prefix| !prefix.is_empty() && prefix != "::");

    (receiver, last.to_string())
}

/// Extract `(qualifier, method)` from a callee node whose method name is its
/// last identifier.
///
/// Handles Kotlin's `navigation_expression` (children: receiver
/// `identifier`, `.`, member `identifier`) and Swift's (children: `target`
/// field, `navigation_suffix`).
///
/// The receiver is the *first* identifier in the subtree and the method is
/// the *last*, which works for both grammars.
fn first_child_last_identifier(callee: &TsNode, source_bytes: &[u8]) -> (Option<String>, String) {
    let kind = callee.kind();
    if matches!(
        kind,
        "identifier" | "simple_identifier" | "field_identifier" | "type_identifier"
    ) {
        let text = callee
            .utf8_text(source_bytes)
            .unwrap_or("")
            .trim()
            .to_string();
        return (None, text);
    }

    // Collect every identifier in document order.
    let mut identifiers = Vec::new();
    collect_identifiers(callee, &mut identifiers);

    let text_of = |node: &TsNode| {
        node.utf8_text(source_bytes)
            .ok()
            .map(|s| s.trim().to_string())
    };

    // The method name is the last identifier.
    let name = identifiers.last().and_then(text_of).unwrap_or_default();

    // The receiver is the first identifier, unless the method name *is* the
    // first identifier (an unqualified call).
    let qualifier = if identifiers.len() >= 2 {
        identifiers
            .first()
            .and_then(text_of)
            .filter(|t| !t.is_empty())
    } else {
        None
    };

    (qualifier, name)
}

/// Collect every identifier-ish node in the subtree, in document order.
fn collect_identifiers<'a>(node: &TsNode<'a>, found: &mut Vec<TsNode<'a>>) {
    let kind = node.kind();
    if matches!(
        kind,
        "identifier" | "simple_identifier" | "field_identifier" | "type_identifier"
    ) {
        found.push(*node);
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_identifiers(&child, found);
    }
}
/// Map a grammar node to a graph node kind for `language`.
///
/// The mapping table lives in [`crate::grammar`] so it can be checked against
/// the real grammar by `tests/grammar_contract_test.rs`. Two languages need
/// extra discrimination that a plain kind→kind table cannot express:
///
/// - **Swift** uses `class_declaration` for `struct`, `class`, `enum` and
///   `extension`, distinguished by the `declaration_kind` field.
/// - **Kotlin** uses `class_declaration` for `class`, `interface` and `enum`.
///   `interface` is an anonymous keyword token (there is no
///   `interface_declaration` node kind), and `enum` shows up as an
///   `enum_class_body` child.
fn map_node_kind(node: &TsNode, language: Language) -> (Option<NodeKind>, bool) {
    let kind = node.kind();
    let (mapped, container) = grammar::map_kind(language, kind);

    if language == Language::Swift && kind == "class_declaration" {
        // Swift reuses `class_declaration` for every nominal type. The
        // `declaration_kind` field points at the introducing keyword token,
        // whose kind is `struct` / `class` / `enum` / `extension` / `actor`.
        let declaration_kind = node
            .child_by_field_name("declaration_kind")
            .map_or("", |n| n.kind());
        return match declaration_kind {
            "struct" => (Some(NodeKind::Struct), container),
            "enum" => (Some(NodeKind::Enum), container),
            _ => (Some(NodeKind::Class), container),
        };
    }

    if language == Language::Kotlin && kind == "class_declaration" {
        // Kotlin also reuses `class_declaration` for classes, interfaces and
        // enum classes.
        let mut cursor = node.walk();
        let children: Vec<TsNode> = node.children(&mut cursor).collect();
        let is_interface = children
            .iter()
            .any(|c| !c.is_named() && c.kind() == "interface");
        let is_enum = children.iter().any(|c| c.kind() == "enum_class_body");
        if is_interface {
            return (Some(NodeKind::Interface), container);
        }
        if is_enum {
            return (Some(NodeKind::Enum), container);
        }
    }

    (mapped, container)
}

fn scan_directory(
    root_dir: &Path,
    config: &CodeGraphConfig,
    mut on_progress: impl FnMut(usize, &str),
) -> Vec<String> {
    let mut files = Vec::new();
    let mut count = 0;

    let mut stack = vec![root_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let rel_path = match path.strip_prefix(root_dir) {
                Ok(rel) => rel,
                Err(_) => continue,
            };
            let rel_str = crate::paths::canonical(rel_path);

            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                let dir_pattern = format!("{}/", rel_str);
                if config.exclude.iter().any(|p| matches_glob(&dir_pattern, p)) {
                    continue;
                }
                // Skip Python virtual environments by their canonical marker file
                // (covers any venv name, not just common patterns like .venv/venv/env)
                if path.join("pyvenv.cfg").exists() {
                    continue;
                }
                stack.push(path);
            } else if entry.file_type().is_ok_and(|t| t.is_file()) {
                if should_include_file(&rel_str, config) {
                    files.push(rel_str.clone());
                    count += 1;
                    on_progress(count, &rel_str);
                }
            }
        }
    }

    files
}

fn should_include_file(file_path: &str, config: &CodeGraphConfig) -> bool {
    for pattern in &config.exclude {
        if matches_glob(file_path, pattern) {
            return false;
        }
    }

    for pattern in &config.include {
        if matches_glob(file_path, pattern) {
            return true;
        }
    }

    false
}

fn matches_glob(file_path: &str, pattern: &str) -> bool {
    globset::Glob::new(pattern)
        .ok()
        .and_then(|glob| glob.compile_matcher().is_match(file_path).then_some(true))
        .unwrap_or(false)
}

fn detect_language(path: &str) -> Language {
    let ext = Path::new(path)
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "ts" => Language::TypeScript,
        "tsx" => Language::Tsx,
        "js" => Language::JavaScript,
        "jsx" => Language::Jsx,
        "py" => Language::Python,
        "go" => Language::Go,
        "rs" => Language::Rust,
        "java" => Language::Java,
        "c" => Language::C,
        "h" => Language::C,
        "cpp" | "cc" | "cxx" | "hpp" => Language::Cpp,
        "cs" => Language::CSharp,
        "php" => Language::Php,
        "rb" => Language::Ruby,
        "swift" => Language::Swift,
        "kt" | "kts" => Language::Kotlin,
        "liquid" => Language::Liquid,
        "razor" | "cshtml" => Language::Blazor,
        // New languages
        "sh" | "bash" => Language::Bash,
        "dart" => Language::Dart,
        "ex" | "exs" => Language::Elixir,
        "elm" => Language::Elm,
        "erl" | "hrl" => Language::Erlang,
        "f" | "f90" | "f95" | "for" | "f03" | "f08" => Language::Fortran,
        "groovy" | "gradle" => Language::Groovy,
        "hs" => Language::Haskell,
        "jl" => Language::Julia,
        "lua" => Language::Lua,
        "md" | "markdown" => Language::Markdown,
        "m" => Language::Matlab,
        "nix" => Language::Nix,
        "pl" | "pm" => Language::Perl,
        "ps1" => Language::Powershell,
        "r" => Language::R,
        "scala" | "sc" => Language::Scala,
        "toml" => Language::Toml,
        "yml" | "yaml" => Language::Yaml,
        "zig" => Language::Zig,
        _ => Language::Unknown,
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "test assertions: panicking on setup failure is the correct behavior"
    )]
    use super::detect_language;
    use crate::config::is_language_supported;
    use crate::types::Language;

    /// Collect every node of `kind` in the tree.
    fn collect_nodes<'a>(
        node: tree_sitter::Node<'a>,
        kind: &str,
        found: &mut Vec<tree_sitter::Node<'a>>,
    ) {
        if node.kind() == kind {
            found.push(node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            collect_nodes(child, kind, found);
        }
    }

    #[test]
    fn kotlin_script_files_map_to_kotlin() {
        assert_eq!(detect_language("build.gradle.kts"), Language::Kotlin);
        assert_eq!(detect_language("src/Main.kt"), Language::Kotlin);
    }

    /// A Rust local takes its type from the annotation, or from the
    /// constructor it is built with.
    ///
    /// The receiver of `hooks.install_hook()` is a variable, not a type,
    /// so without this the resolver looks for a type called `hooks` and
    /// finds nothing.
    #[test]
    fn a_rust_local_takes_its_type_from_the_constructor() {
        let source =
            "fn main() {\n    let hooks = SyncManager::new();\n    let count: usize = 0;\n}\n";

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter::Language::new(tree_sitter_rust::LANGUAGE))
            .expect("set the Rust grammar");
        let tree = parser.parse(source, None).expect("parse the Rust source");

        let mut declarations = Vec::new();
        collect_nodes(tree.root_node(), "let_declaration", &mut declarations);
        assert_eq!(declarations.len(), 2);
        assert_eq!(
            super::local_variable_type(&declarations[0], source, Language::Rust),
            Some(("hooks".to_string(), "SyncManager".to_string()))
        );
        assert_eq!(
            super::local_variable_type(&declarations[1], source, Language::Rust),
            Some(("count".to_string(), "usize".to_string()))
        );
    }

    #[test]
    fn swift_files_map_to_swift() {
        assert_eq!(detect_language("Sources/App/Runner.swift"), Language::Swift);
    }

    #[test]
    fn unknown_extensions_are_not_supported() {
        assert_eq!(detect_language("notes.txt"), Language::Unknown);
        assert!(!is_language_supported(&Language::Unknown));
    }

    #[test]
    fn kotlin_and_swift_are_supported() {
        assert!(is_language_supported(&Language::Kotlin));
        assert!(is_language_supported(&Language::Swift));
    }
}
