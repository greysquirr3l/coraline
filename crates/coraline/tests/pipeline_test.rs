//! Integration tests for the unresolved-reference pipeline.
//!
//! Covers the three behaviours the pipeline depends on:
//!
//! 1. keyset pagination of `unresolved_refs` — every pending reference is
//!    visited exactly once, no matter how many pages it takes;
//! 2. the `attempted` bookkeeping column — references that were tried and
//!    failed are not retried forever;
//! 3. cross-file edge preservation — deleting a file re-queues the edges
//!    that pointed into it from other files instead of losing them.
#![allow(clippy::expect_used)]

use coraline::db;
use coraline::types::EdgeKind;
use coraline::types::UnresolvedReference;
use rusqlite::Connection;
use rusqlite::params;

/// Open an in-memory database with the current schema applied and the
/// incremental migrations run — the same thing `open_database` does for a
/// project database.
fn test_connection() -> Connection {
    let conn = Connection::open_in_memory().expect("open in-memory database");
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .expect("apply pragmas");
    conn.execute_batch(db::SCHEMA_SQL).expect("apply schema");
    db::apply_incremental_migrations(&conn).expect("apply migrations");
    conn
}

fn insert_node(conn: &Connection, id: &str, name: &str, file_path: &str) {
    conn.execute(
        "INSERT INTO nodes (id, kind, name, qualified_name, file_path, language,
            start_line, end_line, start_column, end_column, updated_at)
         VALUES (?1, 'function', ?2, ?2, ?3, 'rust', 1, 1, 0, 0, 0)",
        params![id, name, file_path],
    )
    .expect("insert node");
}

fn insert_unresolved_ref(conn: &Connection, from_node_id: &str, name: &str) -> i64 {
    conn.execute(
        "INSERT INTO unresolved_refs
            (from_node_id, reference_name, reference_kind, line, col, candidates)
         VALUES (?1, ?2, 'calls', 10, 4, NULL)",
        params![from_node_id, name],
    )
    .expect("insert unresolved ref");
    conn.last_insert_rowid()
}

fn insert_edge(conn: &Connection, source: &str, target: &str, kind: &str, line: i64, col: i64) {
    conn.execute(
        "INSERT INTO edges (source, target, kind, metadata, line, col, confidence)
         VALUES (?1, ?2, ?3, NULL, ?4, ?5, 1.0)",
        params![source, target, kind, line, col],
    )
    .expect("insert edge");
}

fn column_present(conn: &Connection, table: &str, column: &str) -> bool {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("prepare table_info");
    let mut rows = stmt.query([]).expect("query table_info");
    while let Some(row) = rows.next().expect("next row") {
        let name: String = row.get(1).expect("read column name");
        if name == column {
            return true;
        }
    }
    false
}

fn count_rows(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |row| row.get(0))
        .expect("count rows")
}

fn write_file(root: &std::path::Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write file");
}

/// Index `root` and return the opened database.
fn index_project(root: &std::path::Path) -> Connection {
    db::initialize_database(root).expect("initialize database");
    let cfg = coraline::config::create_default_config(root);
    coraline::extraction::index_all(root, &cfg, false, None).expect("index project");
    db::open_database(root).expect("open database")
}

