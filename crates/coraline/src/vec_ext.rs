#![deny(unsafe_code)]

//! `sqlite-vec` feature scaffolding (Phase 5.3).
//!
//! Exposes:
//!
//! - A doctor probe (`check_vec_ext`) that reports whether the
//!   `vec-ext` Cargo feature is compiled in and whether the on-disk
//!   vector storage is the v1 in-house BLOB form or the v0 `vec0`
//!   form.
//! - A migration runner (`migrate_to_vec0`) that swaps the v1 BLOB
//!   `vectors` table for a `vec0` virtual table. Gated behind the
//!   `vec-ext` Cargo feature so the default build pays no cost.
//!
//! # Relationship to [`crate::vector_layout`]
//!
//! The *detection* of the on-disk layout lives in
//! [`crate::vector_layout::VectorLayout`] — it is needed by every
//! build, so it cannot live behind a Cargo feature. This module owns
//! the parts that are genuinely `vec-ext`-only: the sqlite-vec FFI
//! registration, the `vec0` runtime helpers, and the v0 migration.
//!
//! The layout is a property of the **database file**, not of the
//! binary that opens it, so the SQL used to reach the vector tables
//! is chosen from the detected layout at runtime — never from a
//! `#[cfg(feature = "vec-ext")]` switch.

use std::path::Path;

#[cfg(feature = "vec-ext")]
use rusqlite::Connection;
use serde::Serialize;

use crate::doctor;
use crate::vector_layout::VectorLayout;

#[cfg(feature = "vec-ext")]
use std::io;

#[cfg(feature = "vec-ext")]
use rusqlite::params;

/// `true` when the binary was compiled with the `vec-ext` feature.
pub const VEC_EXT_ENABLED: bool = cfg!(feature = "vec-ext");

/// Status returned by [`check_vec_ext`] — serialized into the doctor
/// report as a JSON object under the `vec_ext` probe's `detail` field.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct VecExtStatus {
    /// Whether the binary was compiled with the `vec-ext` Cargo feature.
    pub feature_enabled: bool,
    /// State of the on-disk vector storage.
    pub table_state: VectorLayout,
    /// One-line remediation hint, or `None` when no action is needed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

/// One-line remediation hint for the current `(feature, layout)` pair.
///
/// The hint is advisory: it tells the user how to get the *best*
/// configuration, not that something is broken. A v1 project works
/// fine on the default build — it just does not get sqlite-vec's KNN
/// acceleration.
fn remediation_hint(feature: bool, layout: VectorLayout) -> Option<String> {
    match (feature, layout) {
        (false, VectorLayout::V0Vec) => Some(
            "Rebuild with `--features vec-ext` to use this project's sqlite-vec (v0) embeddings."
                .to_string(),
        ),
        (false, VectorLayout::V1Blob) => Some(
            "Rebuild with `--features vec-ext` to upgrade the `vectors` table to sqlite-vec v0."
                .to_string(),
        ),
        (false, VectorLayout::Absent) => Some(
            "Rebuild with `--features vec-ext` to enable sqlite-vec KNN search.".to_string(),
        ),
        (true, VectorLayout::V1Blob) => Some(
            "Run `coraline migrate vec-ext` to convert the BLOB table to vec0.".to_string(),
        ),
        (true, VectorLayout::Absent) => Some(
            "Run `coraline index` first so the `vectors` table exists, then `coraline migrate vec-ext`."
                .to_string(),
        ),
        (true, VectorLayout::V0Vec) => None,
    }
}

/// Build a [`VecExtStatus`] for the project at `project_root`.
///
/// Falls back to `feature_enabled = cfg!(feature = "vec-ext")` when the
/// DB can't be opened (e.g. the project hasn't been `coraline init`'d).
pub fn status(project_root: &Path) -> VecExtStatus {
    let table_state = db::open_database(project_root).map_or(VectorLayout::Absent, |conn| {
        VectorLayout::detect(&conn).unwrap_or(VectorLayout::Absent)
    });
    let fix = remediation_hint(VEC_EXT_ENABLED, table_state);
    VecExtStatus {
        feature_enabled: VEC_EXT_ENABLED,
        table_state,
        fix,
    }
}

/// Doctor probe: `vec-ext` feature state + on-disk vector storage state.
///
/// The probe passes when the on-disk layout is **usable by this
/// build** — that is, when this binary can read and write the
/// project's embeddings:
///
/// - v1 BLOB layout → usable by every build.
/// - v0 `vec0` layout → usable only by a `vec-ext` build (the `vec0`
///   virtual-table module is not registered otherwise).
/// - no vector storage yet → nothing to be incompatible with.
///
/// The probe deliberately does *not* fail just because the `vec-ext`
/// feature is off: that is the normal configuration for the
/// published binaries, and failing it made `coraline doctor` exit
/// non-zero on every default installation.
pub fn check_vec_ext(project_root: &Path) -> doctor::Probe {
    let status = status(project_root);
    let detail =
        serde_json::to_string(&status).unwrap_or_else(|_| status.feature_enabled.to_string());
    let ok = match status.table_state {
        VectorLayout::V0Vec => status.feature_enabled,
        VectorLayout::V1Blob | VectorLayout::Absent => true,
    };
    doctor::Probe {
        name: "vec_ext",
        ok,
        detail,
        fix: status.fix,
    }
}

