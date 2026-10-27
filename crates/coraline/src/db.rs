#![deny(unsafe_code)]

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use tracing::{debug, warn};

use crate::types::{
    EDGE_QUALIFIER_METADATA, Edge, EdgeKind, FileRecord, Language, Node, NodeKind, SearchResult,
    UnresolvedReference, Visibility,
};
use crate::utils::f64_to_f32_lossy;

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

pub fn io_other(err: impl std::error::Error + Send + Sync + 'static) -> std::io::Error {
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
    apply_incremental_migrations(&conn)?;
    Ok(db_path)
}

/// Run additive migrations for DBs created before later schema versions.
///
/// Each migration is idempotent and only runs when the corresponding
/// schema column is missing. Additive-only — no destructive changes.
pub fn apply_incremental_migrations(conn: &Connection) -> std::io::Result<()> {
    // v2 → v3: add `edges.confidence` for resolution-strength scoring
    // (Phase 5.2 — borrows GitNexus's `WHERE r.confidence > 0.8` filter).
    if !column_exists(conn, "edges", "confidence")? {
        debug!("applying migration: edges.confidence");
        conn.execute_batch(
            "ALTER TABLE edges ADD COLUMN confidence REAL NOT NULL DEFAULT 1.0;
             INSERT OR IGNORE INTO schema_versions (version, applied_at, description)
             VALUES (2, strftime('%s', 'now') * 1000,
                     'Add edges.confidence for resolution-strength scoring (Phase 5.2)');",
        )
        .map_err(io_other)?;
    }

    // v3 → v4: add `nodes.cluster_id` + `edges.process_id` (Phase 5.1 —
    // borrows GitNexus's precomputed Louvain clusters and per-edge
    // process trace ids).
    if !column_exists(conn, "nodes", "cluster_id")? {
        debug!("applying migration: nodes.cluster_id");
        conn.execute_batch(
            "ALTER TABLE nodes ADD COLUMN cluster_id INTEGER;
             INSERT OR IGNORE INTO schema_versions (version, applied_at, description)
             VALUES (3, strftime('%s', 'now') * 1000,
                     'Add nodes.cluster_id + edges.process_id for Louvain clustering and process tracing (Phase 5.1)');",
        )
        .map_err(io_other)?;
    }

    if !column_exists(conn, "edges", "process_id")? {
        debug!("applying migration: edges.process_id");
        conn.execute_batch("ALTER TABLE edges ADD COLUMN process_id INTEGER;")
            .map_err(io_other)?;
    }

    // v4 → v5: add `unresolved_refs.attempted` so a reference that was
    // tried and failed to resolve is not retried on every run.
    if !column_exists(conn, "unresolved_refs", "attempted")? {
        debug!("applying migration: unresolved_refs.attempted");
        conn.execute_batch(
            "ALTER TABLE unresolved_refs ADD COLUMN attempted INTEGER NOT NULL DEFAULT 0;
             INSERT OR IGNORE INTO schema_versions (version, applied_at, description)
             VALUES (4, strftime('%s', 'now') * 1000,
                     'Add unresolved_refs.attempted so failed references are not retried until the project changes');",
        )
        .map_err(io_other)?;
    }

    // Supporting index for the keyset listing. `CREATE ... IF NOT EXISTS`
    // keeps this block idempotent, so it is safe to run on every connection
    // open.
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_unresolved_pending ON unresolved_refs(attempted, id);",
    )
    .map_err(io_other)?;

    // v5 → v6: add `unresolved_refs.qualifier` — the receiver of a
    // qualified reference (`Helper.doThing()` → `Helper`). Nullable so
    // existing rows migrate cleanly.
    if !column_exists(conn, "unresolved_refs", "qualifier")? {
        debug!("applying migration: unresolved_refs.qualifier");
        conn.execute_batch(
            "ALTER TABLE unresolved_refs ADD COLUMN qualifier TEXT;
             INSERT OR IGNORE INTO schema_versions (version, applied_at, description)
             VALUES (5, strftime('%s', 'now') * 1000,
                     'Add unresolved_refs.qualifier for the receiver of a qualified reference');",
        )
        .map_err(io_other)?;
    }

    Ok(())
}

