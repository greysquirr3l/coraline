#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};
use tracing::{debug, warn};

use crate::types::{
    Edge, EdgeKind, FileRecord, Language, Node, NodeKind, SearchResult, UnresolvedReference,
    Visibility,
};

pub const DATABASE_FILENAME: &str = "coraline.db";
pub const SCHEMA_SQL: &str = include_str!("db/schema.sql");

/// PRAGMAs applied on every connection open.
///
/// - `foreign_keys = ON`   — enforce referential integrity
/// - `journal_mode = WAL`  — concurrent readers, faster writes
/// - `synchronous = NORMAL`— durable on OS crash, faster than FULL
/// - `cache_size = -65536` — 64 MB page cache (negative = KiB)
/// - `temp_store = MEMORY` — temp tables in RAM
/// - `mmap_size = 268435456` — 256 MB memory-mapped I/O
const PERF_PRAGMAS: &str = "
    PRAGMA foreign_keys  = ON;
    PRAGMA journal_mode  = WAL;
    PRAGMA synchronous   = NORMAL;
    PRAGMA cache_size    = -65536;
    PRAGMA temp_store    = MEMORY;
    PRAGMA mmap_size     = 268435456;
";

#[derive(Debug, Default)]
pub struct Database;

#[derive(Debug, Clone)]
pub struct UnresolvedRefRow {
    pub id: i64,
    pub reference: UnresolvedReference,
}

fn io_other(err: impl std::error::Error + Send + Sync + 'static) -> std::io::Error {
    std::io::Error::other(err)
}

pub fn database_path(project_root: &Path) -> PathBuf {
    project_root.join(".coraline").join(DATABASE_FILENAME)
}

pub fn initialize_database(project_root: &Path) -> std::io::Result<PathBuf> {
    let db_path = database_path(project_root);
    debug!(path = %db_path.display(), "initializing database");

    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let conn = rusqlite::Connection::open(&db_path).map_err(io_other)?;
    conn.execute_batch(PERF_PRAGMAS).map_err(io_other)?;
    conn.execute_batch(SCHEMA_SQL).map_err(io_other)?;
    Ok(db_path)
}

pub fn open_database(project_root: &Path) -> std::io::Result<Connection> {
    let db_path = database_path(project_root);
    let conn = Connection::open(&db_path).map_err(io_other)?;
    conn.execute_batch(PERF_PRAGMAS).map_err(io_other)?;
    Ok(conn)
}

pub fn clear_database(conn: &Connection) -> std::io::Result<()> {
    conn.execute_batch(
        "DELETE FROM unresolved_refs;
         DELETE FROM vectors;
         DELETE FROM edges;
         DELETE FROM nodes;
         DELETE FROM files;",
    )
    .map_err(io_other)
}

pub fn get_file_record(conn: &Connection, path: &str) -> std::io::Result<Option<FileRecord>> {
    let row = conn
        .query_row(
            "SELECT path, content_hash, language, size, modified_at, indexed_at, node_count, errors FROM files WHERE path = ?",
            params![path],
            |row| {
                let errors: Option<String> = row.get(7)?;
                let language_raw: String = row.get(2)?;
                Ok(FileRecord {
                    path: row.get(0)?,
                    content_hash: row.get(1)?,
                    language: parse_language(&language_raw),
                    size: u64::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
                    modified_at: row.get(4)?,
                    indexed_at: row.get(5)?,
                    node_count: row.get(6)?,
                    errors: errors
                        .and_then(|raw| serde_json::from_str(&raw).ok()),
                })
            },
        )
        .optional()
        .map_err(io_other)?;

    Ok(row)
}

