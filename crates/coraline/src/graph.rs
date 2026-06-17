#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet, VecDeque};

use crate::db;
use crate::types::{Edge, EdgeKind, Subgraph, TraversalDirection, TraversalOptions};

#[derive(Debug, Default)]
pub struct Graph;

pub fn build_subgraph(
    conn: &rusqlite::Connection,
    roots: &[String],
    options: &TraversalOptions,
) -> std::io::Result<Subgraph> {
    let mut nodes = HashMap::new();
    let mut edges = Vec::new();
    let mut visited = HashSet::new();

    let max_depth = options.max_depth.unwrap_or(1);
    let include_start = options.include_start.unwrap_or(true);
    let limit = options.limit.unwrap_or(200);
    let direction = options.direction.unwrap_or(TraversalDirection::Both);
    let edge_kinds = options.edge_kinds.as_ref();
    let node_kinds = options.node_kinds.as_ref();

    let mut queue = VecDeque::new();
    for root in roots {
        queue.push_back((root.clone(), 0));
    }

    while let Some((node_id, depth)) = queue.pop_front() {
        if depth > max_depth {
            continue;
        }

        if !visited.insert(node_id.clone()) {
            continue;
        }

        if (include_start || depth > 0)
            && let Some(node) = db::get_node_by_id(conn, &node_id)?
            && node_kinds.is_none_or(|kinds| kinds.contains(&node.kind))
        {
            nodes.insert(node_id.clone(), node);
        }

        if edges.len() >= limit {
            break;
        }

        let mut next_edges = Vec::new();
        if direction != TraversalDirection::Incoming {
            next_edges.extend(fetch_edges(conn, &node_id, true, edge_kinds, limit)?);
        }
        if direction != TraversalDirection::Outgoing {
            next_edges.extend(fetch_edges(conn, &node_id, false, edge_kinds, limit)?);
        }

        for edge in next_edges {
            if edges.len() >= limit {
                break;
            }
            let (next_id, next_depth) = if edge.source == node_id {
                (edge.target.clone(), depth + 1)
            } else {
                (edge.source.clone(), depth + 1)
            };
            edges.push(edge);
            if next_depth <= max_depth {
                queue.push_back((next_id, next_depth));
            }
        }
    }

    Ok(Subgraph {
        nodes,
        edges,
        roots: roots.to_vec(),
    })
}

fn fetch_edges(
    conn: &rusqlite::Connection,
    node_id: &str,
    outgoing: bool,
    edge_kinds: Option<&Vec<EdgeKind>>,
    limit: usize,
) -> std::io::Result<Vec<Edge>> {
    let mut results = Vec::new();
    if let Some(kinds) = edge_kinds {
        for kind in kinds {
            let edges = if outgoing {
                db::get_edges_by_source(conn, node_id, Some(*kind), limit)?
            } else {
                db::get_edges_by_target(conn, node_id, Some(*kind), limit)?
            };
            results.extend(edges);
        }
    } else if outgoing {
        results = db::get_edges_by_source(conn, node_id, None, limit)?;
    } else {
        results = db::get_edges_by_target(conn, node_id, None, limit)?;
    }

    Ok(results)
}