// Re-export `db::io_other` so the helpers in this module don't need a
// deep `crate::db` path.
use crate::db;

/// What a layout migration did (or did not need to do).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationOutcome {
    /// The project was already on the target layout; nothing changed.
    AlreadyOnTarget,
    /// The project was migrated to the target layout.
    Migrated,
}

impl MigrationOutcome {
    /// `true` when the project was actually changed.
    pub const fn migrated(self) -> bool {
        matches!(self, Self::Migrated)
    }
}

/// Convert the on-disk v1 BLOB `vectors` table into a `vec0` virtual
/// table.
///
/// Requires the `vec-ext` Cargo feature — the `vec0` virtual-table
/// module is not registered otherwise, so the conversion cannot be
/// performed.
#[cfg(not(feature = "vec-ext"))]
pub fn migrate_to_vec0(_project_root: &Path) -> std::io::Result<MigrationOutcome> {
    Err(vec_ext_feature_required())
}

/// Convert the on-disk v0 `vec0` layout back into the v1 BLOB
/// `vectors` table.
///
/// Requires the `vec-ext` Cargo feature — reading the `vec0` payload
/// rows needs the sqlite-vec module.
#[cfg(not(feature = "vec-ext"))]
pub fn migrate_to_v1(_project_root: &Path) -> std::io::Result<MigrationOutcome> {
    Err(vec_ext_feature_required())
}

/// Error returned when a layout migration is attempted by a binary that
/// was built without the `vec-ext` feature.
#[cfg(not(feature = "vec-ext"))]
fn vec_ext_feature_required() -> std::io::Error {
    std::io::Error::other(
        "this binary was built without the `vec-ext` feature, so it cannot convert \
         embedding storage. Rebuild with `--features vec-ext` and retry.",
    )
}

#[cfg(feature = "vec-ext")]
/// Convert the on-disk v1 BLOB `vectors` table into a `vec0` virtual
/// table. Preserves existing embeddings.
///
/// `vec0` virtual tables can't have `TEXT PRIMARY KEY` columns — they
/// always key on the integer `rowid`. So the migration drops the v1
/// `vectors` table, creates a `vec0` table for the vectors themselves
/// (`vectors_vec`), and a sibling metadata table (`vectors_meta`)
/// that maps `rowid -> (node_id, model, created_at)`. Embeddings are
/// decoded from the v1 BLOB and re-inserted via sqlite-vec's
/// `vec_f32(JSON(...))` helper.
///
/// The whole thing runs in a single transaction so the v1 table is
/// only dropped after the new tables are ready.
///
/// This is an **explicit, user-initiated** operation — running it
/// changes the project's on-disk layout, so it is never a side effect
/// of an embedding command.
#[cfg(feature = "vec-ext")]
pub fn migrate_to_vec0(project_root: &Path) -> std::io::Result<MigrationOutcome> {
    let conn = db::open_database(project_root)?;
    migrate_to_vec0_conn(&conn)
}

/// Convert the v1 BLOB `vectors` table into the v0 `vec0` layout on
/// an open connection.
#[cfg(feature = "vec-ext")]
pub fn migrate_to_vec0_conn(conn: &Connection) -> std::io::Result<MigrationOutcome> {
    let layout = VectorLayout::detect(conn)?;
    match layout {
        VectorLayout::V0Vec => return Ok(MigrationOutcome::AlreadyOnTarget),
        VectorLayout::Absent => {
            // Nothing to convert, but create the target layout so the
            // project is ready for sqlite-vec storage.
            ensure_vec0_schema(conn)?;
            return Ok(MigrationOutcome::Migrated);
        }
        VectorLayout::V1Blob => {}
    }

    // Read every v1 row into memory first, so the source table can be
    // dropped before the vec0 rows are written.
    let mut rows_to_migrate: Vec<(String, Vec<f32>, String, i64)> = Vec::new();
    {
        let mut stmt = conn
            .prepare("SELECT node_id, embedding, model, created_at FROM vectors")
            .map_err(super::db::io_other)?;
        let mut rows = stmt.query([]).map_err(super::db::io_other)?;
        while let Some(row) = rows.next().map_err(super::db::io_other)? {
            let node_id: String = row.get(0).map_err(super::db::io_other)?;
            let bytes: Vec<u8> = row.get(1).map_err(super::db::io_other)?;
            let model: String = row.get(2).map_err(super::db::io_other)?;
            let created_at: i64 = row.get(3).map_err(super::db::io_other)?;
            rows_to_migrate.push((node_id, decode_le_f32_vec(&bytes), model, created_at));
        }
    }

    // The conversion runs in a single transaction, so the v1 table is
    // only dropped once every row has been written to the v0 tables.
    //
    // Without it, a failure part-way through the inserts would leave
    // the project with the v1 table already dropped and only some of
    // the embeddings written — every original embedding lost, and the
    // project needing a full re-embed to recover. The inserts *can*
    // fail: `vec0` enforces the declared width, and the schema
    // declares `float[768]`.
    let tx = conn.unchecked_transaction().map_err(super::db::io_other)?;

    ensure_vec0_schema(&tx)?;

    tx.execute_batch("DROP INDEX IF EXISTS idx_vectors_model; DROP TABLE IF EXISTS vectors;")
        .map_err(super::db::io_other)?;

    for (node_id, floats, model, created_at) in &rows_to_migrate {
        let json = floats_to_json(floats);
        tx.execute(
            "INSERT INTO vectors_vec (embedding) VALUES (vec_f32(?1))",
            params![json],
        )
        .map_err(super::db::io_other)?;
        let rowid = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO vectors_meta (rowid, node_id, model, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![rowid, node_id, model, created_at],
        )
        .map_err(super::db::io_other)?;
    }

    tx.commit().map_err(super::db::io_other)?;

    Ok(MigrationOutcome::Migrated)
}