#[test]
fn cross_file_call_edge_survives_a_reindex() {
    let temp = tempfile::TempDir::new().expect("create temp dir");
    let root = temp.path();

    write_file(
        root,
        "src/lib.rs",
        "pub mod helper;\npub fn caller() -> i32 { helper::target() }\n",
    );
    write_file(root, "src/helper.rs", "pub fn target() -> i32 { 42 }\n");

    let cfg = coraline::config::create_default_config(root);
    let mut conn = index_project(root);

    // The cross-file call is resolved into an edge.
    let calls = count_rows(
        &conn,
        "SELECT COUNT(*) FROM edges
         WHERE kind = 'calls'
           AND source IN (SELECT id FROM nodes WHERE file_path = 'src/lib.rs')
           AND target IN (SELECT id FROM nodes WHERE file_path = 'src/helper.rs')",
    );
    assert_eq!(
        calls, 1,
        "the cross-file call must be resolved into an edge"
    );

    // Re-index the file that owns the callee: the edge must not be lost.
    db::delete_file(&mut conn, "src/helper.rs").expect("delete file");

    let calls = count_rows(
        &conn,
        "SELECT COUNT(*) FROM edges
         WHERE kind = 'calls'
           AND source IN (SELECT id FROM nodes WHERE file_path = 'src/lib.rs')
           AND target IN (SELECT id FROM nodes WHERE file_path = 'src/helper.rs')",
    );
    assert_eq!(calls, 0, "the cascaded edge is gone from the graph");

    let requeued = count_rows(
        &conn,
        "SELECT COUNT(*) FROM unresolved_refs WHERE reference_name = 'target'",
    );
    assert_eq!(requeued, 1, "the cross-file edge must be re-queued");

    // Re-index the deleted file and let the resolver rebuild the edge.
    coraline::extraction::index_all(root, &cfg, false, None).expect("re-index project");

    let calls = count_rows(
        &conn,
        "SELECT COUNT(*) FROM edges
         WHERE kind = 'calls'
           AND source IN (SELECT id FROM nodes WHERE file_path = 'src/lib.rs')
           AND target IN (SELECT id FROM nodes WHERE file_path = 'src/helper.rs')",
    );
    assert_eq!(calls, 1, "the edge is rebuilt after the re-index");
}

#[test]
fn keyset_pagination_returns_every_unresolved_ref_across_pages() {
    let conn = test_connection();
    for i in 0..25 {
        let node_id = format!("node_{i}");
        insert_node(&conn, &node_id, &format!("fn_{i}"), "src/lib.rs");
        insert_unresolved_ref(&conn, &node_id, &format!("target_{i}"));
    }

    // Page through with the cursor primitive.
    let mut seen: Vec<i64> = Vec::new();
    let mut cursor = 0i64;
    loop {
        let page = db::list_unresolved_refs_after(&conn, cursor, 10).expect("list page");
        let Some(last) = page.last() else {
            break;
        };
        cursor = last.id;
        seen.extend(page.iter().map(|row| row.id));
    }

    assert_eq!(
        seen.len(),
        25,
        "every unresolved reference must be visited exactly once"
    );

    let mut unique = seen.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), 25, "no duplicate ids across pages");

    for pair in seen.windows(2) {
        let (Some(first), Some(second)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        assert!(first < second, "ids must be strictly increasing");
    }

    // The draining helper returns exactly the same set in a single call.
    let all = db::list_all_unresolved_refs_paged(&conn, 7).expect("page all");
    let all_ids: Vec<i64> = all.iter().map(|row| row.id).collect();
    assert_eq!(all_ids, seen);

    assert_eq!(db::count_unresolved_refs(&conn).expect("count"), 25);
    assert_eq!(
        db::count_pending_unresolved_refs(&conn).expect("count pending"),
        25
    );
}

#[test]
fn a_full_walk_covers_the_whole_backlog_regardless_of_page_size() {
    let conn = test_connection();
    for i in 0..25 {
        let node_id = format!("node_{i}");
        insert_node(&conn, &node_id, &format!("fn_{i}"), "src/lib.rs");
        insert_unresolved_ref(&conn, &node_id, &format!("target_{i}"));
    }

    // A page size of 10 must not cap the walk at 10 rows: every reference
    // has to be handed out, otherwise the references beyond the first page
    // are starved forever.
    let all = db::list_all_unresolved_refs_paged(&conn, 10).expect("walk");

    assert_eq!(all.len(), 25, "the walk must cover the whole backlog");

    let mut ids: Vec<i64> = all.iter().map(|row| row.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 25, "no reference is handed out twice");
}