pub fn list_files(conn: &Connection) -> std::io::Result<Vec<FileRecord>> {
    let mut stmt = conn
        .prepare(
            "SELECT path, content_hash, language, size, modified_at, indexed_at, node_count, errors FROM files",
        )
        .map_err(io_other)?;
    let rows = stmt
        .query_map([], |row| {
            let errors: Option<String> = row.get(7)?;
            let language_raw: String = row.get(2)?;
            Ok(FileRecord {
                path: row.get(0)?,
                content_hash: row.get(1)?,
                language: parse_language(&language_raw),
                size: u64::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
                modified_at: row.get(4)?,
                indexed_at: row.get(5)?,
                node_count: row.get(6)?,
                errors: errors.and_then(|raw| serde_json::from_str(&raw).ok()),
            })
        })
        .map_err(io_other)?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(io_other)?);
    }
    Ok(results)
}

pub fn upsert_file(conn: &Connection, file: &FileRecord) -> std::io::Result<()> {
    let errors = file
        .errors
        .as_ref()
        .map(|errs| serde_json::to_string(errs).unwrap_or_default());
    conn.execute(
        "INSERT INTO files (path, content_hash, language, size, modified_at, indexed_at, node_count, errors)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(path) DO UPDATE SET
            content_hash = excluded.content_hash,
            language = excluded.language,
            size = excluded.size,
            modified_at = excluded.modified_at,
            indexed_at = excluded.indexed_at,
            node_count = excluded.node_count,
            errors = excluded.errors",
        params![
            file.path,
            file.content_hash,
            language_to_string(file.language),
            i64::try_from(file.size).unwrap_or(i64::MAX),
            file.modified_at,
            file.indexed_at,
            file.node_count,
            errors,
        ],
    )
    .map_err(io_other)?;
    Ok(())
}

pub fn insert_nodes(conn: &mut Connection, nodes: &[Node]) -> std::io::Result<()> {
    let tx = conn.transaction().map_err(io_other)?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO nodes (
                    id, kind, name, qualified_name, file_path, language,
                    start_line, end_line, start_column, end_column,
                    docstring, signature, visibility,
                    is_exported, is_async, is_static, is_abstract,
                    decorators, type_parameters, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .map_err(io_other)?;

        for node in nodes {
            let decorators = node
                .decorators
                .as_ref()
                .map(|vals| serde_json::to_string(vals).unwrap_or_default());
            let type_parameters = node
                .type_parameters
                .as_ref()
                .map(|vals| serde_json::to_string(vals).unwrap_or_default());
            let visibility = node.visibility.map(visibility_to_string);
            stmt.execute(params![
                node.id,
                kind_to_string(node.kind),
                node.name,
                node.qualified_name,
                node.file_path,
                language_to_string(node.language),
                node.start_line,
                node.end_line,
                node.start_column,
                node.end_column,
                node.docstring,
                node.signature,
                visibility,
                i32::from(node.is_exported),
                i32::from(node.is_async),
                i32::from(node.is_static),
                i32::from(node.is_abstract),
                decorators,
                type_parameters,
                node.updated_at,
            ])
            .map_err(io_other)?;
        }
    }
    tx.commit().map_err(io_other)
}

pub fn insert_edges(conn: &mut Connection, edges: &[Edge]) -> std::io::Result<()> {
    let tx = conn.transaction().map_err(io_other)?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO edges (source, target, kind, metadata, line, col)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .map_err(io_other)?;

        for edge in edges {
            let metadata = edge
                .metadata
                .as_ref()
                .map(|vals| serde_json::to_string(vals).unwrap_or_default());
            stmt.execute(params![
                edge.source,
                edge.target,
                edge_kind_to_string(edge.kind),
                metadata,
                edge.line,
                edge.column,
            ])
            .map_err(io_other)?;
        }
    }
    tx.commit().map_err(io_other)
}