/// Idempotently ensure the vec0 embedding tables exist.
///
/// Creates `vectors_vec` (a `vec0` virtual table) and `vectors_meta`
/// (its metadata companion) if they are not already there. Does
/// **not** touch any existing `vectors` table — migrating a v1
/// project onto the v0 layout is an explicit, user-initiated
/// operation (`coraline migrate vec-ext`), not a side effect of
/// running an embedding command.
///
/// Safe to call on every connection open — once `vectors_vec` exists,
/// the function is a fast no-op.
#[cfg(feature = "vec-ext")]
pub fn ensure_vec0_schema(conn: &Connection) -> io::Result<()> {
    if table_exists(conn, "vectors_vec")? {
        return Ok(());
    }

    // Creating a `vec0` virtual table needs the sqlite-vec module
    // registered on *this* connection. A connection opened before the
    // auto-extension was registered does not have it, so load it
    // explicitly rather than failing with `no such module: vec0`.
    //
    // `load_module` rather than `enable_extension`: this function's
    // job is to ensure the *tables* exist in this database. Registering
    // the process-wide auto-extension would be a side effect that
    // changes every other connection in the process.
    crate::vec_ext::runtime::load_module(conn)?;

    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS vectors_vec USING vec0(
             embedding float[768]
         );
         CREATE TABLE IF NOT EXISTS vectors_meta (
             rowid INTEGER PRIMARY KEY,
             node_id TEXT UNIQUE NOT NULL,
             model TEXT NOT NULL,
             created_at INTEGER NOT NULL
         );",
    )
    .map_err(super::db::io_other)?;

    conn.execute_batch(
        "INSERT OR IGNORE INTO schema_versions (version, applied_at, description)
         VALUES (4, strftime('%s', 'now') * 1000,
                 'Create vec0 embedding tables (Phase 5.3)');",
    )
    .map_err(super::db::io_other)?;
    Ok(())
}

#[cfg(feature = "vec-ext")]
fn table_exists(conn: &Connection, name: &str) -> io::Result<bool> {
    let mut stmt = conn
        .prepare("SELECT 1 FROM sqlite_master WHERE type IN ('table','view') AND name = ?1 LIMIT 1")
        .map_err(super::db::io_other)?;
    let mut rows = stmt.query(params![name]).map_err(super::db::io_other)?;
    Ok(rows.next().map_err(super::db::io_other)?.is_some())
}

/// Convert the on-disk v0 `vec0` layout back into the v1 BLOB
/// `vectors` table, so a project created with the `vec-ext` build can
/// be opened by the default build.
///
/// Reads every `vectors_vec` payload (via the sqlite-vec module) and
/// writes it into a fresh `vectors` table as a little-endian `f32`
/// blob, then drops the v0 tables.
///
/// Like the forward migration, this is explicit and user-initiated —
/// it changes the project's on-disk layout.
#[cfg(feature = "vec-ext")]
pub fn migrate_to_v1(project_root: &Path) -> std::io::Result<MigrationOutcome> {
    let conn = db::open_database(project_root)?;
    migrate_to_v1_conn(&conn)
}

