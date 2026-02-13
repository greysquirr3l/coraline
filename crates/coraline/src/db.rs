#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};

use crate::types::{
    Edge, EdgeKind, FileRecord, Language, Node, NodeKind, SearchResult, UnresolvedReference,
    Visibility,
};

pub const DATABASE_FILENAME: &str = "codegraph.db";
pub const SCHEMA_SQL: &str = include_str!("db/schema.sql");

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

    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let conn = rusqlite::Connection::open(&db_path).map_err(io_other)?;
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")
        .map_err(io_other)?;
    conn.execute_batch(SCHEMA_SQL).map_err(io_other)?;
    Ok(db_path)
}

pub fn open_database(project_root: &Path) -> std::io::Result<Connection> {
    let db_path = database_path(project_root);
    let conn = Connection::open(&db_path).map_err(io_other)?;
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")
        .map_err(io_other)?;
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

pub fn search_nodes(
    conn: &Connection,
    query: &str,
    kind: Option<NodeKind>,
    limit: usize,
) -> std::io::Result<Vec<SearchResult>> {
    // Use FTS5 for multi-word searches, or OR each word for better matching
    // Example: "calculator functionality" -> search for nodes containing "calculator" OR "functionality"
    let words: Vec<&str> = query.split_whitespace().collect();
    let fts_query = if words.len() > 1 {
        // Multi-word: OR search (any word matches)
        words.join(" OR ")
    } else {
        // Single word: use as-is
        query.to_string()
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