pub fn insert_unresolved_refs(
    conn: &mut Connection,
    refs: &[UnresolvedReference],
) -> std::io::Result<()> {
    let tx = conn.transaction().map_err(io_other)?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO unresolved_refs (
                    from_node_id, reference_name, reference_kind, line, col, candidates
                 ) VALUES (?, ?, ?, ?, ?, ?)",
            )
            .map_err(io_other)?;

        for unresolved in refs {
            let candidates = unresolved
                .candidates
                .as_ref()
                .map(|vals| serde_json::to_string(vals).unwrap_or_default());
            stmt.execute(params![
                unresolved.from_node_id,
                unresolved.reference_name,
                edge_kind_to_string(unresolved.reference_kind),
                unresolved.line,
                unresolved.column,
                candidates,
            ])
            .map_err(io_other)?;
        }
    }
    tx.commit().map_err(io_other)
}

/// Store a fully-parsed file's results in a single `SQLite` transaction:
/// nodes, edges, unresolved refs, and the file metadata record.
///
/// This is more efficient than the three separate `insert_nodes` /
/// `insert_edges` / `insert_unresolved_refs` calls because it incurs only
/// one transaction commit instead of three.
#[allow(clippy::too_many_lines)]
pub fn store_file_batch(
    conn: &mut Connection,
    file_record: &FileRecord,
    nodes: &[Node],
    edges: &[Edge],
    unresolved_refs: &[UnresolvedReference],
) -> std::io::Result<()> {
    let tx = conn.transaction().map_err(io_other)?;

    // Nodes
    if !nodes.is_empty() {
        let mut stmt = tx
            .prepare(
                "INSERT INTO nodes (
                    id, kind, name, qualified_name, file_path, language,
                    start_line, end_line, start_column, end_column,
                    docstring, signature, visibility,
                    is_exported, is_async, is_static, is_abstract,
                    decorators, type_parameters, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .map_err(io_other)?;
        for node in nodes {
            let decorators = node
                .decorators
                .as_ref()
                .map(|v| serde_json::to_string(v).unwrap_or_default());
            let type_parameters = node
                .type_parameters
                .as_ref()
                .map(|v| serde_json::to_string(v).unwrap_or_default());
            let visibility = node.visibility.map(visibility_to_string);
            stmt.execute(params![
                node.id,
                kind_to_string(node.kind),
                node.name,
                node.qualified_name,
                node.file_path,
                language_to_string(node.language),
                node.start_line,
                node.end_line,
                node.start_column,
                node.end_column,
                node.docstring,
                node.signature,
                visibility,
                i32::from(node.is_exported),
                i32::from(node.is_async),
                i32::from(node.is_static),
                i32::from(node.is_abstract),
                decorators,
                type_parameters,
                node.updated_at,
            ])
            .map_err(io_other)?;
        }
    }

    // Edges
    if !edges.is_empty() {
        let mut stmt = tx
            .prepare(
                "INSERT INTO edges (source, target, kind, metadata, line, col)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .map_err(io_other)?;
        for edge in edges {
            let metadata = edge
                .metadata
                .as_ref()
                .map(|v| serde_json::to_string(v).unwrap_or_default());
            stmt.execute(params![
                edge.source,
                edge.target,
                edge_kind_to_string(edge.kind),
                metadata,
                edge.line,
                edge.column,
            ])
            .map_err(io_other)?;
        }
    }

    // Unresolved references
    if !unresolved_refs.is_empty() {
        let mut stmt = tx
            .prepare(
                "INSERT INTO unresolved_refs (
                    from_node_id, reference_name, reference_kind, line, col, candidates
                 ) VALUES (?, ?, ?, ?, ?, ?)",
            )
            .map_err(io_other)?;
        for r in unresolved_refs {
            let candidates = r
                .candidates
                .as_ref()
                .map(|v| serde_json::to_string(v).unwrap_or_default());
            stmt.execute(params![
                r.from_node_id,
                r.reference_name,
                edge_kind_to_string(r.reference_kind),
                r.line,
                r.column,
                candidates,
            ])
            .map_err(io_other)?;
        }
    }

    // File record (upsert)
    let errors = file_record
        .errors
        .as_ref()
        .map(|e| serde_json::to_string(e).unwrap_or_default());
    tx.execute(
        "INSERT INTO files (path, content_hash, language, size, modified_at, indexed_at, node_count, errors)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(path) DO UPDATE SET
            content_hash = excluded.content_hash,
            language = excluded.language,
            size = excluded.size,
            modified_at = excluded.modified_at,
            indexed_at = excluded.indexed_at,
            node_count = excluded.node_count,
            errors = excluded.errors",
        params![
            file_record.path,
            file_record.content_hash,
            language_to_string(file_record.language),
            i64::try_from(file_record.size).unwrap_or(i64::MAX),
            file_record.modified_at,
            file_record.indexed_at,
            file_record.node_count,
            errors,
        ],
    )
    .map_err(io_other)?;

    tx.commit().map_err(|err| {
        warn!(file = %file_record.path, error = %err, "store_file_batch commit failed");
        io_other(err)
    })
}

pub fn search_nodes(
    conn: &Connection,
    query: &str,
    kind: Option<NodeKind>,
    limit: usize,
) -> std::io::Result<Vec<SearchResult>> {
    let Some(fts_query) = build_fts_query(query) else {
        return Ok(Vec::new());
    };

    // First try FTS search for better matching
    let mut sql = String::from(
        "SELECT n.id, n.kind, n.name, n.qualified_name, n.file_path, n.language,
                n.start_line, n.end_line, n.start_column, n.end_column,
                n.docstring, n.signature, n.visibility,
                n.is_exported, n.is_async, n.is_static, n.is_abstract,
                n.decorators, n.type_parameters, n.updated_at,
                fts.rank AS score
         FROM nodes n
         INNER JOIN nodes_fts fts ON n.rowid = fts.rowid
         WHERE nodes_fts MATCH ?",
    );

    let mut params_vec: Vec<String> = vec![fts_query];

    if let Some(kind) = kind {
        sql.push_str(" AND n.kind = ?");
        params_vec.push(kind_to_string(kind));
    }

    sql.push_str(" ORDER BY score ASC, length(n.name) ASC LIMIT ?");
    params_vec.push(limit.to_string());

    let mut stmt = conn.prepare(&sql).map_err(io_other)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(params_vec), |row| {
            // FTS rank is negative, convert to positive score (higher = better)
            let rank: f64 = row.get(20)?;
            #[allow(clippy::cast_possible_truncation)]
            let score = (-rank) as f32;
            Ok(SearchResult {
                node: row_to_node(row)?,
                score,
                highlights: None,
            })
        })
        .map_err(io_other)?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(io_other)?);
    }

    Ok(results)
}