/// Convert the v0 `vec0` layout back to the v1 BLOB layout on an open
/// connection.
#[cfg(feature = "vec-ext")]
pub fn migrate_to_v1_conn(conn: &Connection) -> std::io::Result<MigrationOutcome> {
    let layout = VectorLayout::detect(conn)?;
    match layout {
        VectorLayout::V1Blob | VectorLayout::Absent => {
            return Ok(MigrationOutcome::AlreadyOnTarget);
        }
        VectorLayout::V0Vec => {}
    }

    crate::vec_ext::runtime::enable_extension(conn)?;

    // Read the v0 payload + metadata out before dropping anything.
    let mut rows_to_migrate: Vec<(String, Vec<f32>, String, i64)> = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT m.node_id, v.embedding, m.model, m.created_at
                 FROM vectors_meta m
                 JOIN vectors_vec v ON v.rowid = m.rowid",
            )
            .map_err(super::db::io_other)?;
        let mut rows = stmt.query([]).map_err(super::db::io_other)?;
        while let Some(row) = rows.next().map_err(super::db::io_other)? {
            let node_id: String = row.get(0).map_err(super::db::io_other)?;
            // `vec0` returns `float[]` columns as raw little-endian
            // `f32` bytes, not as the JSON text that was fed to
            // `vec_f32()` on the way in — so this has to be read as a
            // generic value and decoded.
            let raw: rusqlite::types::Value = row.get(1).map_err(super::db::io_other)?;
            let floats =
                crate::vector_layout::decode_vec0_embedding(&raw).map_err(super::db::io_other)?;
            let model: String = row.get(2).map_err(super::db::io_other)?;
            let created_at: i64 = row.get(3).map_err(super::db::io_other)?;
            rows_to_migrate.push((node_id, floats, model, created_at));
        }
    }

    // The conversion runs in a single transaction, so the v0 tables are
    // only dropped once every row has been written to the v1 table.
    // Without it, a failure part-way through the inserts would leave
    // the project with both layouts half-populated.
    let tx = conn.unchecked_transaction().map_err(super::db::io_other)?;

    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS vectors (
             node_id TEXT PRIMARY KEY,
             embedding BLOB NOT NULL,
             model TEXT NOT NULL,
             created_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_vectors_model ON vectors(model);",
    )
    .map_err(super::db::io_other)?;

    for (node_id, floats, model, created_at) in &rows_to_migrate {
        let bytes: Vec<u8> = floats.iter().flat_map(|&f| f.to_le_bytes()).collect();
        tx.execute(
            "INSERT OR REPLACE INTO vectors (node_id, embedding, model, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![node_id, bytes, model, created_at],
        )
        .map_err(super::db::io_other)?;
    }

    // Drop the v0 tables. `vectors_vec` is a `vec0` virtual table, so
    // this needs the sqlite-vec module — which is registered, because
    // the read above already required it.
    tx.execute_batch(
        "DROP TABLE IF EXISTS vectors_vec;
         DROP TABLE IF EXISTS vectors_meta;",
    )
    .map_err(super::db::io_other)?;

    tx.commit().map_err(super::db::io_other)?;

    Ok(MigrationOutcome::Migrated)
}

/// Decode a little-endian f32 BLOB (the v1 storage format) back into a
/// `Vec<f32>`. Wraps `[u8; 4]` slices in the standard way.
#[cfg(feature = "vec-ext")]
fn decode_le_f32_vec(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

/// Serialize a slice of f32 as a JSON array string suitable for
/// `vec_f32(?)` parameter binding.
#[cfg(feature = "vec-ext")]
fn floats_to_json(floats: &[f32]) -> String {
    use std::fmt::Write as _;

    let mut s = String::with_capacity(2 + floats.len() * 12);
    s.push('[');
    for (i, f) in floats.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        // Compact representation; SQLite handles scientific notation.
        let _ = write!(s, "{f}");
    }
    s.push(']');
    s
}

// ---------------------------------------------------------------------------
// Feature-gated unsafe: sqlite-vec extension loading + runtime helpers.
// ---------------------------------------------------------------------------
//
// `sqlite-vec` ships only one symbol: `sqlite3_vec_init`, exposed as
// an `extern "C"` function. Registering it as a SQLite auto-extension
// is `unsafe`, but the workspace's `#![deny(unsafe_code)]` forbids
// it inline in `vectors.rs`. This module is the single allow-listed
// home for that one call.

#[cfg(feature = "vec-ext")]
#[allow(
    unsafe_code,
    reason = "registers sqlite3_auto_extension for sqlite-vec FFI"
)]
pub mod runtime {
    //! sqlite-vec runtime: extension registration + vec0-backed
    //! `store_embedding` / `search_similar` overrides.
    use std::io;

    use rusqlite::{Connection, params};

    use super::ensure_vec0_schema;

