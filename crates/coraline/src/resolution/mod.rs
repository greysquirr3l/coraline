#![deny(unsafe_code)]

pub mod frameworks;

use std::collections::HashSet;
use std::path::Path;

use rusqlite::params;
use tracing::debug;

use crate::db;
use crate::db::{io_other, row_to_node};
use crate::types::Node;
use crate::types::{Edge, EdgeKind, NodeKind, UnresolvedReference};

#[derive(Debug, Default)]
pub struct ReferenceResolver;

#[derive(Debug, Clone)]
pub struct ResolveResult {
    pub scanned: usize,
    pub resolved: usize,
    pub remaining: usize,
}

impl ReferenceResolver {
    /// Resolve the pending unresolved references into edges.
    ///
    /// `limit` is the *page size*, not a cap: the whole pending backlog is
    /// walked with a keyset cursor (`unresolved_refs.id` is monotonic), so a
    /// reference beyond the first page is never starved. The previous
    /// implementation used an unordered `LIMIT ?`, which handed out the
    /// same first rows on every run and permanently stalled the backlog.
    ///
    /// References that still do not resolve are flagged as *attempted* so
    /// they are not retried until something in the project changes — see
    /// [`db::reset_unresolved_refs_attempted`].
    pub fn resolve_unresolved(
        conn: &mut rusqlite::Connection,
        project_root: &Path,
        limit: usize,
    ) -> std::io::Result<ResolveResult> {
        let page_size = limit.max(1);
        let unresolved = db::list_all_unresolved_refs_paged(conn, page_size)?;
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
            let candidates = candidates_for_reference(conn, reference)?;

            let import_hint = from_node
                .as_ref()
                .and_then(|node| {
                    import_match_hint(
                        conn,
                        node,
                        &reference.reference_name,
                        reference.qualifier.as_deref(),
                    )
                    .ok()
                })
                .flatten();

            // A qualified call is scoped to the type of its receiver:
            // `helper.area()` on a `Circle` resolves to `Circle.area`, even
            // when the method is declared in another file. This is what makes
            // cross-package calls resolvable without an explicit import.
            if let Some(qualifier) = reference.qualifier.as_deref() {
                let scoped = type_member_candidates(conn, qualifier, &reference.reference_name)?;
                if let [target] = scoped.as_slice() {
                    resolved_edges.push(Edge {
                        source: reference.from_node_id.clone(),
                        target: target.id.clone(),
                        kind: reference.reference_kind,
                        metadata: None,
                        line: Some(reference.line),
                        column: Some(reference.column),
                        confidence: confidence_for_reference(&reference.reference_name),
                        process_id: None,
                    });
                    resolved_ids.push(row.id);
                    continue;
                }
            }
            let candidates = rank_candidates(
                conn,
                candidates,
                from_node.as_ref(),
                import_hint.as_ref(),
                &reference.reference_name,
                reference.reference_kind,
            )?;

            // If generic resolution found nothing, try framework-specific hints.
            let candidates = if candidates.is_empty() {
                from_node.as_ref().map_or_else(Vec::new, |from| {
                    framework_fallback(conn, project_root, from, &reference.reference_name)
                })
            } else {
                candidates
            };

            if let [target] = candidates.as_slice() {
                let confidence = confidence_for_reference(&reference.reference_name);
                resolved_edges.push(Edge {
                    source: reference.from_node_id.clone(),
                    target: target.id.clone(),
                    kind: reference.reference_kind,
                    metadata: None,
                    line: Some(reference.line),
                    column: Some(reference.column),
                    confidence,
                    process_id: None,
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

        // Everything still in the table after this pass was tried and
        // failed. Flagging it keeps the next pass cheap without losing the
        // reference: `reset_unresolved_refs_attempted` puts it back in the
        // queue as soon as the project changes.
        let failed = db::mark_pending_unresolved_refs_attempted(conn)?;
        if failed > 0 {
            debug!(
                failed,
                "unresolved references that failed to resolve were flagged"
            );
        }

        let remaining = unresolved.len().saturating_sub(resolved_ids.len());
        Ok(ResolveResult {
            scanned: unresolved.len(),
            resolved: resolved_ids.len(),
            remaining,
        })
    }
}

/// Collect the nodes that could plausibly be the target of `reference`.
///
/// The candidate set is filtered by the kind of edge being resolved: an
/// instantiation targets the *type*, not a constructor that happens to share
/// its name, and a supertype is likewise always a type.
fn candidates_for_reference(
    conn: &rusqlite::Connection,
    reference: &UnresolvedReference,
) -> std::io::Result<Vec<Node>> {
    match reference.reference_kind {
        EdgeKind::Calls => {
            // Prefer extractor-provided candidate IDs for better locality/precision.
            let from_ids = reference
                .candidates
                .as_ref()
                .map_or_else(Vec::new, |ids| nodes_from_ids(conn, ids));
            let found = if from_ids.is_empty() {
                db::find_nodes_by_name(conn, &reference.reference_name)?
            } else {
                from_ids
            };
            Ok(filter_by_call_kind(found))
        }
        EdgeKind::Instantiates => {
            let found = db::find_nodes_by_name(conn, &reference.reference_name)?;
            Ok(prefer_types(
                found,
                &[
                    NodeKind::Class,
                    NodeKind::Struct,
                    NodeKind::Interface,
                    NodeKind::Enum,
                ],
            ))
        }
        EdgeKind::Extends | EdgeKind::Implements => {
            let found = db::find_nodes_by_name(conn, &reference.reference_name)?;
            Ok(prefer_types(
                found,
                &[
                    NodeKind::Class,
                    NodeKind::Struct,
                    NodeKind::Interface,
                    NodeKind::Trait,
                    NodeKind::Protocol,
                    NodeKind::Enum,
                ],
            ))
        }
        _ => db::find_nodes_by_name(conn, &reference.reference_name),
    }
}

/// Narrow `nodes` to those of one of `kinds`, falling back to the full set
/// when nothing matches (so a still-resolvable but unusual declaration is
/// not silently dropped).
fn prefer_types(nodes: Vec<Node>, kinds: &[NodeKind]) -> Vec<Node> {
    let types: Vec<Node> = nodes
        .iter()
        .filter(|n| kinds.contains(&n.kind))
        .cloned()
        .collect();
    if types.is_empty() { nodes } else { types }
}

/// Compute the resolution-confidence score for a single resolved reference.
///
/// The thresholds mirror the Phase 5.2 spec (borrows `GitNexus`'s per-edge
/// confidence field, used in their `WHERE r.confidence > 0.8` Cypher
/// filters):
///
/// - `0.95` — strongly-typed Rust path (`crate::`, `super::`, `self::`).
/// - `0.5`  — generic name match / framework fallback / heuristic ranker
///   (the default for everything else).
///
/// Alias-chain detection (the `0.7` tier in the spec) would need access to
/// the originating `use ... as ...` AST node, which isn't currently
/// surfaced through [`UnresolvedReference`]. Once it is, this function
/// is the single place to extend.
fn confidence_for_reference(name: &str) -> f32 {
    if name.starts_with("crate::") || name.starts_with("super::") || name.starts_with("self::") {
        0.95
    } else {
        0.5
    }
}

fn nodes_from_ids(conn: &rusqlite::Connection, ids: &[String]) -> Vec<Node> {
    let mut nodes = Vec::new();
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id) {
            continue;
        }

        if let Ok(Some(node)) = db::get_node_by_id(conn, id) {
            nodes.push(node);
        }
    }
    nodes
}

/// Use framework-specific resolvers to find candidates when name search fails.
fn framework_fallback(
    conn: &rusqlite::Connection,
    project_root: &Path,
    from_node: &Node,
    reference_name: &str,
) -> Vec<Node> {
    // from_node.file_path is relative to the project root
    let from_abs = project_root.join(&from_node.file_path);
    let from_abs_str = from_abs.to_string_lossy();

    let hints = frameworks::framework_path_hints(project_root, &from_abs_str, reference_name);
    if hints.is_empty() {
        return Vec::new();
    }

    // The last "::" segment is the symbol name we're looking for in those files
    let sym_name = reference_name.split("::").last().unwrap_or(reference_name);

    let mut candidates = Vec::new();
    for hint_path in &hints {
        let rel = relative_to_root(hint_path, project_root);
        if let Ok(mut nodes) = db::get_nodes_by_file(conn, &rel, None) {
            nodes.retain(|n| n.name == sym_name || n.name == reference_name);
            candidates.extend(nodes);
        }
    }
    candidates
}

fn relative_to_root(path: &Path, root: &Path) -> String {
    crate::paths::canonical(path.strip_prefix(root).unwrap_or(path))
}

/// Keep only the nodes that could plausibly be the target of a call.
///
/// Class-like nodes are included so that constructor calls
/// (`Circle(2.0)` → the `Circle` class) can resolve; previously they were
/// filtered out and every instantiation edge was lost.
fn filter_by_call_kind(nodes: Vec<Node>) -> Vec<Node> {
    let mut seen = HashSet::new();
    let mut filtered = Vec::new();
    for node in nodes {
        if matches!(
            node.kind,
            NodeKind::Function | NodeKind::Method | NodeKind::Class | NodeKind::Struct
        ) && seen.insert(node.id.clone())
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
    reference_kind: EdgeKind,
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

    let from_dir = directory_of(&from_node.file_path);
    let mut import_matches = Vec::new();
    let mut package_matches = Vec::new();
    let mut same_file = Vec::new();
    let mut same_dir = Vec::new();
    let mut same_package = Vec::new();
    let mut others = Vec::new();

    for node in nodes {
        if let Some(hint) = import_hint {
            if matches_import_hint_exact(&node.file_path, &hint.module_path) {
                import_matches.push(node);
                continue;
            }
            if matches_import_package(&node.file_path, &hint.module_path) {
                package_matches.push(node);
                continue;
            }
        }
        if node.file_path == from_node.file_path {
            same_file.push(node);
        } else if !from_dir.is_empty() && directory_of(&node.file_path) == from_dir {
            same_dir.push(node);
        } else if in_same_package(&from_node.file_path, &node.file_path) {
            same_package.push(node);
        } else {
            others.push(node);
        }
    }

    if !import_matches.is_empty() {
        Ok(import_matches)
    } else if !package_matches.is_empty() {
        Ok(package_matches)
    } else if !same_file.is_empty() {
        Ok(same_file)
    } else if !same_dir.is_empty() {
        Ok(same_dir)
    } else if !same_package.is_empty() {
        // Same package (JVM/C#/Swift style directory layout) — a legitimate
        // cross-package call that is not backed by an import.
        Ok(same_package)
    } else if reference_kind == EdgeKind::Calls {
        // Avoid low-confidence global-name fallback for call edges because
        // it causes noisy cross-project links in mixed active/legacy workspaces.
        Ok(Vec::new())
    } else {
        Ok(others)
    }
}

/// Find the import in `from_node`'s file that best explains a reference to
/// `symbol_name`.
///
/// Two lookups are tried, in order:
///
/// 1. **By qualifier.** For a qualified call such as `Helper.doThing()` the
///    receiver is `Helper` and the import is `import com.example.other.Helper`
///    — the import's *local name* is `Helper`, but the symbol being called is
///    `doThing`. Looking the import up by `doThing` finds nothing, which is
///    why cross-package calls used to never resolve.
/// 2. **By symbol name.** For an unqualified call `doThing()`, an import of
///    `doThing` itself is the match.
fn import_match_hint(
    conn: &rusqlite::Connection,
    from_node: &Node,
    symbol_name: &str,
    qualifier: Option<&str>,
) -> std::io::Result<Option<ImportHint>> {
    let qualifier_hint = match qualifier {
        Some(qualifier) => import_hint_for_name(conn, from_node, qualifier)?,
        None => None,
    };
    if let Some(hint) = qualifier_hint {
        return Ok(Some(hint));
    }

    import_hint_for_name(conn, from_node, symbol_name)
}

/// Look up an import whose local name matches `local_name` in the same file
/// as `from_node`.
fn import_hint_for_name(
    conn: &rusqlite::Connection,
    from_node: &Node,
    local_name: &str,
) -> std::io::Result<Option<ImportHint>> {
    let imports = db::find_nodes_by_name(conn, local_name)?;
    for import_node in &imports {
        if import_node.kind != NodeKind::Import {
            continue;
        }
        if import_node.file_path != from_node.file_path {
            continue;
        }

        if let Some(hint) = import_node
            .signature
            .as_deref()
            .and_then(parse_import_signature)
        {
            return Ok(Some(hint));
        }

        return Ok(Some(ImportHint {
            module_path: import_node.name.clone(),
            export_name: None,
        }));
    }

    // The symbol may not be imported by name at all — the import may name
    // the *package* the symbol lives in (`import com.example.util;` in Go,
    // `using Example.Util;` in C#). Fall back to an import whose module
    // path points at the same file as the symbol.
    let symbols = db::find_nodes_by_name(conn, local_name)?;
    for symbol in symbols {
        if symbol.kind == NodeKind::Import {
            continue;
        }
        for import_node in &imports {
            if import_node.file_path != from_node.file_path {
                continue;
            }
            let Some(hint) = import_node
                .signature
                .as_deref()
                .and_then(parse_import_signature)
            else {
                continue;
            };
            if matches_import_hint(&symbol.file_path, &hint.module_path) {
                return Ok(Some(hint));
            }
        }
    }

    Ok(None)
}

/// Does `file_path` correspond to the imported module `hint`?
///
/// Import paths come in two flavours: Rust-style (`crate::module::Thing`) and
/// package-style (`com.example.other.Helper`, `com/example/other/Helper`).
/// Both are normalised to a slash-separated path with the source extension
/// stripped before comparison.
fn matches_import_hint(file_path: &str, hint: &str) -> bool {
    matches_import_hint_exact(file_path, hint) || matches_import_package(file_path, hint)
}

/// Does `file_path` name the exact module the import refers to?
///
/// `import com.example.other.Helper` refers to the file
/// `…/com/example/other/Helper.kt` — the import path's last segment is the
/// file's stem.
/// Find the members of the type named `type_name` that are called
/// `member_name`.
///
/// A type's members are the nodes it *contains* (the `Contains` edge from
/// the class node to its methods), so the lookup is a join rather than a
/// name match.
fn type_member_candidates(
    conn: &rusqlite::Connection,
    type_name: &str,
    member_name: &str,
) -> std::io::Result<Vec<Node>> {
    let mut stmt = conn
        .prepare(
            "SELECT child.id, child.kind, child.name, child.qualified_name, child.file_path,
                    child.language, child.start_line, child.end_line, child.start_column,
                    child.end_column, child.docstring, child.signature, child.visibility,
                    child.is_exported, child.is_async, child.is_static, child.is_abstract,
                    child.decorators, child.type_parameters, child.updated_at
             FROM edges
             JOIN nodes parent ON parent.id = edges.source
             JOIN nodes child ON child.id = edges.target
             WHERE edges.kind = 'contains'
               AND parent.name = ?
               AND child.name = ?",
        )
        .map_err(io_other)?;

    let rows = stmt
        .query_map(params![type_name, member_name], row_to_node)
        .map_err(io_other)?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(io_other)?);
    }
    Ok(results)
}

