//! Tests for when the sqlite-vec `vec0` module becomes available on a
//! connection.
//!
//! # Why this is its own test binary
//!
//! The sqlite-vec auto-extension is **process-global**. Once it has
//! been registered, every connection opened afterwards in that process
//! has the `vec0` module — even one opened by a test that never asked
//! for it.
//!
//! That makes the "module not loaded yet" case impossible to observe
//! from a test binary that also enables the extension somewhere else:
//! the test order is not deterministic, and cargo runs the tests in
//! one binary in parallel threads. Each integration test file is its
//! own binary, and therefore its own process, so this file — which
//! deliberately contains a *single* test — is the only place the
//! pre-registration state can be observed reliably.
//!
//! # The behaviour under test
//!
//! `sqlite3_auto_extension` only fires when a connection is *opened*.
//! Registering it therefore does nothing for a connection that already
//! exists. `enable_extension` has to load the module onto the
//! connection it was handed, or every caller that passes an
//! already-open connection gets `no such module: vec0`.

#![allow(clippy::expect_used)]

use rusqlite::Connection;

#[test]
fn enable_extension_loads_vec0_onto_an_already_open_connection() {
    // Nothing in this process has registered the auto-extension yet,
    // so the connection starts without the module.
    let conn = Connection::open_in_memory().expect("open in-memory db");
    assert!(
        !coraline::vector_layout::vec0_module_loaded(&conn).expect("probe module list"),
        "precondition: a bare connection must not have the vec0 module"
    );

    coraline::vec_ext::runtime::enable_extension(&conn).expect("enable sqlite-vec");

    assert!(
        coraline::vector_layout::vec0_module_loaded(&conn).expect("probe module list"),
        "enable_extension must load vec0 onto the connection it was given"
    );

    // And the module is actually usable, not merely listed.
    let version: String = conn
        .query_row("SELECT vec_version()", [], |row| row.get(0))
        .expect("query vec_version");
    assert!(version.starts_with('v'), "got version: {version}");
}