    /// Ensure `sqlite-vec` is available on `conn` and on every
    /// connection opened afterwards.
    ///
    /// Two things are needed, and they are not the same thing:
    ///
    /// 1. **This** connection must have the `vec0` module loaded.
    ///    `sqlite3_auto_extension` only fires when a connection is
    ///    *opened*, so registering it does nothing for a connection
    ///    that already exists — the init function is called directly
    ///    on the handle.
    /// 2. **Future** connections should get it automatically, which
    ///    the process-global auto-extension registration provides.
    ///
    /// Registering the module twice on the same connection is safe:
    /// `sqlite3_create_module_v2` replaces an existing module of the
    /// same name, and `sqlite3_auto_extension` ignores a duplicate
    /// entry point.
    pub fn enable_extension(conn: &Connection) -> io::Result<()> {
        // SAFETY: `sqlite3_vec_init` is the `extern "C"` symbol that
        // sqlite-vec exports. `register_auto_extension` expects a
        // `RawAutoExtension` — a different signature — so we wrap our
        // init through `init_auto_extension`, which adapts our
        // `extern "C" fn()` to the `RawAutoExtension` signature. The
        // init function itself is idempotent (it just registers a
        // virtual-table module).
        let raw: rusqlite::auto_extension::RawAutoExtension = init_auto_extension;
        unsafe {
            rusqlite::auto_extension::register_auto_extension(raw).map_err(io::Error::other)?;
        }
        load_module(conn)
    }

    /// Load the sqlite-vec module into a single connection, without
    /// registering the process-wide auto-extension.
    ///
    /// This is the narrow half of [`enable_extension`]: it affects
    /// only the connection it is given, and leaves every other
    /// connection in the process alone. That distinction matters for
    /// a build that must observe what a *different* build's project
    /// looks like — a default build has no sqlite-vec module at all,
    /// and a test that wanted to reproduce that state could not get
    /// it back once the auto-extension had been registered.
    pub fn load_module(conn: &Connection) -> io::Result<()> {
        load_into_connection(conn)
    }

    /// Process-wide one-time registration of the sqlite-vec auto-extension.
    ///
    /// Call this at startup — *before* any `Connection::open` happens —
    /// so every new connection the process opens has `vec0` available
    /// as a virtual-table module. The per-connection
    /// [`enable_extension`] entry point covers the case where
    /// `vec_ext` is being used from a library context that has its
    /// own connection lifecycle; for the CLI binary, calling this
    /// from `main()` is enough — the auto-extension handles every
    /// subsequent connection automatically.
    pub fn register_global_init() -> io::Result<()> {
        let raw: rusqlite::auto_extension::RawAutoExtension = init_auto_extension;
        unsafe {
            rusqlite::auto_extension::register_auto_extension(raw).map_err(io::Error::other)?;
        }
        Ok(())
    }

    /// Load the sqlite-vec module into an already-open connection.
    ///
    /// `sqlite3_auto_extension` cannot be used for this: it only runs
    /// its callbacks when a connection is opened, so a connection that
    /// predates the registration never gets the module. Calling the
    /// init function directly against the connection handle is the
    /// only way to backfill it.
    fn load_into_connection(conn: &Connection) -> io::Result<()> {
        // SAFETY: `conn` is alive for the duration of the call, and
        // `sqlite3_vec_init` only registers functions and a virtual
        // table module against the handle it is given — it does not
        // take ownership of it.
        //
        // `p_api` is passed as NULL, which is safe because the
        // `sqlite-vec` crate compiles `sqlite-vec.c` with
        // `-DSQLITE_CORE`, and the `SQLITE_EXTENSION_INIT2(pApi)` call
        // inside `sqlite3_vec_init` is therefore compiled out. If that
        // build flag ever changed, `sqlite3_api` would be assigned
        // this NULL and the next `sqlite3_api->…` call would
        // segfault.
        let rc = unsafe {
            let mut err_msg: *mut std::ffi::c_char = std::ptr::null_mut();
            let rc = sqlite3_vec_init(conn.handle(), &raw mut err_msg, std::ptr::null());

            // `sqlite3_vec_init` allocates the message with
            // `sqlite3_mprintf`, so it has to be freed with
            // `sqlite3_free`. It is freed whenever it is set, not only
            // on the error path, so a future release that populated it
            // on a success would not leak it.
            let message = if err_msg.is_null() {
                None
            } else {
                let message = std::ffi::CStr::from_ptr(err_msg)
                    .to_string_lossy()
                    .into_owned();
                rusqlite::ffi::sqlite3_free(err_msg.cast::<std::ffi::c_void>());
                Some(message)
            };

            if rc != rusqlite::ffi::SQLITE_OK {
                return Err(io::Error::other(message.map_or_else(
                    || format!("failed to load the sqlite-vec extension (SQLite code {rc})"),
                    |message| format!("failed to load the sqlite-vec extension: {message}"),
                )));
            }
            rc
        };
        if rc != rusqlite::ffi::SQLITE_OK {
            return Err(io::Error::other(format!(
                "failed to load the sqlite-vec extension (SQLite code {rc})"
            )));
        }
        Ok(())
    }