fn matches_import_hint_exact(file_path: &str, hint: &str) -> bool {
    let hint_path = normalise_module_path(hint);
    let file_path = normalise_module_path(file_path);

    if hint_path.is_empty() {
        return false;
    }

    if file_path == hint_path {
        return true;
    }

    // The import path is usually relative to the *source root*
    // (`com/example/other/Helper`), while the stored file path is relative
    // to the *project root* (`src/com/example/other/Helper`). Match when
    // the file path ends with the import path on a path-segment boundary.
    if ends_on_segment_boundary(&file_path, &hint_path) {
        return true;
    }

    // The import may name the file's stem: `import Helper` matches
    // `com/example/other/Helper.kt`.
    let file_stem = file_path.rsplit('/').next().unwrap_or(&file_path);
    let hint_stem = hint_path.rsplit('/').next().unwrap_or(&hint_path);
    if !hint_stem.is_empty() && file_stem == hint_stem {
        return true;
    }

    // Rust modules: `mod.rs` stands in for the directory name.
    if file_path.ends_with("/mod.rs") || file_path == "mod.rs" {
        let parent = hint_path
            .rsplit('/')
            .next()
            .unwrap_or(&hint_path)
            .trim_end_matches(".rs");
        return !parent.is_empty() && parent == hint_stem;
    }

    false
}

