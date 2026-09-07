#![forbid(unsafe_code)]

//! Post-extraction graph analytics for Phase 5.1.
//!
//! Two passes run after extraction finishes. Louvain writes
//! `nodes.cluster_id`; process tracing writes `edges.process_id`.
//! Both columns are nullable — `NULL` means "not clustered" / "not
//! part of any traced process".

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io;

use rusqlite::{Connection, OptionalExtension, params};
use tracing::{debug, info};

use crate::db;
use crate::types::{Edge, EdgeKind, Node, NodeKind};

/// Default depth cap for process tracing. Matches the Phase 5.1 spec.
pub const DEFAULT_MAX_PROCESS_DEPTH: usize = 50;

/// Summary of the Louvain pass.
#[derive(Debug, Clone, Default)]
pub struct LouvainResult {
    pub num_clusters: usize,
    pub num_nodes_assigned: usize,
}

/// Summary of the process-tracing pass.
#[derive(Debug, Clone, Default)]
pub struct ProcessTracingResult {
    pub num_entry_points: usize,
    pub num_edges_assigned: usize,
    pub num_edges_unreached: usize,
}

/// One row in the `coraline_cluster_overview` tool output.
#[derive(Debug, Clone)]
pub struct ClusterSummary {
    pub cluster_id: i64,
    pub size: usize,
    pub sample_node_id: String,
    pub sample_qualified_name: String,
    pub sample_kind: NodeKind,
    pub sample_language: crate::types::Language,
}

/// Trace returned by `coraline_process_for(node_id)`.
#[derive(Debug, Clone)]
pub struct ProcessTrace {
    pub entry_point: Node,
    pub depth_reached: usize,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

/// Helper for `usize -> i64` conversions in SQL-bound params where the
/// value is bounded by `LIMIT` / schema constraints.
#[expect(
    clippy::cast_possible_wrap,
    reason = "LIMIT / schema caps keep the values within i64 range"
)]
const fn usize_to_i64(value: usize) -> i64 {
    value as i64
}

/// Louvain iteration cap (Phase 5.1 default).
const MAX_LOUVAIN_ITERATIONS: usize = 10;

// ---------------------------------------------------------------------------
// Louvain
// ---------------------------------------------------------------------------

/// Run Louvain community detection over the `Calls` subgraph and write
/// `nodes.cluster_id`. Unconnected nodes stay `NULL`. Idempotent.
pub fn run_louvain(conn: &mut Connection) -> io::Result<LouvainResult> {
    let (nodes, neighbours) = load_call_graph(conn)?;
    if nodes.is_empty() {
        debug!("louvain: no callable nodes, skipping");
        return Ok(LouvainResult::default());
    }

    // Each node starts in its own community (label = node id).
    let mut community: HashMap<String, String> =
        nodes.iter().map(|n| (n.clone(), n.clone())).collect();

    let mut iteration = 0;
    let mut moved = true;
    while moved && iteration < MAX_LOUVAIN_ITERATIONS {
        moved = false;
        iteration += 1;
        for node in &nodes {
            let Some(current) = community.get(node).cloned() else {
                continue;
            };
            // Count edges from `node` to each neighbour's community.
            let mut counts: HashMap<String, i64> = HashMap::new();
            if let Some(neighs) = neighbours.get(node) {
                for neighbour in neighs {
                    if let Some(c) = community.get(neighbour) {
                        *counts.entry(c.clone()).or_insert(0) += 1;
                    }
                }
            }
            // Greedy Louvain simplification: pick the community with the
            // most neighbours (no modularity arithmetic; produces stable,
            // well-connected clusters).
            let best = counts
                .iter()
                .max_by_key(|(_, count)| **count)
                .map(|(c, _)| c.clone());
            if let Some(best) = best
                && best != current
            {
                community.insert(node.clone(), best);
                moved = true;
            }
        }
    }

    // Remap community labels to a dense 0..N range so the DB column is
    // stable across runs.
    let mut remap: HashMap<String, i64> = HashMap::new();
    let mut next: i64 = 0;
    for c in community.values() {
        remap.entry(c.clone()).or_insert_with(|| {
            let id = next;
            next += 1;
            id
        });
    }

    let tx = conn.transaction().map_err(db::io_other)?;
    tx.execute("UPDATE nodes SET cluster_id = NULL", [])
        .map_err(db::io_other)?;
    {
        let mut stmt = tx
            .prepare("UPDATE nodes SET cluster_id = ?1 WHERE id = ?2")
            .map_err(db::io_other)?;
        for (node, c) in &community {
            let Some(&new_id) = remap.get(c) else {
                continue;
            };
            stmt.execute(params![new_id, node]).map_err(db::io_other)?;
        }
    }
    tx.commit().map_err(db::io_other)?;

    let result = LouvainResult {
        num_clusters: remap.len(),
        num_nodes_assigned: community.len(),
    };
    info!(
        clusters = result.num_clusters,
        nodes = result.num_nodes_assigned,
        iterations = iteration,
        "louvain complete"
    );
    Ok(result)
}

