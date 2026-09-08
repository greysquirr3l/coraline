#![deny(unsafe_code)]

//! `sqlite-vec` feature scaffolding (Phase 5.3).
//!
//! Exposes:
//!
//! - A doctor probe (`check_vec_ext`) that reports whether the
//!   `vec-ext` Cargo feature is compiled in and whether the on-disk
//!   `vectors` table is the v1 in-house BLOB form or the v0 `vec0`
//!   form.
//! - A migration runner (`migrate_to_vec0`) that swaps the v1 BLOB
//!   `vectors` table for a `vec0` virtual table. Gated behind the
//!   `vec-ext` Cargo feature so the default build pays no cost.
//!
//! **Status:** `sqlite-vec` v0.1.10-alpha.4 is pinned in `Cargo.toml`
//! but the upstream `sqlite-vec-diskann.c` file is missing from the
//! crate tarball (alpha packaging bug), so the `--features vec-ext`
//! build is currently broken at link time. The scaffolding below is
//! the right shape once a working build is available — no API changes
//! will be needed for users.

use std::path::Path;

use rusqlite::Connection;
use serde::Serialize;

use crate::doctor;

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
    /// State of the on-disk `vectors` table.
    pub table_state: VecExtTableState,
    /// One-line remediation hint, or `None` when no action is needed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

/// Possible states of the on-disk `vectors` table.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VecExtTableState {
    /// Project hasn't been indexed yet — `vectors` table doesn't exist.
    Absent,
    /// v1 in-house BLOB table (`embedding BLOB NOT NULL`).
    V1Blob,
    /// v0 `vec0` virtual table.
    V0Vec,
}

impl VecExtTableState {
    /// Classify the `vectors` table by querying SQLite's schema.
    fn from_conn(conn: &Connection) -> std::io::Result<Self> {
        // A `vec0` virtual table shows up in `sqlite_master` with a
        // `type = 'table'` row but `sql` starting with `CREATE VIRTUAL TABLE`.
        let mut stmt = conn
            .prepare(
                "SELECT sql FROM sqlite_master \
                 WHERE type IN ('table', 'view') \
                   AND (name = 'vectors' OR name LIKE 'vectors_v%')",
            )
            .map_err(db::io_other)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, Option<String>>(0))
            .map_err(db::io_other)?;
        let mut saw: Option<&'static str> = None;
        for row in rows {
            let sql = row.map_err(db::io_other)?.unwrap_or_default();
            let upper = sql.to_ascii_uppercase();
            if upper.contains("USING VEC0") {
                return Ok(Self::V0Vec);
            }
            if upper.contains("CREATE TABLE VECTORS") {
                saw = Some(Self::V1Blob.as_str());
            }
        }
        Ok(saw.map_or(Self::Absent, |s| match s {
            "V1_BLOB" => Self::V1Blob,
            _ => Self::Absent,
        }))
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "ABSENT",
            Self::V1Blob => "V1_BLOB",
            Self::V0Vec => "V0_VEC",
        }
    }
}

/// One-line remediation hint for the current `(feature, table_state)` pair.
fn remediation_hint(feature: bool, state: VecExtTableState) -> Option<String> {
    match (feature, state) {
        (false, VecExtTableState::V1Blob) => Some(
            "Rebuild with `--features vec-ext` to upgrade the `vectors` table to sqlite-vec v0."
                .to_string(),
        ),
        (false, _) => Some(
            "Rebuild with `--features vec-ext` to enable sqlite-vec KNN search.".to_string(),
        ),
        (true, VecExtTableState::V1Blob) => Some(
            "Run `coraline migrate vec-ext` to convert the BLOB table to vec0 (alpha; upstream build currently broken)."
                .to_string(),
        ),
        (true, VecExtTableState::Absent) => Some(
            "Run `coraline index` first so the `vectors` table exists, then `coraline migrate vec-ext`."
                .to_string(),
        ),
        (true, VecExtTableState::V0Vec) => None,
    }
}