fn build_fts_query(query: &str) -> Option<String> {
    let mut terms = query
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")));

    let first = terms.next()?;
    let fts_query = terms.fold(first, |mut acc, term| {
        acc.push_str(" OR ");
        acc.push_str(&term);
        acc
    });

    Some(fts_query)
}

pub fn find_nodes_by_name(conn: &Connection, name: &str) -> std::io::Result<Vec<Node>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, kind, name, qualified_name, file_path, language,
                    start_line, end_line, start_column, end_column,
                    docstring, signature, visibility,
                    is_exported, is_async, is_static, is_abstract,
                    decorators, type_parameters, updated_at
             FROM nodes WHERE name = ?",
        )
        .map_err(io_other)?;
    let rows = stmt
        .query_map(params![name], row_to_node)
        .map_err(io_other)?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(io_other)?);
    }
    Ok(results)
}

pub fn find_exports_by_module(conn: &Connection, module_path: &str) -> std::io::Result<Vec<Node>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, kind, name, qualified_name, file_path, language,
                    start_line, end_line, start_column, end_column,
                    docstring, signature, visibility,
                    is_exported, is_async, is_static, is_abstract,
                    decorators, type_parameters, updated_at
             FROM nodes WHERE kind = ? AND signature = ?",
        )
        .map_err(io_other)?;
    let rows = stmt
        .query_map(
            params![kind_to_string(NodeKind::Export), module_path],
            row_to_node,
        )
        .map_err(io_other)?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(io_other)?);
    }
    Ok(results)
}