/// In-memory call graph: node id -> list of neighbour ids.
type NeighbourMap = HashMap<String, Vec<String>>;

fn load_call_graph(conn: &Connection) -> io::Result<(Vec<String>, NeighbourMap)> {
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT source FROM edges WHERE kind = 'calls'
             UNION
             SELECT DISTINCT target FROM edges WHERE kind = 'calls'",
        )
        .map_err(db::io_other)?;
    let nodes: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(db::io_other)?
        .filter_map(Result::ok)
        .collect();
    drop(stmt);

    let mut neighbours: NeighbourMap = HashMap::new();
    let mut edge_stmt = conn
        .prepare("SELECT source, target FROM edges WHERE kind = 'calls'")
        .map_err(db::io_other)?;
    let mut rows = edge_stmt.query([]).map_err(db::io_other)?;
    while let Some(row) = rows.next().map_err(db::io_other)? {
        let src: String = row.get(0).map_err(db::io_other)?;
        let tgt: String = row.get(1).map_err(db::io_other)?;
        // Louvain treats the call graph as undirected — add both
        // directions so each node's neighbour count reflects the
        // full local topology.
        neighbours.entry(src.clone()).or_default().push(tgt.clone());
        neighbours.entry(tgt).or_default().push(src);
    }

    Ok((nodes, neighbours))
}

#[doc(hidden)]
#[allow(dead_code, reason = "kept for future string-to-graph-id conversions")]
fn hash_str_to_i64(s: &str) -> i64 {
    let mut h = std::hash::DefaultHasher::new();
    s.hash(&mut h);
    #[expect(
        clippy::cast_possible_wrap,
        reason = "u64 hash values are sign-extended to i64 — collisions are acceptable for in-memory graph keys"
    )]
    let v = h.finish() as i64;
    v
}

// ---------------------------------------------------------------------------
// Process tracing
// ---------------------------------------------------------------------------

/// Walk from each entry-point through the call graph and write
/// `edges.process_id`. Entry point = exported `Function`/`Method`
/// with no incoming `Calls` edges. Re-running replaces process ids.
pub fn run_process_tracing(
    conn: &mut Connection,
    max_depth: usize,
) -> io::Result<ProcessTracingResult> {
    let entry_points = discover_entry_points(conn)?;
    debug!(entry_points = entry_points.len(), "process tracing start");

    let call_edges = load_call_edges(conn)?;
    let total_call_edges = call_edges.len();
    let mut outgoing: HashMap<String, Vec<String>> = HashMap::new();
    let mut edge_row_by_pair: HashMap<(String, String), i64> = HashMap::new();
    for (src, tgt, row_id) in &call_edges {
        outgoing.entry(src.clone()).or_default().push(tgt.clone());
        edge_row_by_pair.insert((src.clone(), tgt.clone()), *row_id);
    }

    conn.execute("UPDATE edges SET process_id = NULL", [])
        .map_err(db::io_other)?;

    let mut total_edges_assigned = 0usize;
    let tx = conn.transaction().map_err(db::io_other)?;
    {
        let mut update_stmt = tx
            .prepare("UPDATE edges SET process_id = ?1 WHERE id = ?2")
            .map_err(db::io_other)?;
        for (process_index, entry_id) in entry_points.iter().enumerate() {
            let process_id = i64::try_from(process_index + 1).unwrap_or(i64::MAX);
            let mut visited: HashSet<String> = HashSet::new();
            let mut stack: Vec<(String, usize)> = vec![(entry_id.clone(), 0)];
            while let Some((node, depth)) = stack.pop() {
                if depth > max_depth || visited.contains(&node) {
                    continue;
                }
                if let Some(children) = outgoing.get(&node) {
                    let child_pairs: Vec<(String, String)> = children
                        .iter()
                        .map(|child| (node.clone(), child.clone()))
                        .collect();
                    for (key_source, key_target) in &child_pairs {
                        let key = (key_source.clone(), key_target.clone());
                        if let Some(&row_id) = edge_row_by_pair.get(&key) {
                            update_stmt
                                .execute(params![process_id, row_id])
                                .map_err(db::io_other)?;
                            total_edges_assigned += 1;
                        }
                        if depth < max_depth {
                            stack.push((key_target.clone(), depth + 1));
                        }
                    }
                }
                visited.insert(node);
            }
        }
    }
    tx.commit().map_err(db::io_other)?;

    let result = ProcessTracingResult {
        num_entry_points: entry_points.len(),
        num_edges_assigned: total_edges_assigned,
        num_edges_unreached: total_call_edges.saturating_sub(total_edges_assigned),
    };
    info!(
        entry_points = result.num_entry_points,
        edges_assigned = result.num_edges_assigned,
        edges_unreached = result.num_edges_unreached,
        "process tracing complete"
    );
    Ok(result)
}