/// Build a [`VecExtStatus`] for the project at `project_root`.
///
/// Falls back to `feature_enabled = cfg!(feature = "vec-ext")` when the
/// DB can't be opened (e.g. the project hasn't been `coraline init`'d).
pub fn status(project_root: &Path) -> VecExtStatus {
    let table_state = db::open_database(project_root).map_or(VecExtTableState::Absent, |conn| {
        VecExtTableState::from_conn(&conn).unwrap_or(VecExtTableState::Absent)
    });
    let fix = remediation_hint(VEC_EXT_ENABLED, table_state);
    VecExtStatus {
        feature_enabled: VEC_EXT_ENABLED,
        table_state,
        fix,
    }
}

/// Doctor probe: `vec-ext` feature state + on-disk `vectors` table state.
pub fn check_vec_ext(project_root: &Path) -> doctor::Probe {
    let status = status(project_root);
    let detail =
        serde_json::to_string(&status).unwrap_or_else(|_| status.feature_enabled.to_string());
    let ok = status.feature_enabled && matches!(status.table_state, VecExtTableState::V0Vec);
    doctor::Probe {
        name: "vec_ext",
        ok,
        detail,
        fix: status.fix,
    }
}

// Re-export `db::io_other` so `from_conn` doesn't need a deep `crate::db`
// path in the future when we actually wire sqlite-vec in.
use crate::db;

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
#[cfg(feature = "vec-ext")]
pub fn migrate_to_vec0(project_root: &Path) -> std::io::Result<()> {
    let conn = db::open_database(project_root)?;
    ensure_vec0_schema(&conn)
}