pub fn get_node_by_id(conn: &Connection, node_id: &str) -> std::io::Result<Option<Node>> {
    let row = conn
        .query_row(
            "SELECT id, kind, name, qualified_name, file_path, language,
                    start_line, end_line, start_column, end_column,
                    docstring, signature, visibility,
                    is_exported, is_async, is_static, is_abstract,
                    decorators, type_parameters, updated_at
             FROM nodes WHERE id = ?",
            params![node_id],
            row_to_node,
        )
        .optional()
        .map_err(io_other)?;

    Ok(row)
}

pub fn get_edges_by_source(
    conn: &Connection,
    source_id: &str,
    kind: Option<EdgeKind>,
    limit: usize,
) -> std::io::Result<Vec<Edge>> {
    let mut sql = String::from(
        "SELECT source, target, kind, metadata, line, col FROM edges WHERE source = ?",
    );
    let mut params_vec: Vec<String> = vec![source_id.to_string()];

    if let Some(kind) = kind {
        sql.push_str(" AND kind = ?");
        params_vec.push(edge_kind_to_string(kind));
    }

    sql.push_str(" LIMIT ?");
    params_vec.push(limit.to_string());

    let mut stmt = conn.prepare(&sql).map_err(io_other)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(params_vec), row_to_edge)
        .map_err(io_other)?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(io_other)?);
    }
    Ok(results)
}

pub fn get_edges_by_target(
    conn: &Connection,
    target_id: &str,
    kind: Option<EdgeKind>,
    limit: usize,
) -> std::io::Result<Vec<Edge>> {
    let mut sql = String::from(
        "SELECT source, target, kind, metadata, line, col FROM edges WHERE target = ?",
    );
    let mut params_vec: Vec<String> = vec![target_id.to_string()];

    if let Some(kind) = kind {
        sql.push_str(" AND kind = ?");
        params_vec.push(edge_kind_to_string(kind));
    }

    sql.push_str(" LIMIT ?");
    params_vec.push(limit.to_string());

    let mut stmt = conn.prepare(&sql).map_err(io_other)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(params_vec), row_to_edge)
        .map_err(io_other)?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(io_other)?);
    }
    Ok(results)
}

pub fn list_unresolved_refs(
    conn: &Connection,
    limit: usize,
) -> std::io::Result<Vec<UnresolvedRefRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, from_node_id, reference_name, reference_kind, line, col, candidates
             FROM unresolved_refs LIMIT ?",
        )
        .map_err(io_other)?;
    let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);
    let rows = stmt
        .query_map(params![limit_i64], |row| {
            let id: i64 = row.get(0)?;
            let reference_kind_raw: String = row.get(3)?;
            let candidates_raw: Option<String> = row.get(6)?;
            Ok(UnresolvedRefRow {
                id,
                reference: UnresolvedReference {
                    from_node_id: row.get(1)?,
                    reference_name: row.get(2)?,
                    reference_kind: parse_edge_kind(&reference_kind_raw),
                    line: row.get(4)?,
                    column: row.get(5)?,
                    candidates: candidates_raw.and_then(|raw| serde_json::from_str(&raw).ok()),
                },
            })
        })
        .map_err(io_other)?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(io_other)?);
    }
    Ok(results)
}

pub fn delete_unresolved_refs(conn: &mut Connection, ids: &[i64]) -> std::io::Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction().map_err(io_other)?;
    {
        let mut stmt = tx
            .prepare("DELETE FROM unresolved_refs WHERE id = ?")
            .map_err(io_other)?;
        for id in ids {
            stmt.execute(params![id]).map_err(io_other)?;
        }
    }
    tx.commit().map_err(io_other)
}

