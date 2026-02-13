#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::db;
use crate::types::Node;
use crate::types::{Edge, EdgeKind, NodeKind};

#[derive(Debug, Default)]
pub struct ReferenceResolver;

#[derive(Debug, Clone)]
pub struct ResolveResult {
    pub scanned: usize,
    pub resolved: usize,
    pub remaining: usize,
}

impl ReferenceResolver {
    pub fn resolve_unresolved(
        conn: &mut rusqlite::Connection,
        limit: usize,
    ) -> std::io::Result<ResolveResult> {
        let unresolved = db::list_unresolved_refs(conn, limit)?;
        if unresolved.is_empty() {
            return Ok(ResolveResult {
                scanned: 0,
                resolved: 0,
                remaining: 0,
            });
        }

        let mut resolved_edges = Vec::new();
        let mut resolved_ids = Vec::new();

        for row in &unresolved {
            let reference = &row.reference;
            let from_node = db::get_node_by_id(conn, &reference.from_node_id)?;
            let candidates = match reference.reference_kind {
                EdgeKind::Calls => {
                    filter_by_call_kind(db::find_nodes_by_name(conn, &reference.reference_name)?)
                }
                _ => db::find_nodes_by_name(conn, &reference.reference_name)?,
            };

            let import_hint = from_node
                .as_ref()
                .and_then(|node| import_match_hint(conn, node, &reference.reference_name).ok())
                .flatten();
            let candidates = rank_candidates(
                conn,
                candidates,
                from_node.as_ref(),
                import_hint.as_ref(),
                &reference.reference_name,
            )?;

            if let [target] = candidates.as_slice() {
                resolved_edges.push(Edge {
                    source: reference.from_node_id.clone(),
                    target: target.id.clone(),
                    kind: reference.reference_kind,
                    metadata: None,
                    line: Some(reference.line),
                    column: Some(reference.column),
                });
                resolved_ids.push(row.id);
            }
        }

        if !resolved_edges.is_empty() {
            db::insert_edges(conn, &resolved_edges)?;
        }
        if !resolved_ids.is_empty() {
            db::delete_unresolved_refs(conn, &resolved_ids)?;
        }

        let remaining = unresolved.len().saturating_sub(resolved_ids.len());
        Ok(ResolveResult {
            scanned: unresolved.len(),
            resolved: resolved_ids.len(),
            remaining,
        })
    }
}

fn filter_by_call_kind(nodes: Vec<Node>) -> Vec<Node> {
    let mut seen = HashSet::new();
    let mut filtered = Vec::new();
    for node in nodes {
        if matches!(node.kind, NodeKind::Function | NodeKind::Method)
            && seen.insert(node.id.clone())
        {
            filtered.push(node);
        }
    }
    filtered
}

fn rank_candidates(
    conn: &rusqlite::Connection,
    nodes: Vec<Node>,
    from_node: Option<&Node>,
    import_hint: Option<&ImportHint>,
    symbol_name: &str,
) -> std::io::Result<Vec<Node>> {
    let Some(from_node) = from_node else {
        return Ok(nodes);
    };

    if let Some(hint) = import_hint {
        let export_name = hint.export_name.as_deref().unwrap_or(symbol_name);
        if let Some(exports) = export_candidates(conn, &hint.module_path, export_name)? {
            return Ok(exports);
        }
    }

    let from_dir = Path::new(&from_node.file_path).parent();
    let mut import_matches = Vec::new();
    let mut same_file = Vec::new();
    let mut same_dir = Vec::new();
    let mut others = Vec::new();

    for node in nodes {
        if import_hint.is_some_and(|hint| matches_import_hint(&node.file_path, &hint.module_path)) {
            import_matches.push(node);
            continue;
        }
        if node.file_path == from_node.file_path {
            same_file.push(node);
        } else if from_dir.is_some() && Path::new(&node.file_path).parent() == from_dir {
            same_dir.push(node);
        } else {
            others.push(node);
        }
    }

    if !import_matches.is_empty() {
        Ok(import_matches)
    } else if !same_file.is_empty() {
        Ok(same_file)
    } else if !same_dir.is_empty() {
        Ok(same_dir)
    } else {
        Ok(others)
    }
}

fn import_match_hint(
    conn: &rusqlite::Connection,
    from_node: &Node,
    symbol_name: &str,
) -> std::io::Result<Option<ImportHint>> {
    let imports = db::find_nodes_by_name(conn, symbol_name)?;
    let mut best: Option<ImportHint> = None;
    for import_node in imports {
        if import_node.kind != NodeKind::Import {
            continue;
        }
        if import_node.file_path == from_node.file_path {
            if let Some(hint) = import_node
                .signature
                .as_deref()
                .and_then(parse_import_signature)
            {
                best = Some(hint);
                break;
            }

            best = Some(ImportHint {
                module_path: import_node.name,
                export_name: None,
            });
            break;
        }
    }
    Ok(best)
}

fn matches_import_hint(file_path: &str, hint: &str) -> bool {
    let hint_clean = hint
        .rsplit("::")
        .next()
        .unwrap_or(hint)
        .trim_end_matches(".ts")
        .trim_end_matches(".tsx")
        .trim_end_matches(".rs");
    let path_no_ext = file_path
        .trim_end_matches(".ts")
        .trim_end_matches(".tsx")
        .trim_end_matches(".rs");

    if path_no_ext.ends_with(hint_clean) {
        return true;
    }

    let file_path_buf = PathBuf::from(file_path);
    let file_name = file_path_buf
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if file_name == hint_clean {
        return true;
    }

    if file_path.ends_with("/mod.rs") {
        let parent_name = Path::new(file_path)
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("");
        return parent_name == hint_clean;
    }

    false
}

fn export_candidates(
    conn: &rusqlite::Connection,
    module_path: &str,
    export_name: &str,
) -> std::io::Result<Option<Vec<Node>>> {
    let exports = db::find_exports_by_module(conn, module_path)?;
    if exports.is_empty() {
        return Ok(None);
    }

    let mut exact = Vec::new();
    for export in exports {
        if export.name == export_name {
            exact.push(export);
        }
    }

    if exact.is_empty() {
        Ok(None)
    } else {
        Ok(Some(exact))
    }
}

#[derive(Debug, Clone)]
struct ImportHint {
    module_path: String,
    export_name: Option<String>,
}

fn parse_import_signature(signature: &str) -> Option<ImportHint> {
    if signature.trim().is_empty() {
        return None;
    }

    if let Some((module_path, export_name)) = signature.split_once("|export=") {
        return Some(ImportHint {
            module_path: module_path.to_string(),
            export_name: Some(export_name.to_string()),
        });
    }

    Some(ImportHint {
        module_path: signature.to_string(),
        export_name: None,
    })
}