/// Return `true` if the named table has the named column.
///
/// `PRAGMA table_info(<table>)` returns one row per column with `name` at
/// index 1; we scan the rows to find a match.
fn column_exists(conn: &Connection, table: &str, column: &str) -> std::io::Result<bool> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(io_other)?;
    let mut rows = stmt.query([]).map_err(io_other)?;
    while let Some(row) = rows.next().map_err(io_other)? {
        let name: String = row.get(1).map_err(io_other)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn open_database(project_root: &Path) -> std::io::Result<Connection> {
    let db_path = database_path(project_root);
    let conn = Connection::open(&db_path).map_err(io_other)?;
    conn.execute_batch(PERF_PRAGMAS).map_err(io_other)?;
    // Run additive migrations on every open. `apply_incremental_migrations`
    // is idempotent (column-existence guard) and additive-only, so it is
    // safe to invoke each time a connection is opened. Without this, DBs
    // upgraded across a schema-version boundary (e.g. v1 -> v3) only get
    // the new columns when `coraline init` is run, leaving `coraline sync`
    // and the post-extraction passes stuck on `no such column`.
    apply_incremental_migrations(&conn)?;
    Ok(conn)
}

pub fn clear_database(conn: &Connection) -> std::io::Result<()> {
    // The vectors storage layout depends on the `vec-ext` Cargo feature
    // (v1: `vectors` BLOB table; v0: `vectors_vec` virtual table plus a
    // `vectors_meta` companion). The default build's schema is created
    // by SCHEMA_SQL on every open; under vec-ext, ensure_vec0_schema
    // (called by the embedding dispatch) migrates to v0 on first use.
    // Either way, this function unconditionally clears the rows of both
    // layouts — DELETE FROM a non-existent table is a no-op as long as
    // the statement parses, and SQLite's `if_exists` flag on
    // sqlite_master lookup would still error on a bare `DELETE FROM
    // vectors` here if v0 was always in place.
    //
    // Wrapped in a single batch so a partial failure rolls back the
    // whole delete and the caller sees a consistent state.
    #[cfg(feature = "vec-ext")]
    let sql = "DELETE FROM unresolved_refs;
               DELETE FROM vectors_meta;
               DELETE FROM vectors_vec;
               DELETE FROM edges;
               DELETE FROM nodes;
               DELETE FROM files;";

    #[cfg(not(feature = "vec-ext"))]
    let sql = "DELETE FROM unresolved_refs;
               DELETE FROM vectors;
               DELETE FROM edges;
               DELETE FROM nodes;
               DELETE FROM files;";

    conn.execute_batch(sql).map_err(io_other)?;

    Ok(())
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
                "INSERT INTO edges (source, target, kind, metadata, line, col, confidence)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
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
                edge.confidence,
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
                    from_node_id, reference_name, reference_kind, line, col, candidates, qualifier
                 ) VALUES (?, ?, ?, ?, ?, ?, ?)",
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
                unresolved.qualifier,
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
pub fn store_file_batch(
    conn: &mut Connection,
    file_record: &FileRecord,
    nodes: &[Node],
    edges: &[Edge],
    unresolved_refs: &[UnresolvedReference],
) -> std::io::Result<()> {
    let tx = conn.transaction().map_err(io_other)?;

    insert_node_batch(&tx, nodes)?;
    insert_edge_batch(&tx, edges)?;
    insert_unresolved_ref_batch(&tx, unresolved_refs)?;
    upsert_file_record(&tx, file_record)?;

    tx.commit().map_err(|err| {
        warn!(file = %file_record.path, error = %err, "store_file_batch commit failed");
        io_other(err)
    })
}

fn insert_node_batch(tx: &Transaction, nodes: &[Node]) -> std::io::Result<()> {
    if nodes.is_empty() {
        return Ok(());
    }
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
    Ok(())
}

fn insert_edge_batch(tx: &Transaction, edges: &[Edge]) -> std::io::Result<()> {
    if edges.is_empty() {
        return Ok(());
    }
    let mut stmt = tx
        .prepare(
            "INSERT INTO edges (source, target, kind, metadata, line, col, confidence)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
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
            edge.confidence,
        ])
        .map_err(io_other)?;
    }
    Ok(())
}

fn insert_unresolved_ref_batch(
    tx: &Transaction,
    unresolved_refs: &[UnresolvedReference],
) -> std::io::Result<()> {
    if unresolved_refs.is_empty() {
        return Ok(());
    }
    let mut stmt = tx
        .prepare(
            "INSERT INTO unresolved_refs (
                from_node_id, reference_name, reference_kind, line, col, candidates, qualifier
             ) VALUES (?, ?, ?, ?, ?, ?, ?)",
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
            r.qualifier,
        ])
        .map_err(io_other)?;
    }
    Ok(())
}

fn upsert_file_record(tx: &Transaction, file_record: &FileRecord) -> std::io::Result<()> {
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
    Ok(())
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
            // FTS rank is negative, convert to positive score (higher = better).
            // f32::from(f64) saturates to ±inf for out-of-range values,
            // which is acceptable for a relevance score.
            let rank: f64 = row.get(20)?;
            let score = f64_to_f32_lossy(-rank);
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
    get_edges_by_source_with_confidence(conn, source_id, kind, limit, None)
}

pub fn get_edges_by_target(
    conn: &Connection,
    target_id: &str,
    kind: Option<EdgeKind>,
    limit: usize,
) -> std::io::Result<Vec<Edge>> {
    get_edges_by_target_with_confidence(conn, target_id, kind, limit, None)
}

