//! Integration tests for the runtime detection of the vector storage
//! layout.
//!
//! # The bug this guards against
//!
//! The vector storage layout is a property of the **database file**,
//! not of the binary that opens it. Coraline has two layouts:
//!
//! - **v1 (BLOB)** — a `vectors` table with an `embedding BLOB` column.
//! - **v0 (sqlite-vec)** — a `vectors_vec` `vec0` virtual table plus a
//!   `vectors_meta` companion table.
//!
//! The SQL used to reach those tables used to be selected with
//! `#[cfg(feature = "vec-ext")]` — a compile-time switch standing in
//! for a runtime property. That produced a broken 2x2 matrix:
//!
//! | Build     | DB layout | `coraline stats` | `coraline_stats` | `index --force` |
//! | --------- | --------- | ----------------- | ----------------- | --------------- |
//! | default   | v1        | OK                | OK                | OK              |
//! | default   | v0        | OK                | **FAIL**          | **FAIL**        |
//! | vec-ext   | v1        | **FAIL**          | **FAIL**          | **FAIL**        |
//! | vec-ext   | v0        | **FAIL**          | **FAIL**          | OK              |
//!
//! These tests pin the behaviour for every cell of that matrix.
//!
//! The v0 half of the matrix needs the `vec0` virtual-table module,
//! which only exists in a `vec-ext` build. A default build cannot
//! *create* a v0 layout — but it can absolutely be *handed* one (a
//! project created by a vec-ext build, then opened by a default
//! build), which is exactly the cell that used to fail. The tests
//! below therefore create the v0 layout in whatever way the current
//! build allows, and assert the behaviour that follows.

#![allow(clippy::expect_used)]

use coraline::db;
use coraline::vector_layout::VectorLayout;
use rusqlite::Connection;

/// Create an in-memory database with the v1 BLOB layout applied.
fn v1_db() -> Connection {
    let conn = Connection::open_in_memory().expect("open in-memory db");
    conn.execute_batch(db::SCHEMA_SQL).expect("apply schema");
    conn
}

/// Create an in-memory database with the v0 `vec0` layout applied.
///
/// The `vec0` virtual table can only be created by a build that has
/// the sqlite-vec module registered. When the module is unavailable we
/// build the *observable* shape of a v0 database instead: the
/// `vectors_meta` companion table (an ordinary table, writable by
/// every build) and no `vectors` table. That is the state a default
/// build actually finds on disk, and it is enough to exercise every
/// metadata path.
fn v0_db() -> Connection {
    let conn = Connection::open_in_memory().expect("open in-memory db");
    conn.execute_batch(db::SCHEMA_SQL).expect("apply schema");

    // The v1 `vectors` table is replaced by the v0 pair.
    conn.execute_batch("DROP TABLE IF EXISTS vectors; DROP INDEX IF EXISTS idx_vectors_model;")
        .expect("drop v1 vectors table");

    // Try to create the real vec0 virtual table first. A default build
    // has no sqlite-vec module, so this fails — and that is fine: the
    // `vectors_meta` companion is an ordinary table that every build
    // can create and read.
    let _ =
        conn.execute_batch("CREATE VIRTUAL TABLE vectors_vec USING vec0(embedding float[768]);");

    conn.execute_batch(
        "CREATE TABLE vectors_meta (
             rowid INTEGER PRIMARY KEY,
             node_id TEXT UNIQUE NOT NULL,
             model TEXT NOT NULL,
             created_at INTEGER NOT NULL
         );",
    )
    .expect("create vectors_meta");

    conn
}

/// Seed a node row.
fn seed_node(conn: &Connection, id: &str, updated_at: i64) {
    conn.execute(
        "INSERT INTO nodes (id, kind, name, qualified_name, file_path, language,
            start_line, end_line, start_column, end_column, updated_at)
         VALUES (?1, 'function', ?1, ?1, 'src/lib.rs', 'rust', 1, 1, 0, 0, ?2)",
        rusqlite::params![id, updated_at],
    )
    .expect("insert node");
}

/// Seed an embedding-metadata row into whichever metadata table the
/// database's layout uses.
fn seed_vector_metadata(conn: &Connection, node_id: &str, model: &str, created_at: i64) {
    let layout = VectorLayout::detect(conn).expect("detect layout");
    match layout.metadata_relation() {
        "vectors_meta" => {
            // `vectors_meta.rowid` is the `vec0` rowid — an integer
            // primary key, not the node id.
            let next_rowid: i64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(rowid), 0) + 1 FROM vectors_meta",
                    [],
                    |r| r.get(0),
                )
                .expect("next rowid");
            conn.execute(
                "INSERT INTO vectors_meta (rowid, node_id, model, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![next_rowid, node_id, model, created_at],
            )
            .expect("insert vectors_meta row");
        }
        _ => {
            conn.execute(
                "INSERT INTO vectors (node_id, embedding, model, created_at)
                 VALUES (?1, x'00', ?2, ?3)",
                rusqlite::params![node_id, model, created_at],
            )
            .expect("insert vectors row");
        }
    }
}