pub fn delete_file(conn: &mut Connection, path: &str) -> std::io::Result<()> {
    let tx = conn.transaction().map_err(io_other)?;
    tx.execute("DELETE FROM nodes WHERE file_path = ?", params![path])
        .map_err(io_other)?;
    tx.execute("DELETE FROM files WHERE path = ?", params![path])
        .map_err(io_other)?;
    tx.commit().map_err(io_other)
}

/// Get all nodes belonging to a specific file, optionally filtered by kind.
pub fn get_nodes_by_file(
    conn: &Connection,
    file_path: &str,
    kind: Option<NodeKind>,
) -> std::io::Result<Vec<Node>> {
    let mut sql = String::from(
        "SELECT id, kind, name, qualified_name, file_path, language,
                start_line, end_line, start_column, end_column,
                docstring, signature, visibility,
                is_exported, is_async, is_static, is_abstract,
                decorators, type_parameters, updated_at
         FROM nodes WHERE file_path = ?",
    );
    let mut params_vec: Vec<String> = vec![file_path.to_string()];

    if let Some(k) = kind {
        sql.push_str(" AND kind = ?");
        params_vec.push(kind_to_string(k));
    }

    sql.push_str(" ORDER BY start_line ASC");

    let mut stmt = conn.prepare(&sql).map_err(io_other)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(params_vec), row_to_node)
        .map_err(io_other)?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(io_other)?);
    }
    Ok(results)
}

/// Return every node in the database ordered by file path then start line.
pub fn get_all_nodes(conn: &Connection) -> std::io::Result<Vec<Node>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, kind, name, qualified_name, file_path, language,
                    start_line, end_line, start_column, end_column,
                    docstring, signature, visibility,
                    is_exported, is_async, is_static, is_abstract,
                    decorators, type_parameters, updated_at
             FROM nodes
             ORDER BY file_path ASC, start_line ASC",
        )
        .map_err(io_other)?;

    let rows = stmt.query_map([], row_to_node).map_err(io_other)?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(io_other)?);
    }
    Ok(results)
}

/// Return nodes that have no corresponding row in the `vectors` table.
pub fn get_unembedded_nodes(conn: &Connection) -> std::io::Result<Vec<Node>> {
    let mut stmt = conn
        .prepare(
            "SELECT n.id, n.kind, n.name, n.qualified_name, n.file_path, n.language,
                    n.start_line, n.end_line, n.start_column, n.end_column,
                    n.docstring, n.signature, n.visibility,
                    n.is_exported, n.is_async, n.is_static, n.is_abstract,
                    n.decorators, n.type_parameters, n.updated_at
             FROM nodes n
             LEFT JOIN vectors v ON n.id = v.node_id
             WHERE v.node_id IS NULL
             ORDER BY n.file_path ASC, n.start_line ASC",
        )
        .map_err(io_other)?;

    let rows = stmt.query_map([], row_to_node).map_err(io_other)?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(io_other)?);
    }
    Ok(results)
}

/// Database statistics returned by `get_db_stats`.
#[derive(Debug, serde::Serialize)]
pub struct DbStats {
    pub node_count: i64,
    pub edge_count: i64,
    pub file_count: i64,
    pub unresolved_count: i64,
}

/// Return summary statistics for the indexed codebase.
pub fn get_db_stats(conn: &Connection) -> std::io::Result<DbStats> {
    let node_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))
        .map_err(io_other)?;
    let edge_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
        .map_err(io_other)?;
    let file_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
        .map_err(io_other)?;
    let unresolved_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM unresolved_refs", [], |r| r.get(0))
        .map_err(io_other)?;

    Ok(DbStats {
        node_count,
        edge_count,
        file_count,
        unresolved_count,
    })
}