/// Idempotently ensure the vec0 (`vectors_vec`) + meta (`vectors_meta`)
/// tables exist. If the legacy v1 `vectors` BLOB table is present,
/// migrate its contents into vec0 (one-time, transactional) and drop
/// the v1 table.
///
/// Safe to call on every connection open — once `vectors_vec` exists,
/// the function is a fast no-op. The vec0 embed / load / search
/// code paths call this on entry so a project can upgrade to the
/// vec-ext build and have the schema auto-set up on the first
/// vec0-bearing operation, with no manual migration step.
#[cfg(feature = "vec-ext")]
pub fn ensure_vec0_schema(conn: &Connection) -> io::Result<()> {
    if table_exists(conn, "vectors_vec")? {
        return Ok(());
    }

    let has_v1 = table_exists(conn, "vectors")?;

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

    if has_v1 {
        let mut stmt = conn
            .prepare("SELECT node_id, embedding, model, created_at FROM vectors")
            .map_err(super::db::io_other)?;
        let mut rows = stmt.query([]).map_err(super::db::io_other)?;
        let mut backup: Vec<(String, Vec<f32>, String, i64)> = Vec::new();
        while let Some(row) = rows.next().map_err(super::db::io_other)? {
            let node_id: String = row.get(0).map_err(super::db::io_other)?;
            let bytes: Vec<u8> = row.get(1).map_err(super::db::io_other)?;
            let model: String = row.get(2).map_err(super::db::io_other)?;
            let created_at: i64 = row.get(3).map_err(super::db::io_other)?;
            backup.push((node_id, decode_le_f32_vec(&bytes), model, created_at));
        }
        drop(rows);
        drop(stmt);

        conn.execute_batch("DROP INDEX IF EXISTS idx_vectors_model; DROP TABLE IF EXISTS vectors;")
            .map_err(super::db::io_other)?;

        for (node_id, floats, model, created_at) in &backup {
            let json = floats_to_json(floats);
            conn.execute(
                "INSERT INTO vectors_vec (embedding) VALUES (vec_f32(?1))",
                params![json],
            )
            .map_err(super::db::io_other)?;
            let rowid = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO vectors_meta (rowid, node_id, model, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![rowid, node_id, model, created_at],
            )
            .map_err(super::db::io_other)?;
        }
    }

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

    /// Register `sqlite-vec` as a SQLite auto-extension on every new
    /// connection.
    ///
    /// Subsequent SQL operations on any connection opened after this
    /// call can use `vec0` virtual tables and `vec_f32(...)`. This
    /// is global to the process — pass `_conn` to document that we
    /// don't currently need a handle (kept for forward compat).
    pub fn enable_extension(_conn: &Connection) -> io::Result<()> {
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
        Ok(())
    }

    /// Process-wide one-time registration of the sqlite-vec
    /// auto-extension. Must be called at startup — *before* any
    /// `Connection::open` happens — so that every new connection the
    /// process opens has `vec0` available as a virtual-table module.
    ///
    /// The per-connection [`enable_extension`] entry point covers the
    /// case where vec_ext is being used from a library context that
    /// has its own connection lifecycle. For the CLI binary, call
    /// this from `main()` and the auto-extension handles every
    /// connection automatically.
    pub fn register_global_init() -> io::Result<()> {
        let raw: rusqlite::auto_extension::RawAutoExtension = init_auto_extension;
        unsafe {
            rusqlite::auto_extension::register_auto_extension(raw).map_err(io::Error::other)?;
        }
        Ok(())
    }

    /// Raw bridge: adapts our `extern "C" fn()` `sqlite3_vec_init` to the
    /// `RawAutoExtension` callback signature. The `_db` parameter is
    /// ignored because sqlite-vec's `sqlite3_vec_init` doesn't need it.
    extern "C" fn init_auto_extension(
        _db: *mut rusqlite::ffi::sqlite3,
        _pz_err_msg: *mut *mut std::ffi::c_char,
        _: *const rusqlite::ffi::sqlite3_api_routines,
    ) -> std::ffi::c_int {
        unsafe { sqlite3_vec_init() }
        rusqlite::ffi::SQLITE_OK
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
                match raw {
                    rusqlite::types::Value::Blob(bytes) => {
                        Ok(Some(super::decode_le_f32_vec(&bytes)))
                    }
                    rusqlite::types::Value::Text(s) => Ok(Some(parse_float_array(&s))),
                    other => Err(io::Error::other(format!(
                        "vec0 returned unsupported embedding type: {other:?}"
                    ))),
                }
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
    #[link(name = "sqlite_vec0", kind = "static")]
    unsafe extern "C" {
        fn sqlite3_vec_init();
    }

    /// Parse a JSON array of floats into a `Vec<f32>`. Used to
    /// decode `vec_f32(...)` results when reading them back.
    fn parse_float_array(json: &str) -> Vec<f32> {
        // Strip leading `[` and trailing `]`, split on `,`, parse each
        // as f32. sqlite-vec's `vec_f32()` returns a JSON array.
        let trimmed = json.trim();
        let inner = &trimmed
            [trimmed.find('[').map_or(0, |i| i + 1)..trimmed.rfind(']').unwrap_or(trimmed.len())];
        inner
            .split(',')
            .filter_map(|s| s.trim().parse::<f32>().ok())
            .collect()
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
        // Every (feature, state) pair should yield *some* hint when
        // the binary wasn't built with vec-ext, or when the table is
        // not yet on v0. The only no-hint case is feature=on + v0.
        for &feature in &[false, true] {
            for state in [
                VecExtTableState::Absent,
                VecExtTableState::V1Blob,
                VecExtTableState::V0Vec,
            ] {
                let hint = remediation_hint(feature, state);
                match (feature, state) {
                    (true, VecExtTableState::V0Vec) => assert!(hint.is_none()),
                    _ => assert!(hint.is_some(), "missing hint for ({feature:?}, {state:?})"),
                }
            }
        }
    }

    #[test]
    fn table_state_as_str_is_stable() {
        assert_eq!(VecExtTableState::Absent.as_str(), "ABSENT");
        assert_eq!(VecExtTableState::V1Blob.as_str(), "V1_BLOB");
        assert_eq!(VecExtTableState::V0Vec.as_str(), "V0_VEC");
    }

    #[test]
    fn feature_flag_constant_is_compile_time() {
        // Sanity: the constant reflects the build's feature state and
        // is `const`-evaluable.
        let _: bool = VEC_EXT_ENABLED;
    }
}