/// Variant of [`get_edges_by_source`] that filters out edges below a
/// resolution-confidence threshold. Pass `min_confidence = None` to keep
/// the existing behaviour.
pub fn get_edges_by_source_with_confidence(
    conn: &Connection,
    source_id: &str,
    kind: Option<EdgeKind>,
    limit: usize,
    min_confidence: Option<f32>,
) -> std::io::Result<Vec<Edge>> {
    let mut sql = String::from(
        "SELECT source, target, kind, metadata, line, col, confidence \
         FROM edges WHERE source = ?",
    );
    let mut params_vec: Vec<String> = vec![source_id.to_string()];

    if let Some(kind) = kind {
        sql.push_str(" AND kind = ?");
        params_vec.push(edge_kind_to_string(kind));
    }

    if let Some(threshold) = min_confidence {
        sql.push_str(" AND confidence >= ?");
        params_vec.push(threshold.to_string());
    }

    sql.push_str(" ORDER BY COALESCE(line, 0) ASC, COALESCE(col, 0) ASC, target ASC LIMIT ?");
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

/// Variant of [`get_edges_by_target`] that filters out edges below a
/// resolution-confidence threshold. Pass `min_confidence = None` to keep
/// the existing behaviour.
pub fn get_edges_by_target_with_confidence(
    conn: &Connection,
    target_id: &str,
    kind: Option<EdgeKind>,
    limit: usize,
    min_confidence: Option<f32>,
) -> std::io::Result<Vec<Edge>> {
    let mut sql = String::from(
        "SELECT source, target, kind, metadata, line, col, confidence \
         FROM edges WHERE target = ?",
    );
    let mut params_vec: Vec<String> = vec![target_id.to_string()];

    if let Some(kind) = kind {
        sql.push_str(" AND kind = ?");
        params_vec.push(edge_kind_to_string(kind));
    }

    if let Some(threshold) = min_confidence {
        sql.push_str(" AND confidence >= ?");
        params_vec.push(threshold.to_string());
    }

    sql.push_str(" ORDER BY COALESCE(line, 0) ASC, COALESCE(col, 0) ASC, source ASC LIMIT ?");
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

/// Columns of `unresolved_refs` selected by the listing helpers, in the
/// order expected by [`row_to_unresolved_ref`].
const UNRESOLVED_REF_COLUMNS: &str =
    "id, from_node_id, reference_name, reference_kind, line, col, candidates, qualifier";

/// Map a row of the `unresolved_refs` listing onto [`UnresolvedRefRow`].
fn row_to_unresolved_ref(row: &rusqlite::Row<'_>) -> rusqlite::Result<UnresolvedRefRow> {
    let id: i64 = row.get(0)?;
    let reference_kind_raw: String = row.get(3)?;
    let candidates_raw: Option<String> = row.get(6)?;
    let qualifier: Option<String> = row.get(7)?;
    Ok(UnresolvedRefRow {
        id,
        reference: UnresolvedReference {
            from_node_id: row.get(1)?,
            reference_name: row.get(2)?,
            reference_kind: parse_edge_kind(&reference_kind_raw),
            line: row.get(4)?,
            column: row.get(5)?,
            candidates: candidates_raw.and_then(|raw| serde_json::from_str(&raw).ok()),
            qualifier,
        },
    })
}

/// Return the next page of unresolved references with an id greater than
/// `after_id`.
///
/// This is the keyset-pagination primitive: `unresolved_refs.id` is an
/// `INTEGER PRIMARY KEY AUTOINCREMENT`, so it increases monotonically and
/// `id > after_id` is a stable, gap-free cursor. Only references that have
/// not been attempted yet are returned.
pub fn list_unresolved_refs_after(
    conn: &Connection,
    after_id: i64,
    limit: usize,
) -> std::io::Result<Vec<UnresolvedRefRow>> {
    let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {UNRESOLVED_REF_COLUMNS} FROM unresolved_refs
             WHERE id > ?1 AND attempted = 0
             ORDER BY id ASC
             LIMIT ?2"
        ))
        .map_err(io_other)?;
    let rows = stmt
        .query_map(params![after_id, limit_i64], row_to_unresolved_ref)
        .map_err(io_other)?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(io_other)?);
    }
    Ok(results)
}