    /// Raw bridge: adapts sqlite-vec's `sqlite3_vec_init` to the
    /// `RawAutoExtension` callback signature.
    ///
    /// sqlite-vec's real entry point has the standard SQLite extension
    /// signature:
    ///
    /// ```c
    /// int sqlite3_vec_init(sqlite3 *db, char **pzErrMsg,
    ///                     const sqlite3_api_routines *pApi);
    /// ```
    ///
    /// The `sqlite-vec` crate declares it with no arguments (it only
    /// ever calls it through a transmuted function pointer), so the
    /// declaration is repeated here with the true signature. The
    /// `db` handle is what the init function uses to register the
    /// `vec0` virtual-table module and the `vec_*` SQL functions, so
    /// dropping it — as a zero-argument call does — would register
    /// them against a garbage handle.
    ///
    /// The return code and the error message are propagated rather
    /// than swallowed, so a failed init surfaces as a failed
    /// connection open instead of a confusing `no such module: vec0`
    /// later on. sqlite-vec sets `*pzErrMsg` on both of its failure
    /// paths (with `sqlite3_mprintf`, so the caller owns the string).
    extern "C" fn init_auto_extension(
        db: *mut rusqlite::ffi::sqlite3,
        pz_err_msg: *mut *mut std::ffi::c_char,
        p_api: *const rusqlite::ffi::sqlite3_api_routines,
    ) -> std::ffi::c_int {
        // SAFETY: the three arguments are forwarded verbatim from
        // SQLite's own auto-extension callback, which supplies a
        // valid connection handle and a valid `sqlite3_api_routines`
        // pointer. `sqlite3_vec_init` is declared here with that
        // exact signature, so the call matches the function's real
        // ABI — the `sqlite-vec` crate's own zero-argument
        // declaration does not, and calling through it would be UB.
        unsafe { sqlite3_vec_init(db, pz_err_msg, p_api) }
    }

    /// Store an embedding in the vec0 `vectors_vec` table plus the
    /// `vectors_meta` companion row. Called from `vectors::store_embedding`
    /// when the `vec-ext` Cargo feature is enabled.
    pub fn store_embedding_vec0(
        conn: &Connection,
        node_id: &str,
        embedding: &[f32],
        model_name: &str,
    ) -> io::Result<()> {
        ensure_vec0_schema(conn)?;
        let json = super::floats_to_json(embedding);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| io::Error::other(format!("clock error: {e}")))?
            .as_millis();
        let now = i64::try_from(now).unwrap_or(0);