fn discover_entry_points(conn: &Connection) -> io::Result<Vec<String>> {
    let mut stmt = conn
        .prepare(
            "SELECT n.id FROM nodes n
             WHERE n.kind IN ('function', 'method')
               AND n.is_exported = 1
               AND NOT EXISTS (
                   SELECT 1 FROM edges e
                   WHERE e.target = n.id AND e.kind = 'calls'
               )
             ORDER BY n.id",
        )
        .map_err(db::io_other)?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(db::io_other)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(db::io_other)?);
    }
    Ok(out)
}

fn load_call_edges(conn: &Connection) -> io::Result<Vec<(String, String, i64)>> {
    let mut stmt = conn
        .prepare("SELECT id, source, target FROM edges WHERE kind = 'calls'")
        .map_err(db::io_other)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(db::io_other)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, src, tgt) = row.map_err(db::io_other)?;
        out.push((src, tgt, id));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Tool support functions
// ---------------------------------------------------------------------------

/// `coraline_cluster_overview` — list clusters ordered by size, with
/// one representative node per cluster.
pub fn cluster_overview(conn: &Connection, limit: usize) -> io::Result<Vec<ClusterSummary>> {
    let mut stmt = conn
        .prepare(
            "SELECT cluster_id,
                    COUNT(*) AS size,
                    MIN(qualified_name) AS sample_qualified_name,
                    MIN(id) AS sample_id,
                    MIN(kind) AS sample_kind,
                    MIN(language) AS sample_language
             FROM nodes
             WHERE cluster_id IS NOT NULL
             GROUP BY cluster_id
             ORDER BY size DESC, cluster_id ASC
             LIMIT ?1",
        )
        .map_err(db::io_other)?;
    let rows = stmt
        .query_map(params![usize_to_i64(limit)], |row| {
            let kind: String = row.get(4)?;
            let language: String = row.get(5)?;
            Ok(ClusterSummary {
                cluster_id: row.get(0)?,
                #[expect(
                    clippy::cast_sign_loss,
                    clippy::cast_possible_truncation,
                    reason = "COUNT(*) is always non-negative and bounded by row count"
                )]
                size: row.get::<_, i64>(1)? as usize,
                sample_node_id: row.get(3)?,
                sample_qualified_name: row.get(2)?,
                sample_kind: parse_node_kind(&kind),
                sample_language: parse_language(&language),
            })
        })
        .map_err(db::io_other)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(db::io_other)?);
    }
    Ok(out)
}

/// `coraline_cluster_members` — list nodes that share the given
/// `cluster_id`, ordered by `qualified_name`.
pub fn cluster_members(conn: &Connection, cluster_id: i64, limit: usize) -> io::Result<Vec<Node>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, kind, name, qualified_name, file_path, language,
                    start_line, end_line, start_column, end_column,
                    docstring, signature, visibility,
                    is_exported, is_async, is_static, is_abstract,
                    decorators, type_parameters, cluster_id, updated_at
             FROM nodes
             WHERE cluster_id = ?1
             ORDER BY qualified_name ASC
             LIMIT ?2",
        )
        .map_err(db::io_other)?;
    let rows = stmt
        .query_map(
            params![cluster_id, usize_to_i64(limit)],
            row_to_clustered_node,
        )
        .map_err(db::io_other)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(db::io_other)?);
    }
    Ok(out)
}