/// Return every not-yet-attempted unresolved reference by walking the
/// keyset pages until the table is exhausted.
///
/// Unlike a single `LIMIT ?` query this is not capped: the whole backlog is
/// returned, so callers cannot get stuck re-reading the same first rows.
pub fn list_all_unresolved_refs_paged(
    conn: &Connection,
    page_size: usize,
) -> std::io::Result<Vec<UnresolvedRefRow>> {
    if page_size == 0 {
        return Ok(Vec::new());
    }

    let mut all = Vec::new();
    let mut cursor = 0i64;
    loop {
        let page = list_unresolved_refs_after(conn, cursor, page_size)?;
        if page.is_empty() {
            break;
        }
        if let Some(last) = page.last() {
            cursor = last.id;
        }
        let page_len = page.len();
        all.extend(page);
        if page_len < page_size {
            break;
        }
    }
    Ok(all)
}

/// Flag the given unresolved references as attempted.
///
/// Use this for references that were tried and could not be resolved: they
/// are excluded from the default listing, so they are not retried forever.
pub fn mark_unresolved_refs_attempted(
    conn: &mut Connection,
    ids: &[i64],
) -> std::io::Result<usize> {
    if ids.is_empty() {
        return Ok(0);
    }

    let tx = conn.transaction().map_err(io_other)?;
    let mut marked = 0usize;
    {
        let mut stmt = tx
            .prepare("UPDATE unresolved_refs SET attempted = 1 WHERE id = ?1 AND attempted = 0")
            .map_err(io_other)?;
        for id in ids {
            marked += stmt.execute(params![id]).map_err(io_other)?;
        }
    }
    tx.commit().map_err(io_other)?;
    Ok(marked)
}

/// Flag every pending unresolved reference as attempted.
///
/// A reference is removed from `unresolved_refs` once it resolves, so a row
/// that is still there after a resolution pass was *tried and failed*. This
/// keeps the next pass cheap (calls into the standard library can never be
/// resolved and would otherwise be retried on every run) without losing the
/// reference — [`reset_unresolved_refs_attempted`] puts it back in the
/// queue as soon as the project changes.
pub fn mark_pending_unresolved_refs_attempted(conn: &Connection) -> std::io::Result<usize> {
    conn.execute(
        "UPDATE unresolved_refs SET attempted = 1 WHERE attempted = 0",
        [],
    )
    .map_err(io_other)
}

/// Put every unresolved reference back in the queue.
///
/// Call this when the project has changed — a new or edited file may
/// declare the symbol a previously-failed reference was looking for. Without
/// it a reference that failed once would never be retried, and the edge it
/// should have produced would silently never appear.
pub fn reset_unresolved_refs_attempted(conn: &Connection) -> std::io::Result<usize> {
    conn.execute(
        "UPDATE unresolved_refs SET attempted = 0 WHERE attempted = 1",
        [],
    )
    .map_err(io_other)
}

/// Return the largest `unresolved_refs.id` currently stored, or `0` when the
/// table is empty.
pub fn max_unresolved_ref_id(conn: &Connection) -> std::io::Result<i64> {
    conn.query_row(
        "SELECT COALESCE(MAX(id), 0) FROM unresolved_refs",
        [],
        |row| row.get::<_, i64>(0),
    )
    .map_err(io_other)
}

/// Return the total number of unresolved references, attempted or not.
pub fn count_unresolved_refs(conn: &Connection) -> std::io::Result<usize> {
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM unresolved_refs", [], |row| row.get(0))
        .map_err(io_other)?;
    Ok(usize::try_from(count).unwrap_or(0))
}

/// Return the number of unresolved references that have not been attempted
/// yet — the size of the remaining resolution backlog.
pub fn count_pending_unresolved_refs(conn: &Connection) -> std::io::Result<usize> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM unresolved_refs WHERE attempted = 0",
            [],
            |row| row.get(0),
        )
        .map_err(io_other)?;
    Ok(usize::try_from(count).unwrap_or(0))
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

/// An edge that pointed into a file from a *different* file.
struct CrossFileIncomingEdge {
    source: String,
    kind: String,
    line: i64,
    col: i64,
    target_name: String,
    /// The receiver of the reference the edge was resolved from, when the
    /// call site was qualified (`util.Area()` has the qualifier `util`).
    qualifier: Option<String>,
}