#[test]
fn a_failed_reference_is_retried_after_the_project_changes() {
    let conn = test_connection();
    let node_id = "node_0";
    insert_node(&conn, node_id, "fn_0", "src/lib.rs");
    insert_unresolved_ref(&conn, node_id, "target_0");

    // A resolution pass that cannot find the target flags the reference as
    // attempted, so it is not retried on every run.
    db::mark_pending_unresolved_refs_attempted(&conn).expect("flag");
    assert_eq!(
        db::count_pending_unresolved_refs(&conn).expect("count pending"),
        0
    );
    assert!(
        db::list_all_unresolved_refs_paged(&conn, 10)
            .expect("walk")
            .is_empty(),
        "an attempted reference is not handed out again"
    );

    // The project changed: the reference goes back in the queue, otherwise
    // the edge it should have produced would never appear.
    db::reset_unresolved_refs_attempted(&conn).expect("re-queue");
    assert_eq!(
        db::count_pending_unresolved_refs(&conn).expect("count pending"),
        1
    );
    assert_eq!(
        db::list_all_unresolved_refs_paged(&conn, 10)
            .expect("walk")
            .len(),
        1
    );
}

#[test]
fn attempted_migration_defaults_to_zero_and_excludes_marked_refs() {
    let mut conn = test_connection();

    insert_node(&conn, "node_a", "fn_a", "src/a.rs");
    let legacy_id = insert_unresolved_ref(&conn, "node_a", "target_a");

    // Simulate a database that was created before the `attempted` column
    // existed.
    conn.execute_batch(
        "DROP INDEX IF EXISTS idx_unresolved_pending;
         ALTER TABLE unresolved_refs DROP COLUMN attempted;",
    )
    .expect("revert to the pre-migration schema");
    assert!(
        !column_present(&conn, "unresolved_refs", "attempted"),
        "the column must be gone before the migration runs"
    );

    // Re-run the migrations, exactly like `open_database` does.
    db::apply_incremental_migrations(&conn).expect("apply migrations");

    assert!(
        column_present(&conn, "unresolved_refs", "attempted"),
        "the migration must add the column"
    );

    let attempted: i64 = conn
        .query_row(
            "SELECT attempted FROM unresolved_refs WHERE id = ?1",
            params![legacy_id],
            |row| row.get(0),
        )
        .expect("read attempted flag");
    assert_eq!(attempted, 0, "pre-existing rows default to attempted = 0");

    let schema_version_count = count_rows(
        &conn,
        "SELECT COUNT(*) FROM schema_versions WHERE version = 4",
    );
    assert_eq!(schema_version_count, 1, "schema version 4 must be recorded");

    // A new row also defaults to 0.
    let fresh_id = insert_unresolved_ref(&conn, "node_a", "target_b");
    let attempted: i64 = conn
        .query_row(
            "SELECT attempted FROM unresolved_refs WHERE id = ?1",
            params![fresh_id],
            |row| row.get(0),
        )
        .expect("read attempted flag");
    assert_eq!(attempted, 0, "new rows default to attempted = 0");

    // Marking a reference flips it to 1 and excludes it from the listing.
    let marked = db::mark_unresolved_refs_attempted(&mut conn, &[legacy_id]).expect("mark refs");
    assert_eq!(marked, 1, "exactly one row is flagged as attempted");

    let attempted: i64 = conn
        .query_row(
            "SELECT attempted FROM unresolved_refs WHERE id = ?1",
            params![legacy_id],
            |row| row.get(0),
        )
        .expect("read attempted flag");
    assert_eq!(attempted, 1);

    let listed = db::list_all_unresolved_refs_paged(&conn, 10).expect("list refs");
    let listed_ids: Vec<i64> = listed.iter().map(|row| row.id).collect();
    assert_eq!(
        listed_ids,
        vec![fresh_id],
        "attempted references are excluded from the default listing"
    );

    // Marking the same row twice is a no-op.
    let marked_again =
        db::mark_unresolved_refs_attempted(&mut conn, &[legacy_id]).expect("mark refs again");
    assert_eq!(marked_again, 0);
}

