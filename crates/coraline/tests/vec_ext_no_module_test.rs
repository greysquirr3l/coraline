//! What a build *without* the sqlite-vec module finds when it opens a
//! project that a `vec-ext` build created.
//!
//! # Why this is its own test binary
//!
//! The sqlite-vec auto-extension is **process-global**. Once it has
//! been registered, every connection opened afterwards in that process
//! has the `vec0` module — even one opened by a test that never asked
//! for it. So the "the module is not available" state cannot be
//! observed from a test binary that enables the extension somewhere
//! else: the test order is not deterministic, and cargo runs the tests
//! in one binary in parallel threads. Each integration test file is
//! its own binary, and therefore its own process, so this file — which
//! deliberately never registers the auto-extension — is the only place
//! the state can be observed reliably.
//!
//! # The scenario
//!
//! A project is created by a `vec-ext` build, so its embeddings live
//! in a `vectors_vec` `vec0` virtual table. A default build then opens
//! that project. It cannot read, write, or drop the `vec0` payload
//! table, because the module simply is not registered on its
//! connections — but it must not fail on it either.

#![allow(clippy::expect_used)]

use coraline::db;
use rusqlite::Connection;

/// Create a project on the v0 layout, the way a `vec-ext` build would.
///
/// The module is loaded onto this connection *directly*, without
/// registering the process-wide auto-extension, so that the project
/// can be reopened below on a connection that does not have it.
fn create_v0_project(db_path: &std::path::Path) {
    let conn = Connection::open(db_path).expect("open db");
    coraline::vec_ext::runtime::load_module(&conn).expect("load module");

    conn.execute_batch(db::SCHEMA_SQL).expect("apply schema");
    // The v1 `vectors` table is replaced by the v0 pair.
    conn.execute_batch("DROP TABLE IF EXISTS vectors; DROP INDEX IF EXISTS idx_vectors_model;")
        .expect("drop v1 vectors table");
    coraline::vec_ext::ensure_vec0_schema(&conn).expect("ensure vec0 schema");

    conn.execute(
        "INSERT INTO nodes (id, kind, name, qualified_name, file_path, language,
            start_line, end_line, start_column, end_column, updated_at)
         VALUES ('node_a', 'function', 'node_a', 'node_a', 'src/lib.rs', 'rust',
            1, 1, 0, 0, 0)",
        [],
    )
    .expect("insert node");
}

#[test]
fn a_project_without_the_vec0_module_can_still_be_cleared() {
    let dir =
        std::env::temp_dir().join(format!("coraline-vec-ext-no-module-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let db_path = dir.join("coraline.db");

    create_v0_project(&db_path);

    // Reopen it the way a default build would: no module.
    let conn = Connection::open(&db_path).expect("reopen db");
    assert!(
        !coraline::vector_layout::vec0_module_loaded(&conn).expect("probe module list"),
        "precondition: this connection must not have the vec0 module"
    );
    assert!(
        coraline::vector_layout::vec0_payload_table_exists(&conn).expect("check payload table"),
        "precondition: the payload table is on disk"
    );

    // The clear must succeed rather than fail with
    // `no such module: vec0` or `no such table: vectors_vec`.
    db::clear_database(&conn).expect("clear must not fail without the vec0 module");

    let node_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))
        .expect("count nodes");
    assert_eq!(node_count, 0, "the graph tables must still be cleared");

    let _ = std::fs::remove_dir_all(&dir);
}