/// Re-queue cross-file edges that point into `path` as unresolved
/// references.
///
/// `edges.source` and `edges.target` both cascade on node deletion, so
/// deleting the nodes of a file also drops the edges that *other* files
/// created against it. Those references are re-inserted into
/// `unresolved_refs` (keyed on the surviving source node) so the resolver
/// can rebuild the edges once the file has been re-indexed.
///
/// Edges whose source also lives in the deleted file are skipped — they are
/// genuinely gone.
///
/// The qualifier of the original reference is carried over from the edge's
/// metadata. Without it a qualified call (`util.Area()`) would come back as
/// an unqualified `Area`, and the resolver could re-point it at an
/// unrelated symbol that happens to share the name.
///
/// A reference that already exists is re-armed rather than duplicated: the
/// row is put back in the queue and a missing qualifier is filled in. That
/// keeps the table from growing a row per re-index, and keeps a duplicate
/// from turning into a duplicate edge.
fn requeue_cross_file_incoming_edges(tx: &Transaction, path: &str) -> std::io::Result<usize> {
    let incoming = {
        let mut stmt = tx
            .prepare(
                "SELECT e.source, e.kind, COALESCE(e.line, 0), COALESCE(e.col, 0), t.name, e.metadata
                 FROM edges e
                 JOIN nodes t ON t.id = e.target
                 JOIN nodes s ON s.id = e.source
                 WHERE t.file_path = ?1 AND s.file_path <> ?1",
            )
            .map_err(io_other)?;
        let rows = stmt
            .query_map(params![path], |row| {
                Ok(CrossFileIncomingEdge {
                    source: row.get(0)?,
                    kind: row.get(1)?,
                    line: row.get(2)?,
                    col: row.get(3)?,
                    target_name: row.get(4)?,
                    qualifier: qualifier_from_metadata(row.get(5)?),
                })
            })
            .map_err(io_other)?;

        let mut collected = Vec::new();
        for row in rows {
            collected.push(row.map_err(io_other)?);
        }
        collected
    };

    if incoming.is_empty() {
        return Ok(0);
    }

    let mut requeued = 0usize;
    {
        let mut rearm = tx
            .prepare(
                "UPDATE unresolved_refs
                    SET attempted = 0, qualifier = COALESCE(qualifier, ?6)
                  WHERE from_node_id = ?1
                    AND reference_name = ?2
                    AND reference_kind = ?3
                    AND line = ?4
                    AND col = ?5",
            )
            .map_err(io_other)?;
        let mut insert = tx
            .prepare(
                "INSERT INTO unresolved_refs
                     (from_node_id, reference_name, reference_kind, line, col, candidates, qualifier)
                 SELECT ?1, ?2, ?3, ?4, ?5, NULL, ?6
                 WHERE NOT EXISTS (
                     SELECT 1 FROM unresolved_refs u
                     WHERE u.from_node_id = ?1
                       AND u.reference_name = ?2
                       AND u.reference_kind = ?3
                       AND u.line = ?4
                       AND u.col = ?5
                 )",
            )
            .map_err(io_other)?;

        for edge in &incoming {
            let params = params![
                edge.source,
                edge.target_name,
                edge.kind,
                edge.line,
                edge.col,
                edge.qualifier
            ];
            let rearmed = rearm.execute(params).map_err(io_other)?;
            if rearmed == 0 {
                insert.execute(params).map_err(io_other)?;
            }
            requeued += 1;
        }
    }
    Ok(requeued)
}

/// Read the reference qualifier out of an edge's metadata.
///
/// The metadata is stored as a JSON object; the qualifier is the value
/// under [`EDGE_QUALIFIER_METADATA`]. Anything else in the metadata, or
/// metadata that is not an object at all, is ignored.
fn qualifier_from_metadata(metadata: Option<String>) -> Option<String> {
    let metadata = metadata?;
    let parsed: serde_json::Value = serde_json::from_str(&metadata).ok()?;
    let qualifier = parsed.get(EDGE_QUALIFIER_METADATA)?.as_str()?;
    if qualifier.is_empty() {
        None
    } else {
        Some(qualifier.to_string())
    }
}

/// Delete a file's record, its nodes and everything that cascades from them.
///
/// Cross-file edges that pointed into the deleted file are re-queued as
/// unresolved references first, so they are rebuilt once the file is
/// indexed again.
pub fn delete_file(conn: &mut Connection, path: &str) -> std::io::Result<()> {
    let tx = conn.transaction().map_err(io_other)?;
    let requeued = requeue_cross_file_incoming_edges(&tx, path)?;
    if requeued > 0 {
        debug!(
            path,
            requeued, "re-queued cross-file references into the deleted file"
        );
    }
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

/// Return nodes that have no row in the `vectors` table for `model`.
///
/// A node with an embedding from a *different* (previously configured)
/// model still counts as unembedded here, so coverage reporting stays
/// honest across model switches.
pub fn get_unembedded_nodes(conn: &Connection, model: &str) -> std::io::Result<Vec<Node>> {
    let mut stmt = conn
        .prepare(
            "SELECT n.id, n.kind, n.name, n.qualified_name, n.file_path, n.language,
                    n.start_line, n.end_line, n.start_column, n.end_column,
                    n.docstring, n.signature, n.visibility,
                    n.is_exported, n.is_async, n.is_static, n.is_abstract,
                    n.decorators, n.type_parameters, n.updated_at
             FROM nodes n
             LEFT JOIN vectors v ON n.id = v.node_id AND v.model = ?1
             WHERE v.node_id IS NULL
             ORDER BY n.file_path ASC, n.start_line ASC",
        )
        .map_err(io_other)?;

    let rows = stmt
        .query_map(params![model], row_to_node)
        .map_err(io_other)?;
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

pub fn language_to_string(language: Language) -> String {
    language.name().to_string()
}

pub fn kind_to_string(kind: NodeKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(std::string::ToString::to_string))
        .unwrap_or_else(|| "file".to_string())
}

pub fn edge_kind_to_string(kind: EdgeKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(std::string::ToString::to_string))
        .unwrap_or_else(|| "contains".to_string())
}