fn language_to_string(language: Language) -> String {
    serde_json::to_value(language)
        .ok()
        .and_then(|v| v.as_str().map(std::string::ToString::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

fn kind_to_string(kind: NodeKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(std::string::ToString::to_string))
        .unwrap_or_else(|| "file".to_string())
}

fn edge_kind_to_string(kind: EdgeKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(std::string::ToString::to_string))
        .unwrap_or_else(|| "contains".to_string())
}

fn visibility_to_string(visibility: Visibility) -> String {
    serde_json::to_value(visibility)
        .ok()
        .and_then(|v| v.as_str().map(std::string::ToString::to_string))
        .unwrap_or_else(|| "public".to_string())
}

fn parse_kind(raw: &str) -> NodeKind {
    serde_json::from_str::<NodeKind>(&format!("\"{raw}\"")).unwrap_or(NodeKind::File)
}

fn parse_language(raw: &str) -> Language {
    serde_json::from_str::<Language>(&format!("\"{raw}\"")).unwrap_or(Language::Unknown)
}

fn parse_visibility(raw: &str) -> Visibility {
    serde_json::from_str::<Visibility>(&format!("\"{raw}\"")).unwrap_or(Visibility::Public)
}

fn parse_edge_kind(raw: &str) -> EdgeKind {
    serde_json::from_str::<EdgeKind>(&format!("\"{raw}\"")).unwrap_or(EdgeKind::Contains)
}

fn row_to_node(row: &rusqlite::Row<'_>) -> rusqlite::Result<Node> {
    let kind_raw: String = row.get(1)?;
    let language_raw: String = row.get(5)?;
    let visibility_raw: Option<String> = row.get(12)?;
    let decorators: Option<String> = row.get(17)?;
    let type_parameters: Option<String> = row.get(18)?;

    Ok(Node {
        id: row.get(0)?,
        kind: parse_kind(&kind_raw),
        name: row.get(2)?,
        qualified_name: row.get(3)?,
        file_path: row.get(4)?,
        language: parse_language(&language_raw),
        start_line: row.get(6)?,
        end_line: row.get(7)?,
        start_column: row.get(8)?,
        end_column: row.get(9)?,
        docstring: row.get(10)?,
        signature: row.get(11)?,
        visibility: visibility_raw.as_deref().map(parse_visibility),
        is_exported: row.get::<_, i64>(13)? != 0,
        is_async: row.get::<_, i64>(14)? != 0,
        is_static: row.get::<_, i64>(15)? != 0,
        is_abstract: row.get::<_, i64>(16)? != 0,
        decorators: decorators.and_then(|raw| serde_json::from_str(&raw).ok()),
        type_parameters: type_parameters.and_then(|raw| serde_json::from_str(&raw).ok()),
        updated_at: row.get(19)?,
    })
}

fn row_to_edge(row: &rusqlite::Row<'_>) -> rusqlite::Result<Edge> {
    let kind_raw: String = row.get(2)?;
    let metadata: Option<String> = row.get(3)?;

    Ok(Edge {
        source: row.get(0)?,
        target: row.get(1)?,
        kind: parse_edge_kind(&kind_raw),
        metadata: metadata.and_then(|raw| serde_json::from_str(&raw).ok()),
        line: row.get(4)?,
        column: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::build_fts_query;

    #[test]
    fn build_fts_query_quotes_slash_terms() {
        assert_eq!(
            build_fts_query("/auth/login/2fa"),
            Some("\"/auth/login/2fa\"".to_string())
        );
    }

    #[test]
    fn build_fts_query_escapes_embedded_quotes() {
        assert_eq!(
            build_fts_query("route \"name\""),
            Some("\"route\" OR \"\"\"name\"\"\"".to_string())
        );
    }

    #[test]
    fn build_fts_query_returns_none_for_blank_input() {
        assert_eq!(build_fts_query("   \n\t  "), None);
    }
}