#[test]
fn delete_file_requeues_cross_file_incoming_edges_only() {
    let mut conn = test_connection();

    insert_node(&conn, "a_caller", "caller", "src/a.rs");
    insert_node(&conn, "b_callee", "callee", "src/b.rs");
    insert_node(&conn, "b_sibling", "sibling", "src/b.rs");

    // Cross-file edge: the caller in a.rs calls the callee in b.rs.
    insert_edge(&conn, "a_caller", "b_callee", "calls", 10, 4);
    // Intra-file edge: both ends live in b.rs.
    insert_edge(&conn, "b_sibling", "b_callee", "calls", 20, 8);
    // Outgoing edge: b.rs calls back into a.rs.
    insert_edge(&conn, "b_callee", "a_caller", "calls", 30, 2);

    db::delete_file(&mut conn, "src/b.rs").expect("delete file");

    // The nodes of the deleted file are gone.
    assert_eq!(
        count_rows(
            &conn,
            "SELECT COUNT(*) FROM nodes WHERE file_path = 'src/b.rs'"
        ),
        0
    );
    // ... and so are the edges that cascaded with them.
    assert_eq!(
        count_rows(
            &conn,
            "SELECT COUNT(*) FROM edges WHERE target = 'b_callee'"
        ),
        0
    );

    // The cross-file edge came back as an unresolved reference; the
    // intra-file edge and the outgoing edge did not.
    let refs = db::list_all_unresolved_refs_paged(&conn, 100).expect("list refs");
    assert_eq!(refs.len(), 1, "only the cross-file edge is re-queued");

    let requeued = refs.first().expect("one re-queued reference");
    assert_eq!(requeued.reference.from_node_id, "a_caller");
    assert_eq!(requeued.reference.reference_name, "callee");
    assert_eq!(requeued.reference.reference_kind, EdgeKind::Calls);
    assert_eq!(requeued.reference.line, 10);
    assert_eq!(requeued.reference.column, 4);

    // The re-queued reference points at a node that still exists, so it
    // survives the cascade and is ready to be resolved again.
    assert_eq!(
        count_rows(&conn, "SELECT COUNT(*) FROM nodes WHERE id = 'a_caller'"),
        1
    );
    assert_eq!(
        count_rows(
            &conn,
            "SELECT COUNT(*) FROM unresolved_refs WHERE from_node_id = 'b_sibling'"
        ),
        0,
        "intra-file edges are not re-queued"
    );
}

#[test]
fn delete_file_is_idempotent_about_requeued_references() {
    let mut conn = test_connection();

    insert_node(&conn, "a_caller", "caller", "src/a.rs");
    insert_node(&conn, "b_callee", "callee", "src/b.rs");
    insert_edge(&conn, "a_caller", "b_callee", "calls", 10, 4);

    db::delete_file(&mut conn, "src/b.rs").expect("delete file");
    // Re-insert the file and delete it again: the reference must not pile up.
    insert_node(&conn, "b_callee", "callee", "src/b.rs");
    insert_edge(&conn, "a_caller", "b_callee", "calls", 10, 4);
    db::delete_file(&mut conn, "src/b.rs").expect("delete file again");

    assert_eq!(
        count_rows(&conn, "SELECT COUNT(*) FROM unresolved_refs"),
        1,
        "the same cross-file edge must not be re-queued twice"
    );
}

fn reference(from_node_id: &str, name: &str, qualifier: Option<&str>) -> UnresolvedReference {
    UnresolvedReference {
        from_node_id: from_node_id.to_string(),
        reference_name: name.to_string(),
        reference_kind: EdgeKind::Calls,
        line: 12,
        column: 8,
        candidates: None,
        qualifier: qualifier.map(std::string::ToString::to_string),
    }
}

#[test]
fn qualifier_round_trips_through_the_unresolved_ref_table() {
    let mut conn = test_connection();
    insert_node(&conn, "node_a", "fn_a", "src/a.rs");

    let refs = [
        reference("node_a", "doThing", Some("Helper")),
        reference("node_a", "plainCall", None),
    ];
    db::insert_unresolved_refs(&mut conn, &refs).expect("insert unresolved refs");

    let listed = db::list_all_unresolved_refs_paged(&conn, 10).expect("list refs");
    assert_eq!(listed.len(), 2);

    let qualified = listed
        .iter()
        .find(|row| row.reference.reference_name == "doThing")
        .expect("qualified reference");
    assert_eq!(qualified.reference.qualifier.as_deref(), Some("Helper"));

    let plain = listed
        .iter()
        .find(|row| row.reference.reference_name == "plainCall")
        .expect("unqualified reference");
    assert_eq!(plain.reference.qualifier, None);
}