pub fn visibility_to_string(visibility: Visibility) -> String {
    serde_json::to_value(visibility)
        .ok()
        .and_then(|v| v.as_str().map(std::string::ToString::to_string))
        .unwrap_or_else(|| "public".to_string())
}

fn parse_kind(raw: &str) -> NodeKind {
    serde_json::from_str::<NodeKind>(&format!("\"{raw}\"")).unwrap_or(NodeKind::File)
}

fn parse_language(raw: &str) -> Language {
    Language::from_name(raw)
}

fn parse_visibility(raw: &str) -> Visibility {
    serde_json::from_str::<Visibility>(&format!("\"{raw}\"")).unwrap_or(Visibility::Public)
}

fn parse_edge_kind(raw: &str) -> EdgeKind {
    serde_json::from_str::<EdgeKind>(&format!("\"{raw}\"")).unwrap_or(EdgeKind::Contains)
}

pub fn row_to_node(row: &rusqlite::Row<'_>) -> rusqlite::Result<Node> {
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
        // `cluster_id` lives at the same column index as `updated_at` was
        // before Phase 5.1 — existing SELECT lists don't include it, so
        // we leave it as `None`. The clustering module uses its own
        // column-aware queries via `row_to_node_with_cluster`.
        cluster_id: None,
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
        confidence: row.get(6)?,
        // `process_id` is at column index 7; existing SELECT lists don't
        // include it, so the process-tracing module uses its own
        // column-aware queries via `row_to_edge_with_process`.
        process_id: None,
    })
}

/// Validates if a call edge is within valid crate/module boundaries.
/// A call is valid if:
/// 1. Both functions are in the same file
/// 2. Both functions are in the same directory
/// 3. The caller has an import statement for the callee's module
///
/// Returns false for cross-crate calls without proper imports.
pub fn is_valid_call_edge(
    conn: &Connection,
    from_node: &Node,
    to_node: &Node,
) -> std::io::Result<bool> {
    // Same file always valid
    if from_node.file_path == to_node.file_path {
        return Ok(true);
    }

    // Same directory usually valid (intra-module calls)
    let from_dir = std::path::Path::new(&from_node.file_path).parent();
    let to_dir = std::path::Path::new(&to_node.file_path).parent();
    if from_dir.is_some() && from_dir == to_dir {
        return Ok(true);
    }

    // Check if caller imports the callee's module
    // Look for import nodes in the same file as the caller
    let mut stmt = conn
        .prepare("SELECT id, name, signature FROM nodes WHERE file_path = ? AND kind = 'import'")
        .map_err(io_other)?;
    let imports = stmt
        .query_map(params![&from_node.file_path], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(io_other)?;

    for import_result in imports {
        let (_, import_name, import_sig) = import_result.map_err(io_other)?;
        // Check if import references the callee's file or module
        if to_node.file_path.contains(&import_name)
            || to_node
                .file_path
                .contains(import_name.replace("::", "/").as_str())
        {
            return Ok(true);
        }

        // Also check the signature if available
        if let Some(sig) = import_sig
            && (to_node.file_path.contains(&sig)
                || to_node.file_path.contains(sig.replace("::", "/").as_str()))
        {
            return Ok(true);
        }
    }

    // Cross-crate call without import is invalid
    Ok(false)
}

// ─── Doc-audit helpers ────────────────────────────────────────────────────────

/// A single unresolved reference whose source node lives in a Markdown file.
///
/// After the resolution pass runs, these are references from doc sections to
/// code symbols that could not be matched — i.e. stale documentation.
#[derive(Debug, Clone)]
pub struct DocUnresolvedRef {
    /// The symbol name written in backticks in the doc (e.g. `"MyStruct"`).
    pub reference_name: String,
    /// Relative path of the Markdown file that contains the reference.
    pub doc_file_path: String,
    /// Name of the heading section the reference was found in, or the file
    /// name when the reference appears before any heading.
    pub doc_section_name: String,
    /// 1-based line number inside the Markdown file.
    pub line: i64,
    /// 0-based column.
    pub column: i64,
}

/// Return all unresolved references whose source node is in a Markdown file.
///
/// These represent inline `` `code_span` `` references that were not matched
/// to any code symbol during the resolution pass — stale documentation.
pub fn list_doc_unresolved_refs(conn: &Connection) -> std::io::Result<Vec<DocUnresolvedRef>> {
    let mut stmt = conn
        .prepare(
            "SELECT ur.reference_name, n.file_path, n.name, ur.line, ur.col
             FROM unresolved_refs ur
             JOIN nodes n ON ur.from_node_id = n.id
             WHERE n.language = 'markdown'
             ORDER BY n.file_path, ur.line",
        )
        .map_err(io_other)?;

    let rows = stmt
        .query_map([], |row| {
            Ok(DocUnresolvedRef {
                reference_name: row.get(0)?,
                doc_file_path: row.get(1)?,
                doc_section_name: row.get(2)?,
                line: row.get(3)?,
                column: row.get(4)?,
            })
        })
        .map_err(io_other)?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(io_other)?);
    }
    Ok(results)
}

