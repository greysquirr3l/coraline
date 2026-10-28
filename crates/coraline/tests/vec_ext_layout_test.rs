//! Integration tests for the `vec-ext` (sqlite-vec) storage layout.
//!
//! These tests only compile when the `vec-ext` Cargo feature is
//! enabled, because they need the `vec0` virtual-table module — which
//! only exists in that build.
//!
//! # The bug this guards against
//!
//! The vector storage layout is a property of the **database file**,
//! not of the binary that opens it. The SQL that reached those tables
//! used to be selected with `#[cfg(feature = "vec-ext")]` — a
//! compile-time switch standing in for a runtime property. That broke
//! half of the (build × layout) matrix:
//!
//! | Build   | DB layout | What went wrong                            |
//! | ------- | --------- | ------------------------------------------ |
//! | default | v1        | —                                          |
//! | default | v0        | `coraline_stats`: `no such table: vectors`  |
//! | vec-ext | v1        | `coraline_stats`: `no such table: vectors`  |
//! | vec-ext | v0        | —                                          |
//!
//! See issue #137. The fix is to detect the layout at runtime from
//! the open connection (see `coraline::vector_layout`) and resolve
//! every vector-related statement from that.
//!
//! These tests cover the `vec-ext` half of the matrix, plus the
//! migration paths between the two layouts.

#![allow(clippy::expect_used)]

use coraline::db;
use coraline::tools::Tool;
use coraline::tools::graph_tools::StatsTool;
use coraline::vector_layout::VectorLayout;
use rusqlite::Connection;
use serde_json::json;

/// Open a connection with the sqlite-vec module loaded.
///
/// The module is registered process-wide by `register_global_init`,
/// but that only affects connections opened *after* the call. Loading
/// it directly on the handle is what makes the module available on
/// *this* connection.
fn vec0_connection() -> Connection {
    let conn = Connection::open_in_memory().expect("open in-memory db");
    coraline::vec_ext::runtime::enable_extension(&conn).expect("enable sqlite-vec");
    conn
}

/// A v0-layout database with a *real* `vec0` payload table.
fn v0_db() -> Connection {
    let conn = vec0_connection();
    conn.execute_batch(db::SCHEMA_SQL).expect("apply schema");
    // The v1 `vectors` table is replaced by the v0 pair.
    conn.execute_batch("DROP TABLE IF EXISTS vectors; DROP INDEX IF EXISTS idx_vectors_model;")
        .expect("drop v1 vectors table");
    coraline::vec_ext::ensure_vec0_schema(&conn).expect("ensure vec0 schema");
    conn
}

/// A v1-layout database, migrated onto the v0 layout the way
/// `coraline migrate vec-ext` does it.
fn v1_db() -> Connection {
    let conn = vec0_connection();
    conn.execute_batch(db::SCHEMA_SQL).expect("apply schema");
    conn
}

/// Seed a node row.
fn seed_node(conn: &Connection, id: &str) {
    conn.execute(
        "INSERT INTO nodes (id, kind, name, qualified_name, file_path, language,
            start_line, end_line, start_column, end_column, updated_at)
         VALUES (?1, 'function', ?1, ?1, 'src/lib.rs', 'rust', 1, 1, 0, 0, 0)",
        rusqlite::params![id],
    )
    .expect("insert node");
}

/// Store a real embedding in whichever layout the database uses.
fn seed_embedding(conn: &Connection, node_id: &str, model: &str) {
    let embedding = vec![0.25_f32; 768];
    let relation = VectorLayout::detect(conn)
        .expect("detect")
        .metadata_relation();
    if relation == "vectors_meta" {
        // The v0 layout keeps the payload in the `vec0` table and
        // the metadata in its companion.
        let json = serde_json::to_string(&embedding).expect("serialize embedding");
        conn.execute(
            "INSERT INTO vectors_vec (embedding) VALUES (vec_f32(?1))",
            rusqlite::params![json],
        )
        .expect("insert vec0 payload");
        let rowid = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO vectors_meta (rowid, node_id, model, created_at)
             VALUES (?1, ?2, ?3, 0)",
            rusqlite::params![rowid, node_id, model],
        )
        .expect("insert vectors_meta row");
    } else {
        let bytes: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();
        conn.execute(
            "INSERT INTO vectors (node_id, embedding, model, created_at)
             VALUES (?1, ?2, ?3, 0)",
            rusqlite::params![node_id, bytes, model],
        )
        .expect("insert vector");
    }
}

// ── The v0 module really is available on the connection ────────────────────

#[test]
fn vec0_module_is_loaded_on_the_connection() {
    let conn = vec0_connection();
    assert!(
        coraline::vector_layout::vec0_module_loaded(&conn).expect("probe module list"),
        "the vec0 module should be registered on a connection that has it enabled"
    );
}