#[test]
fn qualifier_migration_adds_a_nullable_column() {
    let conn = test_connection();

    insert_node(&conn, "node_a", "fn_a", "src/a.rs");
    let legacy_id = insert_unresolved_ref(&conn, "node_a", "target_a");

    // Simulate a database that was created before the `qualifier` column
    // existed.
    conn.execute_batch("ALTER TABLE unresolved_refs DROP COLUMN qualifier;")
        .expect("revert to the pre-migration schema");
    assert!(
        !column_present(&conn, "unresolved_refs", "qualifier"),
        "the column must be gone before the migration runs"
    );

    db::apply_incremental_migrations(&conn).expect("apply migrations");

    assert!(
        column_present(&conn, "unresolved_refs", "qualifier"),
        "the migration must add the column"
    );

    let qualifier: Option<String> = conn
        .query_row(
            "SELECT qualifier FROM unresolved_refs WHERE id = ?1",
            params![legacy_id],
            |row| row.get(0),
        )
        .expect("read qualifier");
    assert_eq!(qualifier, None, "pre-existing rows migrate to NULL");

    let schema_version_count = count_rows(
        &conn,
        "SELECT COUNT(*) FROM schema_versions WHERE version = 5",
    );
    assert_eq!(schema_version_count, 1, "schema version 5 must be recorded");
}

#[test]
fn requeued_references_have_no_qualifier() {
    let mut conn = test_connection();

    insert_node(&conn, "a_caller", "caller", "src/a.rs");
    insert_node(&conn, "b_callee", "callee", "src/b.rs");
    insert_edge(&conn, "a_caller", "b_callee", "calls", 10, 4);

    db::delete_file(&mut conn, "src/b.rs").expect("delete file");

    let refs = db::list_all_unresolved_refs_paged(&conn, 100).expect("list refs");
    let requeued = refs.first().expect("one re-queued reference");
    assert_eq!(requeued.reference.qualifier, None);
    // The NOT NULL columns of `unresolved_refs` are all populated.
    assert_ne!(requeued.reference.from_node_id, "");
    assert_ne!(requeued.reference.reference_name, "");
    assert_eq!(
        count_rows(
            &conn,
            "SELECT COUNT(*) FROM unresolved_refs
             WHERE from_node_id IS NULL OR reference_name IS NULL
                OR reference_kind IS NULL OR line IS NULL OR col IS NULL"
        ),
        0
    );
}

/// A reference that failed to resolve must be retried once the project
/// changes.
///
/// The reference is created in a file that is *not* re-extracted, so the
/// only thing that can turn it into an edge is the resolver taking another
/// look at it. If failed references are never retried, the edge silently
/// never appears.
#[test]
fn a_reference_resolves_after_the_missing_declaration_is_added() {
    let temp = tempfile::TempDir::new().expect("create temp dir");
    let root = temp.path();

    write_file(
        root,
        "src/caller.rs",
        "pub fn caller() -> i32 { later() }\n",
    );

    let mut conn = index_project(root);

    let calls = count_rows(&conn, "SELECT COUNT(*) FROM edges WHERE kind = 'calls'");
    assert_eq!(calls, 0, "nothing can resolve yet");

    let pending = db::count_unresolved_refs(&conn).expect("count unresolved");
    assert_eq!(pending, 1, "the call is parked as an unresolved reference");

    // The declaration arrives in a *different* file, so the caller's own
    // file is never re-extracted.
    write_file(root, "src/later.rs", "pub fn later() -> i32 { 7 }\n");
    let cfg = coraline::config::create_default_config(root);
    coraline::extraction::sync(root, &cfg, None).expect("sync project");
    conn = db::open_database(root).expect("reopen database");

    let calls = count_rows(
        &conn,
        "SELECT COUNT(*) FROM edges
         WHERE kind = 'calls'
           AND source IN (SELECT id FROM nodes WHERE file_path = 'src/caller.rs')
           AND target IN (SELECT id FROM nodes WHERE file_path = 'src/later.rs')",
    );
    assert_eq!(
        calls, 1,
        "the reference must be retried once the declaration exists"
    );
}