/// Does `file_path` live in the package the import refers to?
///
/// On the JVM (and in C#, Swift, Go) an import names a *package* plus a
/// symbol: `import com.example.other.Helper` pulls the symbol `Helper` out
/// of the package `com.example.other`, which may live in any file inside it.
/// The file `com/example/other/Shapes.kt` is in that package.
fn matches_import_package(file_path: &str, hint: &str) -> bool {
    let hint_path = normalise_module_path(hint);
    let file_path = normalise_module_path(file_path);

    if hint_path.is_empty() || file_path.is_empty() {
        return false;
    }

    // The package is the import path minus its last segment.
    let Some((package, _)) = hint_path.rsplit_once('/') else {
        // A bare name (`import Helper`) names a file, not a package.
        return false;
    };

    let Some((file_dir, _)) = file_path.rsplit_once('/') else {
        return false;
    };

    file_dir == package || ends_on_segment_boundary(file_dir, package)
}

/// Does `path` end with `suffix` on a path-segment boundary?
///
/// `com/example/other/Helper` ends with `example/other/Helper` on a
/// boundary, but `com/example/other/Helper` does *not* end with
/// `other/Helper` — there the preceding character is a letter, not a
/// separator.
fn ends_on_segment_boundary(path: &str, suffix: &str) -> bool {
    if !path.ends_with(suffix) {
        return false;
    }
    let boundary = path.len().saturating_sub(suffix.len());
    boundary
        .checked_sub(1)
        .is_none_or(|index| path.as_bytes().get(index) == Some(&b'/'))
}