// ── ensure_vec0_schema over a seeded database (issue #137) ─────────────────

#[test]
fn ensure_vec0_schema_creates_the_v0_layout() {
    let conn = v1_db();
    assert_eq!(
        VectorLayout::detect(&conn).expect("detect"),
        VectorLayout::V1Blob,
        "precondition: starts on the v1 layout"
    );

    coraline::vec_ext::ensure_vec0_schema(&conn).expect("ensure vec0 schema");

    assert_eq!(
        VectorLayout::detect(&conn).expect("detect"),
        VectorLayout::V0Vec,
        "ensure_vec0_schema must move the database onto the v0 layout"
    );
    assert!(
        coraline::vector_layout::vec0_payload_table_exists(&conn).expect("check payload table"),
        "the vec0 payload table must exist"
    );
}

#[test]
fn ensure_vec0_schema_is_idempotent() {
    let conn = v1_db();
    coraline::vec_ext::ensure_vec0_schema(&conn).expect("first call");
    coraline::vec_ext::ensure_vec0_schema(&conn).expect("second call");
    assert_eq!(
        VectorLayout::detect(&conn).expect("detect"),
        VectorLayout::V0Vec
    );
}

#[test]
fn stats_tool_works_on_a_v0_database() {
    // The regression from issue #137: `coraline_stats` used to run
    // `SELECT COUNT(*) FROM vectors` unconditionally, which fails with
    // `no such table: vectors` on a v0 database.
    let conn = v0_db();
    seed_node(&conn, "node_a");
    seed_embedding(&conn, "node_a", "nomic-embed-text-v1.5");

    let root = tempdir("stats-v0");
    copy_db(&conn, &root);

    let tool = StatsTool::new(root);
    let result = tool
        .execute(json!({}))
        .expect("stats must not fail on a v0 database");

    let vectors = result
        .get("totals")
        .and_then(|totals| totals.get("vectors"))
        .and_then(serde_json::Value::as_u64);
    assert_eq!(
        vectors,
        Some(1),
        "the v0 layout holds one embedding: {result}"
    );
}

// ── The migration paths ────────────────────────────────────────────────────

#[test]
fn migrate_to_vec0_then_back_to_vec1_preserves_the_embedding() {
    // The round trip is the interesting case: it has to survive both
    // the write into the `vec0` payload table and the read back out
    // of it.
    let conn = v1_db();
    seed_node(&conn, "node_a");
    seed_embedding(&conn, "node_a", "nomic-embed-text-v1.5");

    let outcome = coraline::vec_ext::migrate_to_vec0_conn(&conn).expect("migrate to vec0");
    assert!(
        matches!(outcome, coraline::vec_ext::MigrationOutcome::Migrated),
        "expected a migration, got {outcome:?}"
    );
    assert_eq!(
        VectorLayout::detect(&conn).expect("detect"),
        VectorLayout::V0Vec
    );

    let outcome = coraline::vec_ext::migrate_to_v1_conn(&conn).expect("migrate to v1");
    assert!(
        matches!(outcome, coraline::vec_ext::MigrationOutcome::Migrated),
        "expected a migration, got {outcome:?}"
    );
    assert_eq!(
        VectorLayout::detect(&conn).expect("detect"),
        VectorLayout::V1Blob
    );

    // The embedding must have survived the round trip intact.
    let bytes: Vec<u8> = conn
        .query_row(
            "SELECT embedding FROM vectors WHERE node_id = ?1",
            rusqlite::params!["node_a"],
            |row| row.get(0),
        )
        .expect("read migrated embedding");
    let floats: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect();
    assert_eq!(floats.len(), 768, "the embedding must keep its width");
    assert!(
        floats.iter().all(|f| (f - 0.25).abs() < 1e-6),
        "the embedding must keep its values: {floats:?}"
    );
}

#[test]
fn a_failed_migration_leaves_the_source_table_intact() {
    // The migration drops the v1 `vectors` table and then writes the
    // vec0 rows. If the writes fail, the drop must not have been
    // committed — otherwise the project is left with no embeddings at
    // all and needs a full re-embed to recover.
    //
    // The way to make the writes fail is to give the v1 table an
    // embedding whose width does not match the `vec0` schema's
    // `float[768]`: `vec0` rejects a dimension mismatch.
    let conn = v1_db();
    seed_node(&conn, "node_a");

    let wrong_width = vec![0.25_f32; 512];
    let bytes: Vec<u8> = wrong_width.iter().flat_map(|f| f.to_le_bytes()).collect();
    conn.execute(
        "INSERT INTO vectors (node_id, embedding, model, created_at)
         VALUES ('node_a', ?1, 'm', 0)",
        rusqlite::params![bytes],
    )
    .expect("insert vector");

    let outcome = coraline::vec_ext::migrate_to_vec0_conn(&conn);
    assert!(
        outcome.is_err(),
        "a dimension mismatch must fail the migration, not corrupt the project"
    );

    // The v1 table must still be there, with its row intact.
    assert_eq!(
        VectorLayout::detect(&conn).expect("detect"),
        VectorLayout::V1Blob,
        "a failed migration must leave the project on the v1 layout"
    );
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM vectors", [], |r| r.get(0))
        .expect("count vectors");
    assert_eq!(
        count, 1,
        "the original embedding must survive a failed migration"
    );
}

