#![forbid(unsafe_code)]

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
/// table. Preserves existing embeddings: rows are copied into a
/// `_vectors_v1_backup` temp table, the v1 table is dropped, the v0
/// table is created, and rows are re-inserted.
///
/// **Currently inert:** the upstream `sqlite-vec` crate fails to build
/// (missing `sqlite-vec-diskann.c` in v0.1.10-alpha.4). Once that's
/// fixed, this function is the single integration point.
pub fn migrate_to_vec0(project_root: &Path) -> std::io::Result<()> {
    let mut conn = db::open_database(project_root)?;
    let tx = conn.transaction().map_err(db::io_other)?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS _vectors_v1_backup (
             node_id TEXT PRIMARY KEY,
             embedding BLOB NOT NULL,
             model TEXT NOT NULL,
             created_at INTEGER NOT NULL
         );
         INSERT INTO _vectors_v1_backup SELECT * FROM vectors;
         DROP TABLE vectors;
         CREATE VIRTUAL TABLE vectors USING vec0(
             node_id TEXT PRIMARY KEY,
             embedding float[384]
         );
         INSERT INTO vectors (node_id, embedding)
             SELECT node_id, vec_f32(embedding)
             FROM _vectors_v1_backup;
         DROP TABLE _vectors_v1_backup;
         INSERT OR IGNORE INTO schema_versions (version, applied_at, description)
         VALUES (4, strftime('%s', 'now') * 1000,
                 'Convert vectors table to vec0 (Phase 5.3)');",
    )
    .map_err(db::io_other)?;
    tx.commit().map_err(db::io_other)?;
    Ok(())
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