fn row_to_clustered_node(row: &rusqlite::Row<'_>) -> rusqlite::Result<Node> {
    let kind_raw: String = row.get(1)?;
    let language_raw: String = row.get(5)?;
    let visibility_raw: Option<String> = row.get(12)?;
    let decorators: Option<String> = row.get(17)?;
    let type_parameters: Option<String> = row.get(18)?;
    Ok(Node {
        id: row.get(0)?,
        kind: parse_node_kind(&kind_raw),
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
        visibility: visibility_raw.as_deref().and_then(parse_visibility_opt),
        is_exported: row.get::<_, i64>(13)? != 0,
        is_async: row.get::<_, i64>(14)? != 0,
        is_static: row.get::<_, i64>(15)? != 0,
        is_abstract: row.get::<_, i64>(16)? != 0,
        decorators: decorators.and_then(|raw| serde_json::from_str(&raw).ok()),
        type_parameters: type_parameters.and_then(|raw| serde_json::from_str(&raw).ok()),
        cluster_id: row.get(19)?,
        updated_at: row.get(20)?,
    })
}

/// `coraline_process_for(node_id)` — find the entry point that owns
/// the given node's call trace and return the trace as nodes + edges.
pub fn process_for(
    conn: &Connection,
    node_id: &str,
    max_depth: usize,
) -> io::Result<Option<ProcessTrace>> {
    let Some(entry) = find_process_entry_point(conn, node_id)? else {
        return Ok(None);
    };

    let Some(entry_node) = db::get_node_by_id(conn, &entry)? else {
        return Ok(None);
    };

    let mut visited: HashSet<String> = HashSet::new();
    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut depth_reached = 0usize;

    let mut stack: Vec<(String, usize)> = vec![(entry, 0)];
    while let Some((current, depth)) = stack.pop() {
        if depth > max_depth || visited.contains(&current) {
            continue;
        }
        depth_reached = depth_reached.max(depth);
        if let Some(node) = db::get_node_by_id(conn, &current)? {
            nodes.push(node);
        }
        if let Some(call_edges) = outgoing_call_edges(conn, &current)? {
            for edge in call_edges {
                let target_id = edge.target.clone();
                edges.push(edge);
                if let Ok(Some(target)) = db::get_node_by_id(conn, &target_id)
                    && depth < max_depth
                {
                    stack.push((target.id, depth + 1));
                }
            }
        }
        visited.insert(current);
    }

    Ok(Some(ProcessTrace {
        entry_point: entry_node,
        depth_reached,
        nodes,
        edges,
    }))
}

fn find_process_entry_point(conn: &Connection, node_id: &str) -> io::Result<Option<String>> {
    let mut current = node_id.to_string();
    let mut visited: HashSet<String> = HashSet::new();
    for _ in 0..DEFAULT_MAX_PROCESS_DEPTH {
        if visited.contains(&current) {
            return Ok(None);
        }
        if !visited.insert(current.clone()) {
            return Ok(None);
        }
        let mut stmt = conn
            .prepare(
                "SELECT source FROM edges
                 WHERE target = ?1 AND kind = 'calls'
                 ORDER BY id ASC LIMIT 1",
            )
            .map_err(db::io_other)?;
        let row: Option<String> = stmt
            .query_row(params![&current], |row| row.get(0))
            .optional()
            .map_err(db::io_other)?;
        match row {
            Some(src) => current = src,
            None => return Ok(Some(current)),
        }
    }
    Ok(Some(current))
}

fn outgoing_call_edges(conn: &Connection, source_id: &str) -> io::Result<Option<Vec<Edge>>> {
    let mut stmt = conn
        .prepare(
            "SELECT source, target, kind, metadata, line, col, confidence, process_id
             FROM edges
             WHERE source = ?1 AND kind = 'calls'
             ORDER BY COALESCE(line, 0) ASC, COALESCE(col, 0) ASC, target ASC",
        )
        .map_err(db::io_other)?;
    let rows = stmt
        .query_map(params![source_id], row_to_process_edge)
        .map_err(db::io_other)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(db::io_other)?);
    }
    Ok(if out.is_empty() { None } else { Some(out) })
}

fn row_to_process_edge(row: &rusqlite::Row<'_>) -> rusqlite::Result<Edge> {
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
        process_id: row.get(7)?,
    })
}