        // Upsert pattern: if a row already exists for this node_id,
        // drop the old mapping first. The vec0 row stays (sqlite-vec
        // doesn't support UPDATE on `float[]` columns — rows are
        // immutable, so we delete + insert and let the prior rowid
        // dangle). Embeddings are rebuildable, so the brief moment of
        // inconsistency between DELETE and INSERT is acceptable.
        conn.execute(
            "DELETE FROM vectors_meta WHERE node_id = ?1",
            params![node_id],
        )
        .map_err(super::db::io_other)?;
        conn.execute(
            "INSERT INTO vectors_vec (embedding) VALUES (vec_f32(?1))",
            params![json],
        )
        .map_err(super::db::io_other)?;
        let rowid = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO vectors_meta (rowid, node_id, model, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![rowid, node_id, model_name, now],
        )
        .map_err(super::db::io_other)?;
        Ok(())
    }

    pub fn load_embedding_vec0(conn: &Connection, node_id: &str) -> io::Result<Option<Vec<f32>>> {
        ensure_vec0_schema(conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT v.embedding
                 FROM vectors_vec v
                 JOIN vectors_meta m ON m.rowid = v.rowid
                 WHERE m.node_id = ?1",
            )
            .map_err(super::db::io_other)?;
        let mut rows = stmt.query(params![node_id]).map_err(super::db::io_other)?;
        match rows.next().map_err(super::db::io_other)? {
            Some(row) => {
                let raw: rusqlite::types::Value = row.get(0).map_err(super::db::io_other)?;
                // vec0 returns `float[]` columns as raw f32 little-endian
                // BLOBs. Some builds may serialise as text instead, so
                // accept both shapes.
                Ok(Some(
                    crate::vector_layout::decode_vec0_embedding(&raw)
                        .map_err(super::db::io_other)?,
                ))
            }
            None => Ok(None),
        }
    }

    /// Search for nodes similar to the query embedding using vec0's
    /// KNN operator. Returns `SearchResult` rows (which live in
    /// `crate::types`) ordered by cosine *distance* (lowest = most
    /// similar).
    ///
    /// `min_similarity` (the public API's cosine similarity, range
    /// -1..1) is converted to the equivalent max distance:
    /// `1 - min_similarity`.
    pub fn search_similar_vec0(
        conn: &Connection,
        query_embedding: &[f32],
        model: &str,
        limit: usize,
        min_similarity: f32,
    ) -> io::Result<Vec<crate::types::SearchResult>> {
        ensure_vec0_schema(conn)?;
        let json = super::floats_to_json(query_embedding);
        let max_distance = 1.0_f32 - min_similarity;
        // sqlite-vec KNN requires the `k = ?` constraint directly in
        // the WHERE clause of the MATCH query — plain `LIMIT ?` doesn't
        // get recognised by its parser, and extra JOINed predicates on
        // the same level confuse it. We isolate the KNN scan in a CTE
        // and apply the model + similarity filters after.
        let mut stmt = conn
            .prepare(
                "WITH knn AS (
                     SELECT rowid, distance
                     FROM vectors_vec
                     WHERE embedding MATCH vec_f32(?1)
                       AND k = ?4
                 )
                 SELECT m.node_id, knn.distance
                 FROM knn
                 JOIN vectors_meta m ON m.rowid = knn.rowid
                 WHERE m.model = ?2
                   AND knn.distance <= ?3
                 ORDER BY knn.distance ASC",
            )
            .map_err(super::db::io_other)?;

        let rows = stmt
            .query_map(
                params![
                    json,
                    model,
                    max_distance,
                    i64::try_from(limit).unwrap_or(i64::MAX)
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, f32>(1)?)),
            )
            .map_err(super::db::io_other)?;

        let mut results = Vec::new();
        for row in rows {
            let (node_id, distance) = row.map_err(super::db::io_other)?;
            let Some(node) =
                super::db::get_node_by_id(conn, &node_id).map_err(super::db::io_other)?
            else {
                continue;
            };
            let similarity = 1.0 - distance;
            results.push(crate::types::SearchResult {
                node,
                score: similarity,
                highlights: None,
            });
        }
        Ok(results)
    }

    // sqlite-vec exports this `extern "C"` symbol from its `lib.rs`.
    // We re-declare it here so the `#[link]` attribute propagates the
    // static-library link directive through to the final binary —
    // otherwise the linker only sees the symbol inside sqlite-vec's
    // own crate and never pulls `libsqlite_vec0.a` into coraline's
    // binary link line.
    //
    // The declaration carries sqlite-vec's *real* signature
    // (`int sqlite3_vec_init(sqlite3*, char**, const
    // sqlite3_api_routines*)`). The `sqlite-vec` crate itself declares
    // it with no arguments, which is only safe there because it never
    // calls it with a handle. We do need the handle: the init
    // function registers `vec0` and the `vec_*` functions against it.
    #[link(name = "sqlite_vec0", kind = "static")]
    unsafe extern "C" {
        fn sqlite3_vec_init(
            db: *mut rusqlite::ffi::sqlite3,
            pz_err_msg: *mut *mut std::ffi::c_char,
            p_api: *const rusqlite::ffi::sqlite3_api_routines,
        ) -> std::ffi::c_int;
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::db;

        fn fresh_db() -> Result<Connection, Box<dyn std::error::Error>> {
            // sqlite-vec registers itself as a SQLite auto-extension,
            // which means the callback fires only on connection OPEN.
            // Register the extension *first*, then open the connection.
            enable_extension_dummy()?;

            let conn = rusqlite::Connection::open_in_memory()?;
            conn.execute_batch(db::SCHEMA_SQL)?;
            db::apply_incremental_migrations(&conn)?;
            // Create the vec0 + meta tables that migrate_to_vec0 would
            // create on disk. The default `vectors` BLOB table is
            // dropped by the migration if it exists; here we don't run
            // the migration path and just create the vec0 tables
            // directly on top of the empty schema.
            conn.execute_batch(
                "CREATE VIRTUAL TABLE vectors_vec USING vec0(
                     embedding float[768]
                 );
                 CREATE TABLE vectors_meta (
                     rowid INTEGER PRIMARY KEY,
                     node_id TEXT UNIQUE NOT NULL,
                     model TEXT NOT NULL,
                     created_at INTEGER NOT NULL
                 );",
            )?;
            Ok(conn)
        }

        #[test]
        fn enable_extension_makes_vec_version_queryable() -> Result<(), Box<dyn std::error::Error>>
        {
            let conn = fresh_db()?;

            // After enabling sqlite-vec, the vec_version() SQL function
            // should be registered.
            let version: String = conn.query_row("SELECT vec_version()", [], |row| row.get(0))?;
            assert!(version.starts_with('v'), "got version: {version}");
            Ok(())
        }

        #[test]
        fn store_and_load_embedding_round_trip() -> Result<(), Box<dyn std::error::Error>> {
            let conn = fresh_db()?;
            enable_extension(&conn)?;

            let mut embedding = vec![0.0_f32; 768];
            if let Some(v) = embedding.get_mut(0) {
                *v = 1.0;
            }
            if let Some(v) = embedding.get_mut(100) {
                *v = 0.5;
            }
            if let Some(v) = embedding.get_mut(500) {
                *v = -0.25;
            }

            store_embedding_vec0(&conn, "node-a", &embedding, "nomic-embed-text-v1.5")?;

            let loaded = load_embedding_vec0(&conn, "node-a")?.ok_or("embedding not found")?;
            assert_eq!(loaded.len(), 768);
            assert!((loaded.first().copied().unwrap_or(0.0) - 1.0).abs() < 1e-6);
            assert!((loaded.get(100).copied().unwrap_or(0.0) - 0.5).abs() < 1e-6);
            assert!((loaded.get(500).copied().unwrap_or(0.0) + 0.25).abs() < 1e-6);
            Ok(())
        }

        #[test]
        fn store_embedding_vec0_is_an_upsert() -> Result<(), Box<dyn std::error::Error>> {
            let conn = fresh_db()?;
            enable_extension(&conn)?;

            let mut v1 = vec![0.0_f32; 768];
            if let Some(slot) = v1.get_mut(0) {
                *slot = 1.0;
            }
            store_embedding_vec0(&conn, "node-x", &v1, "m1")?;

            let mut v2 = vec![0.0_f32; 768];
            if let Some(slot) = v2.get_mut(1) {
                *slot = 1.0;
            }
            store_embedding_vec0(&conn, "node-x", &v2, "m2")?;

            let loaded = load_embedding_vec0(&conn, "node-x")?.ok_or("embedding not found")?;
            assert!(loaded.first().copied().unwrap_or(1.0).abs() < 1e-6);
            assert!((loaded.get(1).copied().unwrap_or(0.0) - 1.0).abs() < 1e-6);

            let model: String = conn.query_row(
                "SELECT model FROM vectors_meta WHERE node_id = ?1",
                rusqlite::params!["node-x"],
                |row| row.get(0),
            )?;
            assert_eq!(model, "m2");
            Ok(())
        }

        #[test]
        fn search_similar_returns_nearest_first() -> Result<(), Box<dyn std::error::Error>> {
            let conn = fresh_db()?;
            enable_extension(&conn)?;

            let mut a = vec![0.0_f32; 768];
            if let Some(slot) = a.get_mut(0) {
                *slot = 1.0;
            }
            let mut b = vec![0.0_f32; 768];
            if let Some(slot) = b.get_mut(1) {
                *slot = 1.0;
            }
            let mut c = vec![0.0_f32; 768];
            if let Some(slot) = c.get_mut(2) {
                *slot = 1.0;
            }
            let query = a.clone();

            store_embedding_vec0(&conn, "a", &a, "m")?;
            store_embedding_vec0(&conn, "b", &b, "m")?;
            store_embedding_vec0(&conn, "c", &c, "m")?;

            // `search_similar_vec0` joins against the `nodes` table to
            // fill in `SearchResult` fields, so seed those rows too.
            for id in ["a", "b", "c"] {
                conn.execute(
                    "INSERT INTO nodes (id, kind, name, qualified_name, file_path, language,
                     start_line, end_line, start_column, end_column,
                     is_exported, is_async, is_static, is_abstract, updated_at)
                     VALUES (?1, 'function', ?1, ?1, 'src/lib.rs', 'rust',
                     1, 1, 0, 0, 1, 0, 0, 0, 0)",
                    [id],
                )?;
            }

            // sqlite-vec's `distance` is L2 (Euclidean), not cosine.
            // For unit vectors at opposite axes, L2 distance is
            // sqrt(2) ≈ 1.414. Use `min_similarity = -1.0` so
            // `max_distance = 2.0` covers all unit-vector pairs.
            let results = search_similar_vec0(&conn, &query, "m", 3, -1.0)?;
            assert_eq!(results.len(), 3);
            let first_id = results
                .first()
                .map(|r| r.node.id.as_str())
                .ok_or("expected at least one result")?;
            assert_eq!(first_id, "a");
            Ok(())
        }

        fn enable_extension_dummy() -> Result<(), Box<dyn std::error::Error>> {
            // SAFETY: sqlite-vec's `sqlite3_vec_init` is the documented
            // auto-extension entry point. `init_auto_extension` is the
            // raw C signature bridge that `register_auto_extension`
            // expects — it wraps a safe `AutoExtension` callback into a
            // C-compatible `extern "C" fn`. The init function itself
            // is idempotent.
            let raw: rusqlite::auto_extension::RawAutoExtension = init_auto_extension;
            unsafe {
                rusqlite::auto_extension::register_auto_extension(raw)?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remediation_hint_matches_every_state() {
        // Every (feature, layout) pair should yield *some* hint when
        // the binary wasn't built with vec-ext, or when the layout is
        // not yet on v0. The only no-hint case is feature=on + v0.
        for &feature in &[false, true] {
            for layout in [
                VectorLayout::Absent,
                VectorLayout::V1Blob,
                VectorLayout::V0Vec,
            ] {
                let hint = remediation_hint(feature, layout);
                match (feature, layout) {
                    (true, VectorLayout::V0Vec) => assert!(hint.is_none()),
                    _ => assert!(hint.is_some(), "missing hint for ({feature:?}, {layout:?})"),
                }
            }
        }
    }

    #[test]
    fn feature_flag_constant_is_compile_time() {
        // Sanity: the constant reflects the build's feature state and
        // is `const`-evaluable.
        let _: bool = VEC_EXT_ENABLED;
    }
}