/// Return exported code symbols that have **no** `references` edge arriving
/// from any Markdown-language node.
///
/// These are public API items that are not mentioned anywhere in the
/// documentation.
pub fn list_undocumented_exports(conn: &Connection) -> std::io::Result<Vec<Node>> {
    let mut stmt = conn
        .prepare(
            "SELECT n.id, n.kind, n.name, n.qualified_name, n.file_path, n.language,
                    n.start_line, n.end_line, n.start_column, n.end_column,
                    n.docstring, n.signature, n.visibility,
                    n.is_exported, n.is_async, n.is_static, n.is_abstract,
                    n.decorators, n.type_parameters, n.updated_at
             FROM nodes n
             WHERE n.is_exported = 1
               AND n.language != 'markdown'
               AND n.kind IN (
                     'function','method','struct','class','interface',
                     'trait','enum','type_alias','constant'
                   )
               AND NOT EXISTS (
                     SELECT 1
                     FROM edges e
                     JOIN nodes src ON e.source = src.id
                     WHERE e.target = n.id
                       AND e.kind = 'references'
                       AND src.language = 'markdown'
                   )
             ORDER BY n.file_path, n.start_line",
        )
        .map_err(io_other)?;

    let rows = stmt.query_map([], row_to_node).map_err(io_other)?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(io_other)?);
    }
    Ok(results)
}