/// Normalise a module path or file path for comparison.
///
/// - Rust `::` separators become `/`.
/// - Package separators (`.`) become `/` **only** when the path looks like a
///   Java/Kotlin/C# package rather than a file path — a path that already
///   contains `/` or a known file extension is left alone.
/// - Source extensions are stripped.
fn normalise_module_path(path: &str) -> String {
    let path = path.trim();
    let path = path.replace("::", "/");
    // A path that reaches the resolver as a string may still carry Windows
    // separators, either because it came from an older database or because
    // it was read out of source. The canonical form uses `/` throughout.
    let path = crate::paths::normalise_separators(&path);

    let has_slash = path.contains('/');
    let has_known_extension = [
        ".kt", ".kts", ".java", ".cs", ".go", ".py", ".swift", ".ts", ".tsx", ".js", ".jsx", ".rs",
        ".rb", ".php", ".c", ".h", ".cpp", ".hpp", ".cc", ".cxx",
    ]
    .iter()
    .any(|ext| path.ends_with(ext));

    let slashed = if !has_slash && !has_known_extension {
        path.replace('.', "/")
    } else {
        path
    };

    let mut normalised = slashed.trim_matches('/').to_string();

    for ext in [
        ".kt", ".kts", ".java", ".cs", ".go", ".py", ".swift", ".ts", ".tsx", ".js", ".jsx", ".rs",
        ".rb", ".php", ".c", ".h", ".cpp", ".hpp", ".cc", ".cxx",
    ] {
        if normalised.ends_with(ext) {
            normalised.truncate(normalised.len() - ext.len());
            break;
        }
    }

    normalised
}