// ── Detection ───────────────────────────────────────────────────────────────

#[test]
fn detects_v1_layout() {
    let conn = v1_db();
    assert_eq!(
        VectorLayout::detect(&conn).expect("detect"),
        VectorLayout::V1Blob
    );
}

#[test]
fn detects_v0_layout() {
    let conn = v0_db();
    assert_eq!(
        VectorLayout::detect(&conn).expect("detect"),
        VectorLayout::V0Vec
    );
}

#[test]
fn detects_absent_layout_on_an_empty_database() {
    let conn = Connection::open_in_memory().expect("open in-memory db");
    assert_eq!(
        VectorLayout::detect(&conn).expect("detect"),
        VectorLayout::Absent
    );
}

// ── Metadata queries (the `coraline_stats` / coverage paths) ───────────────

#[test]
fn unembedded_nodes_query_works_on_v1_layout() {
    let conn = v1_db();
    seed_node(&conn, "node_a", 0);
    seed_node(&conn, "node_b", 0);
    seed_vector_metadata(&conn, "node_a", "nomic-embed-text-v1.5", 0);

    let unembedded = db::get_unembedded_nodes(&conn, "nomic-embed-text-v1.5").expect("query");
    let ids: Vec<&str> = unembedded.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, vec!["node_b"]);
}

#[test]
fn unembedded_nodes_query_works_on_v0_layout() {
    // The regression: on a v0 database this used to fail with
    // `no such table: vectors`.
    let conn = v0_db();
    seed_node(&conn, "node_a", 0);
    seed_node(&conn, "node_b", 0);
    seed_vector_metadata(&conn, "node_a", "nomic-embed-text-v1.5", 0);

    let unembedded = db::get_unembedded_nodes(&conn, "nomic-embed-text-v1.5").expect("query");
    let ids: Vec<&str> = unembedded.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, vec!["node_b"]);
}

#[test]
fn unembedded_nodes_query_works_with_no_vector_storage() {
    let conn = Connection::open_in_memory().expect("open in-memory db");
    conn.execute_batch(db::SCHEMA_SQL).expect("apply schema");
    seed_node(&conn, "node_a", 0);

    let unembedded = db::get_unembedded_nodes(&conn, "nomic-embed-text-v1.5").expect("query");
    assert_eq!(unembedded.len(), 1);
}

#[test]
fn vector_count_query_works_on_both_layouts() {
    // Mirrors the `coraline_stats` MCP tool's `SELECT COUNT(*)` against
    // the metadata relation.
    let count_vectors = |conn: &Connection| -> i64 {
        let layout = VectorLayout::detect(conn).expect("detect");
        let relation = layout.metadata_relation();
        conn.query_row(&format!("SELECT COUNT(*) FROM {relation}"), [], |r| {
            r.get(0)
        })
        .expect("count")
    };

    let v1 = v1_db();
    seed_vector_metadata(&v1, "node_a", "m", 0);
    assert_eq!(count_vectors(&v1), 1);

    let v0 = v0_db();
    seed_vector_metadata(&v0, "node_a", "m", 0);
    assert_eq!(count_vectors(&v0), 1);
}

// ── clear_database ──────────────────────────────────────────────────────────

#[test]
fn clear_database_works_on_v1_layout() {
    let conn = v1_db();
    seed_node(&conn, "node_a", 0);
    seed_vector_metadata(&conn, "node_a", "m", 0);

    db::clear_database(&conn).expect("clear");

    let node_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))
        .expect("count nodes");
    assert_eq!(node_count, 0);

    let vector_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM vectors", [], |r| r.get(0))
        .expect("count vectors");
    assert_eq!(vector_count, 0);
}

#[test]
fn clear_database_works_on_v0_layout() {
    // The regression: on a v0 database this used to fail with
    // `no such table: vectors`.
    let conn = v0_db();
    seed_node(&conn, "node_a", 0);
    seed_vector_metadata(&conn, "node_a", "m", 0);

    db::clear_database(&conn).expect("clear");

    let node_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))
        .expect("count nodes");
    assert_eq!(node_count, 0);

    let vector_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM vectors_meta", [], |r| r.get(0))
        .expect("count vectors_meta");
    assert_eq!(vector_count, 0);
}

#[test]
fn clear_database_works_with_no_vector_storage() {
    let conn = Connection::open_in_memory().expect("open in-memory db");
    conn.execute_batch(db::SCHEMA_SQL).expect("apply schema");
    seed_node(&conn, "node_a", 0);

    db::clear_database(&conn).expect("clear");

    let node_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))
        .expect("count nodes");
    assert_eq!(node_count, 0);
}