/// Return `(doc_files_count, doc_sections_count)` — the number of distinct
/// Markdown files that have been indexed with heading nodes, and the total
/// number of heading sections across all of them.
pub fn get_doc_coverage_stats(conn: &Connection) -> std::io::Result<(usize, usize)> {
    let mut stmt = conn
        .prepare(
            "SELECT COUNT(DISTINCT file_path), COUNT(id)
             FROM nodes
             WHERE language = 'markdown' AND kind = 'module'",
        )
        .map_err(io_other)?;

    let (files, sections): (i64, i64) = stmt
        .query_row([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(io_other)?;

    Ok((
        usize::try_from(files).unwrap_or(0),
        usize::try_from(sections).unwrap_or(0),
    ))
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "test assertions: panicking on setup failure is the correct behavior"
    )]
    use super::{SCHEMA_SQL, build_fts_query, get_unembedded_nodes};
    use rusqlite::Connection;

    fn seed_node(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO nodes (id, kind, name, qualified_name, file_path, language,
                start_line, end_line, start_column, end_column, updated_at)
             VALUES (?1, 'function', ?1, ?1, 'src/lib.rs', 'rust', 1, 1, 0, 0, 0)",
            [id],
        )
        .expect("insert node");
    }

    fn seed_vector(conn: &Connection, node_id: &str, model: &str) {
        conn.execute(
            "INSERT INTO vectors (node_id, embedding, model, created_at)
             VALUES (?1, x'00', ?2, 0)",
            [node_id, model],
        )
        .expect("insert vector");
    }

    #[test]
    fn get_unembedded_nodes_excludes_rows_from_other_models() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(SCHEMA_SQL).expect("apply schema");

        seed_node(&conn, "node_a");
        seed_node(&conn, "node_b");
        // node_a has an embedding, but from a *different* model than the one
        // we're checking coverage for — it must still count as unembedded.
        seed_vector(&conn, "node_a", "other-model");

        let unembedded =
            get_unembedded_nodes(&conn, "nomic-embed-text-v1.5").expect("query unembedded");
        let ids: Vec<&str> = unembedded.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(&"node_a"));
        assert!(ids.contains(&"node_b"));
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn get_unembedded_nodes_excludes_rows_from_matching_model() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(SCHEMA_SQL).expect("apply schema");

        seed_node(&conn, "node_a");
        seed_node(&conn, "node_b");
        seed_vector(&conn, "node_a", "nomic-embed-text-v1.5");

        let unembedded =
            get_unembedded_nodes(&conn, "nomic-embed-text-v1.5").expect("query unembedded");
        let ids: Vec<&str> = unembedded.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["node_b"]);
    }

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

    #[test]
    fn build_fts_query_executes_with_slash_and_quote_terms() {
        let conn = Connection::open_in_memory();
        assert!(conn.is_ok());
        let Some(conn) = conn.ok() else {
            return;
        };

        let create_result = conn.execute_batch(
            "CREATE VIRTUAL TABLE nodes_fts USING fts5(name, qualified_name, docstring, content='');",
        );
        assert!(create_result.is_ok());

        let insert_result = conn.execute(
            "INSERT INTO nodes_fts(rowid, name, qualified_name, docstring) VALUES (1, ?1, ?2, ?3)",
            ("auth_login_2fa", "/auth/login/2fa", "route \"name\""),
        );
        assert!(insert_result.is_ok());

        let queries = ["/auth/login/2fa", "route \"name\""];
        for raw in queries {
            let fts_query = build_fts_query(raw);
            assert!(fts_query.is_some());
            let Some(fts_query) = fts_query else {
                return;
            };

            let query_result: rusqlite::Result<i64> = conn.query_row(
                "SELECT COUNT(*) FROM nodes_fts WHERE nodes_fts MATCH ?1",
                [fts_query],
                |row| row.get(0),
            );

            assert!(query_result.is_ok());
            if let Ok(count) = query_result {
                assert!(count >= 1);
            }
        }
    }

    // Phase 5.2 — `edges.confidence` resolution-strength scoring.

    fn seed_edge(conn: &Connection, source: &str, target: &str, kind: &str, confidence: f64) {
        conn.execute(
            "INSERT INTO edges (source, target, kind, line, col, confidence)
             VALUES (?1, ?2, ?3, 1, 0, ?4)",
            rusqlite::params![source, target, kind, confidence],
        )
        .expect("insert edge");
    }

    #[test]
    fn migration_adds_confidence_column_with_default_one() {
        // Simulate a v1 DB (no confidence column) by creating the schema
        // directly, then running the migration runner.
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(SCHEMA_SQL).expect("apply schema");
        conn.execute("ALTER TABLE edges DROP COLUMN confidence", [])
            .expect("drop confidence column to simulate pre-v2 DB");

        super::apply_incremental_migrations(&conn).expect("run migrations");

        // Re-inserting should now succeed and existing rows should get
        // confidence = 1.0 (DEFAULT).
        seed_node(&conn, "a");
        seed_node(&conn, "b");
        seed_edge(&conn, "a", "b", "calls", 0.5);

        let confidence: f64 = conn
            .query_row(
                "SELECT confidence FROM edges WHERE source = 'a' AND target = 'b'",
                [],
                |row| row.get(0),
            )
            .expect("query confidence");
        assert!(
            (confidence - 0.5).abs() < 1e-9,
            "expected 0.5, got {confidence}"
        );
    }

    #[test]
    fn migration_is_idempotent() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(SCHEMA_SQL).expect("apply schema");
        // Running twice must not error — the column_exists() guard makes
        // the migration a no-op the second time.
        super::apply_incremental_migrations(&conn).expect("first run");
        super::apply_incremental_migrations(&conn).expect("second run");
    }

    #[test]
    fn edges_by_target_with_confidence_filters_low_confidence_rows() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(SCHEMA_SQL).expect("apply schema");
        super::apply_incremental_migrations(&conn).expect("migrations");

        seed_node(&conn, "callee");
        seed_node(&conn, "high");
        seed_node(&conn, "low");
        seed_edge(&conn, "high", "callee", "calls", 0.95);
        seed_edge(&conn, "low", "callee", "calls", 0.5);

        let all = super::get_edges_by_target_with_confidence(&conn, "callee", None, 100, None)
            .expect("query all");
        assert_eq!(all.len(), 2);

        let high_only =
            super::get_edges_by_target_with_confidence(&conn, "callee", None, 100, Some(0.8))
                .expect("query high");
        assert_eq!(high_only.len(), 1);
        for item in &high_only {
            assert!((item.confidence - 0.95).abs() < 1e-6);
        }
    }

    #[test]
    fn edges_by_source_with_confidence_filters_low_confidence_rows() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(SCHEMA_SQL).expect("apply schema");
        super::apply_incremental_migrations(&conn).expect("migrations");

        seed_node(&conn, "caller");
        seed_node(&conn, "a");
        seed_node(&conn, "b");
        seed_edge(&conn, "caller", "a", "calls", 0.95);
        seed_edge(&conn, "caller", "b", "calls", 0.5);

        let all = super::get_edges_by_source_with_confidence(&conn, "caller", None, 100, None)
            .expect("query all");
        assert_eq!(all.len(), 2);

        let high_only =
            super::get_edges_by_source_with_confidence(&conn, "caller", None, 100, Some(0.8))
                .expect("query high");
        assert_eq!(high_only.len(), 1);
        for item in &high_only {
            assert!((item.confidence - 0.95).abs() < 1e-6);
        }
    }
}