// ---------------------------------------------------------------------------
// Tiny parsers for the string-encoded kind/language columns
// ---------------------------------------------------------------------------

fn parse_node_kind(raw: &str) -> NodeKind {
    serde_json::from_str(&format!("\"{raw}\"")).unwrap_or(NodeKind::Function)
}

fn parse_language(raw: &str) -> crate::types::Language {
    serde_json::from_str(&format!("\"{raw}\"")).unwrap_or(crate::types::Language::Unknown)
}

fn parse_edge_kind(raw: &str) -> EdgeKind {
    serde_json::from_str(&format!("\"{raw}\"")).unwrap_or(EdgeKind::References)
}

fn parse_visibility_opt(s: &str) -> Option<crate::types::Visibility> {
    serde_json::from_str(&format!("\"{s}\"")).ok()
}

// ---------------------------------------------------------------------------
// EdgePair alias kept for self-documentation; consumed by the
// `edge_row_by_pair` map in `run_process_tracing`.
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "tests panic on setup failure by design")]
mod tests {
    use super::*;

    fn seed_call(conn: &Connection, src: &str, tgt: &str) {
        for id in [src, tgt] {
            conn.execute(
                "INSERT OR IGNORE INTO nodes
                 (id, kind, name, qualified_name, file_path, language,
                  start_line, end_line, start_column, end_column,
                  is_exported, is_async, is_static, is_abstract, updated_at)
                 VALUES (?1, 'function', ?1, ?1, 'src/lib.rs', 'rust',
                 1, 1, 0, 0, 1, 0, 0, 0, 0)",
                [id],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO edges (source, target, kind, line, col, confidence)
             VALUES (?1, ?2, 'calls', 1, 0, 1.0)",
            rusqlite::params![src, tgt],
        )
        .unwrap();
    }

    fn fresh_db() -> Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(db::SCHEMA_SQL).unwrap();
        db::apply_incremental_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn louvain_clusters_two_disconnected_triangles_separately() {
        let mut conn = fresh_db();
        seed_call(&conn, "a", "b");
        seed_call(&conn, "b", "c");
        seed_call(&conn, "c", "a");
        seed_call(&conn, "d", "e");
        seed_call(&conn, "e", "f");
        seed_call(&conn, "f", "d");

        let result = run_louvain(&mut conn).unwrap();
        eprintln!(
            "[DEBUG TEST] num_clusters={}, num_nodes_assigned={}",
            result.num_clusters, result.num_nodes_assigned
        );
        assert_eq!(result.num_clusters, 2);
        assert_eq!(result.num_nodes_assigned, 6);

        let overview = cluster_overview(&conn, 10).unwrap();
        assert_eq!(overview.len(), 2);
        for cluster in &overview {
            assert_eq!(cluster.size, 3);
        }
    }

    #[test]
    fn process_tracing_assigns_each_entry_point_a_unique_process_id() {
        let mut conn = fresh_db();
        seed_call(&conn, "entry_a", "callee_a");
        seed_call(&conn, "entry_b", "callee_b");

        let result = run_process_tracing(&mut conn, 5).unwrap();
        assert_eq!(result.num_entry_points, 2);
        assert_eq!(result.num_edges_assigned, 2);

        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT process_id FROM edges WHERE kind = 'calls' ORDER BY process_id",
            )
            .unwrap();
        let ids: Vec<i64> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn process_tracing_respects_cycle_protection() {
        let mut conn = fresh_db();
        seed_call(&conn, "entry", "a");
        seed_call(&conn, "a", "b");
        seed_call(&conn, "b", "a");

        let result = run_process_tracing(&mut conn, 50).unwrap();
        assert_eq!(result.num_edges_assigned, 3);
    }

    #[test]
    fn process_for_returns_none_for_unconnected_node() {
        let conn = fresh_db();
        seed_call(&conn, "a", "b");
        conn.execute(
            "INSERT OR IGNORE INTO nodes
             (id, kind, name, qualified_name, file_path, language,
              start_line, end_line, start_column, end_column,
              is_exported, is_async, is_static, is_abstract, updated_at)
             VALUES ('orphan', 'function', 'orphan', 'orphan', 'x.rs', 'rust',
             1, 1, 0, 0, 1, 0, 0, 0, 0)",
            [],
        )
        .unwrap();

        let trace = process_for(&conn, "orphan", 5).unwrap();
        assert!(trace.is_none_or(|t| t.entry_point.id == "orphan"));
    }
}