#[test]
fn migrate_to_vec1_reads_a_vec0_payload_as_bytes() {
    // `vec0` returns a `float[]` column as a raw little-endian BLOB,
    // not as the JSON text that was fed to `vec_f32()`. Reading it as
    // text fails with `InvalidColumnType`, which is what the v0 -> v1
    // migration used to do.
    let conn = v0_db();
    seed_embedding(&conn, "node_a", "m");

    let raw: rusqlite::types::Value = conn
        .query_row("SELECT embedding FROM vectors_vec LIMIT 1", [], |row| {
            row.get(0)
        })
        .expect("read vec0 payload");
    assert!(
        matches!(raw, rusqlite::types::Value::Blob(_)),
        "vec0 stores float[] columns as blobs, got {raw:?}"
    );

    let decoded = coraline::vector_layout::decode_vec0_embedding(&raw).expect("decode");
    assert_eq!(decoded.len(), 768);
    let first = decoded.first().copied().unwrap_or_default();
    assert!((first - 0.25).abs() < 1e-6);
}

#[test]
fn decode_vec0_embedding_accepts_text_for_older_databases() {
    let text = rusqlite::types::Value::Text("[0.5,0.25,1.0]".to_string());
    let decoded = coraline::vector_layout::decode_vec0_embedding(&text).expect("decode text");
    assert_eq!(decoded, vec![0.5_f32, 0.25, 1.0]);
}

#[test]
fn decode_vec0_embedding_rejects_an_unsupported_type() {
    let value = rusqlite::types::Value::Integer(7);
    assert!(
        coraline::vector_layout::decode_vec0_embedding(&value).is_err(),
        "an integer is not a valid embedding"
    );
}

#[test]
fn decode_vec0_embedding_survives_malformed_text() {
    // A corrupt or hand-edited database must not be able to panic the
    // process. A `]` before the `[` used to produce an out-of-order
    // slice range.
    for text in ["]abc[", "no brackets at all", "[1,2", "", "[]", "[1,2,abc]"] {
        let value = rusqlite::types::Value::Text(text.to_string());
        let decoded = coraline::vector_layout::decode_vec0_embedding(&value)
            .expect("malformed text must not panic");
        assert!(
            decoded.len() <= 3,
            "a malformed array must not produce more elements than it had: {decoded:?}"
        );
    }
}

// ── clear_database on a real v0 database ───────────────────────────────────

#[test]
fn clear_database_clears_a_real_v0_database() {
    let conn = v0_db();
    seed_node(&conn, "node_a");
    seed_embedding(&conn, "node_a", "m");

    db::clear_database(&conn).expect("clear");

    let node_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))
        .expect("count nodes");
    assert_eq!(node_count, 0);

    let meta_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM vectors_meta", [], |r| r.get(0))
        .expect("count vectors_meta");
    assert_eq!(meta_count, 0);

    let payload_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM vectors_vec", [], |r| r.get(0))
        .expect("count vectors_vec");
    assert_eq!(
        payload_count, 0,
        "the vec0 payload rows must be cleared too, not orphaned"
    );
}

#[test]
fn clear_database_works_on_a_v1_database() {
    let conn = v1_db();
    seed_node(&conn, "node_a");
    seed_embedding(&conn, "node_a", "m");

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

// ── Helpers ────────────────────────────────────────────────────────────────

/// Create a temporary directory that is removed when it goes out of
/// scope.
fn tempdir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "coraline-vec-ext-test-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Copy an in-memory database into `<root>/.coraline/coraline.db`, so
/// that a tool can open it from a project root.
fn copy_db(conn: &Connection, root: &std::path::Path) {
    let coraline_dir = root.join(".coraline");
    std::fs::create_dir_all(&coraline_dir).expect("create .coraline dir");
    let target = coraline_dir.join("coraline.db");
    conn.execute(
        "VACUUM INTO ?1",
        rusqlite::params![target.to_string_lossy()],
    )
    .expect("vacuum into target");
}
