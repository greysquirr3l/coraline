//! Fuzz target: `SQLite` query layer (`FTS5` search).
//!
//! Drives `db::search_nodes` with arbitrary strings against an in-memory
//! `SQLite` database initialised with the production schema. This exercises:
//!
//! - `build_fts_query` (whitespace splitting, quote escaping, string growth)
//! - `FTS5` MATCH parsing (parameterised, so SQL injection is not the risk;
//!   the risk is panics, unbounded allocation, or surprising behaviour from
//!   the `FTS5` query engine on adversarial input)
//! - Parameter binding and result iteration for the dynamic `SQL` builder
//!   inside `search_nodes` (the `LIMIT ?` and `kind = ?` paths)
//!
//! An empty / non-`UTF-8` input is treated as a no-op; we are interested in
//! strings the application would actually receive from an MCP client.

#![no_main]
// Fuzz harnesses are not production code: the once-per-process init of
// the in-memory schema database cannot meaningfully fail (in-memory open,
// no I/O, constant schema string). Allowing `expect_used` here keeps the
// harness readable; the alternative is a chain of `match` that hides the
// intent. `significant_drop_tightening` is informational only — the
// single-threaded libFuzzer harness has no real contention on the
// database mutex, and shortening the lock scope would force a more
// awkward `unsafe` lifetime transmute.
#![allow(clippy::expect_used, clippy::significant_drop_tightening)]

use std::sync::Mutex;

use libfuzzer_sys::fuzz_target;
use rusqlite::Connection;

static SCHEMA: &str = coraline::db::SCHEMA_SQL;

/// Lazily initialised, schema-loaded, in-memory `SQLite` connection.
///
/// `rusqlite::Connection` is `!Sync` (it carries an interior `RefCell`
/// statement cache), so we wrap it in a `Mutex` for the shared static.
/// libFuzzer's stock harness is single-threaded, so contention is not a
/// concern in practice.
static DB: Mutex<Option<Connection>> = Mutex::new(None);

fn db() -> std::sync::MutexGuard<'static, Option<Connection>> {
    let mut guard = DB.lock().expect("db mutex poisoned");
    if guard.is_none() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(SCHEMA)
            .expect("apply schema; this is a startup invariant");
        *guard = Some(conn);
    }
    guard
}

fuzz_target!(|data: &[u8]| {
    let Some(query) = std::str::from_utf8(data).ok() else {
        return;
    };

    let guard = db();
    let Some(conn) = guard.as_ref() else {
        unreachable!("db() initialised the connection");
    };

    // Primary path: unfiltered FTS search with a generous limit. The query
    // string is exactly what an MCP client controls.
    let _ = coraline::db::search_nodes(conn, query, None, 100);

    // Secondary path: same query but with a `kind` filter, exercising the
    // branch in `search_nodes` that appends `AND n.kind = ?` and binds the
    // serialised `NodeKind`. The fuzz corpus naturally exercises both
    // branches over time.
    let _ = coraline::db::search_nodes(conn, query, Some(coraline::types::NodeKind::Function), 100);

    // Tertiary path: exact-name lookup. Parameterised by `?`, so this
    // primarily exercises string handling and row decoding for arbitrary
    // names. Catches panics in `row_to_node` / `parse_language` paths.
    let _ = coraline::db::find_nodes_by_name(conn, query);

    // Quaternary path: module export lookup. Also parameterised; the
    // interesting surface is `parse_language` and string trimming in the
    // result decoder.
    let _ = coraline::db::find_exports_by_module(conn, query);
});