/// Do two files belong to the same package?
///
/// Java, Kotlin, C# and Swift all group files into packages that mirror the
/// directory layout. A call from `com/example/app/Main.kt` to a symbol in
/// `com/example/util/Helper.kt` is same-package when both live under the same
/// package root.
/// The directory a canonical path lives in, or an empty string when the
/// path has no directory part.
///
/// The comparison is done on the string rather than with [`Path::parent`]
/// so that it does not depend on the platform's path separator — a
/// Windows path rendered with `\` has to bucket the same way as the same
/// path rendered with `/`.
fn directory_of(file_path: &str) -> String {
    let normalised = crate::paths::normalise_separators(file_path);
    crate::paths::parent(&normalised).to_string()
}

fn in_same_package(from_file: &str, to_file: &str) -> bool {
    let from_path = crate::paths::normalise_separators(from_file);
    let to_path = crate::paths::normalise_separators(to_file);
    let from_dir = crate::paths::parent(&from_path);
    let to_dir = crate::paths::parent(&to_path);

    if from_dir.is_empty() || to_dir.is_empty() {
        return false;
    }

    // Walk up from the deeper directory until the paths meet; the meeting
    // point is the package root.
    let from_parts: Vec<&str> = from_dir.split('/').filter(|s| !s.is_empty()).collect();
    let to_parts: Vec<&str> = to_dir.split('/').filter(|s| !s.is_empty()).collect();

    let mut common = 0;
    for (a, b) in from_parts.iter().zip(to_parts.iter()) {
        if a != b {
            break;
        }
        common += 1;
    }

    // Require at least one shared directory component beyond the file name
    // so that sibling directories in unrelated packages do not match.
    common >= 1 && (from_parts.len() > common || to_parts.len() > common)
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

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "test assertions: panicking on setup failure is the correct behavior"
    )]
    use super::confidence_for_reference;
    use super::{
        ImportHint, in_same_package, matches_import_hint, matches_import_hint_exact,
        matches_import_package, rank_candidates,
    };
    use crate::types::{EdgeKind, Language, Node, NodeKind};

    #[test]
    fn confidence_is_high_for_strongly_typed_rust_paths() {
        assert!((confidence_for_reference("crate::foo::bar") - 0.95).abs() < f32::EPSILON);
        assert!((confidence_for_reference("super::baz") - 0.95).abs() < f32::EPSILON);
        assert!((confidence_for_reference("self::quux") - 0.95).abs() < f32::EPSILON);
    }

    #[test]
    fn confidence_is_default_for_generic_or_unresolved_paths() {
        assert!((confidence_for_reference("foo") - 0.5).abs() < f32::EPSILON);
        assert!((confidence_for_reference("std::collections::HashMap") - 0.5).abs() < f32::EPSILON);
        assert!((confidence_for_reference("") - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn import_hint_matches_a_file_under_the_source_root() {
        // The import path is relative to the source root; the file path is
        // relative to the project root.
        assert!(matches_import_hint_exact(
            "src/com/example/util/Formatter.kt",
            "com.example.util.Formatter"
        ));
        assert!(matches_import_hint_exact(
            "src/com/example/util/Formatter.kt",
            "com/example/util/Formatter"
        ));
    }

    #[test]
    fn import_hint_matches_a_file_by_stem() {
        assert!(matches_import_hint_exact(
            "com/example/other/Helper.kt",
            "Helper"
        ));
    }

    #[test]
    fn import_hint_does_not_match_a_different_stem() {
        assert!(!matches_import_hint_exact(
            "com/example/other/Helper.kt",
            "Formatter"
        ));
    }

    #[test]
    fn package_hint_matches_any_file_in_the_package() {
        // `import com.example.util.Helper` pulls a symbol out of the package
        // `com.example.util`, whatever file it lives in.
        assert!(matches_import_package(
            "src/com/example/util/Shapes.kt",
            "com.example.util.Helper"
        ));
        assert!(!matches_import_package(
            "src/com/example/core/Shape.kt",
            "com.example.util.Helper"
        ));
    }

    /// A path that reaches the resolver with Windows separators has to
    /// match exactly like the same path with forward slashes.
    ///
    /// The database used to store `src\com\example\util\Formatter.kt` on
    /// Windows, which matched neither the import hint nor the package
    /// comparison, so every cross-package call in a Windows checkout
    /// resolved to nothing.
    #[test]
    fn windows_separators_match_the_same_way_as_forward_slashes() {
        assert!(matches_import_hint(
            "src\\com\\example\\util\\Formatter.kt",
            "com.example.util.Formatter"
        ));
        assert!(matches_import_package(
            "src\\com\\example\\util\\Shapes.kt",
            "com.example.util.Helper"
        ));
        // Two packages under the same root, as in a JVM layout.
        assert!(in_same_package(
            "src/com/example/util/Formatter.kt",
            "src/com/example/core/Shape.kt"
        ));
        assert!(in_same_package(
            "src\\com\\example\\util\\Formatter.kt",
            "src\\com\\example\\core\\Shape.kt"
        ));
    }

    fn node(id: &str, name: &str, file_path: &str) -> Node {
        Node {
            id: id.to_string(),
            kind: NodeKind::Function,
            name: name.to_string(),
            qualified_name: name.to_string(),
            file_path: file_path.to_string(),
            language: Language::Kotlin,
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
            updated_at: 0,
        }
    }

    /// The candidate ranking has to pick the same target whether the paths
    /// use `/` or `\`.
    ///
    /// Windows used to store `src\com\example\util\Formatter.kt`, which
    /// matched neither the import hint nor the package comparison, so the
    /// candidate fell into the "unrelated" bucket — and a call reference
    /// with an unrelated candidate produces no edge at all. Every
    /// cross-package call in a Windows checkout therefore resolved to
    /// nothing.
    #[test]
    fn candidate_ranking_is_the_same_for_windows_and_unix_paths() {
        let conn = rusqlite::Connection::open_in_memory().expect("open database");
        conn.execute_batch(crate::db::SCHEMA_SQL)
            .expect("apply schema");
        let caller = node("caller", "run", "src\\com\\example\\app\\Main.kt");
        let target = node("target", "format", "src\\com\\example\\util\\Formatter.kt");
        let hint = ImportHint {
            module_path: "com.example.util.Formatter".to_string(),
            export_name: None,
        };

        // A candidate in an unrelated package must not be picked up: that
        // is the #43 guard, and it has to hold for Windows paths too.
        let unrelated = node("unrelated", "format", "src\\com\\other\\Format.kt");

        let ranked = rank_candidates(
            &conn,
            vec![target, unrelated],
            Some(&caller),
            Some(&hint),
            "format",
            EdgeKind::Calls,
        )
        .expect("rank candidates");

        assert_eq!(
            ranked.len(),
            1,
            "only the imported candidate may be selected, got {ranked:?}"
        );
        assert_eq!(ranked.first().map(|n| n.id.as_str()), Some("target"));
    }

    #[test]
    fn combined_import_hint_matches_either_flavour() {
        assert!(matches_import_hint(
            "src/com/example/util/Formatter.kt",
            "com.example.util.Formatter"
        ));
        assert!(matches_import_hint(
            "src/com/example/util/Shapes.kt",
            "com.example.util.Helper"
        ));
        assert!(!matches_import_hint(
            "src/com/example/core/Shape.kt",
            "com.example.util.Helper"
        ));
    }
}
