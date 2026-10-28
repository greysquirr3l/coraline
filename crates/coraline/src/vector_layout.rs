#![deny(unsafe_code)]

//! Runtime detection of the on-disk vector storage layout.
//!
//! # Why this exists
//!
//! Coraline has two on-disk layouts for embedding storage:
//!
//! - **v1 (BLOB)** — a single `vectors` table holding a little-endian
//!   `f32` blob per node. Created by `SCHEMA_SQL` on every
//!   `initialize_database`.
//! - **v0 (sqlite-vec)** — a `vectors_vec` `vec0` virtual table holding
//!   the vectors, plus a sibling `vectors_meta` table that maps
//!   `rowid -> (node_id, model, created_at)`. Created by
//!   `vec_ext::ensure_vec0_schema`.
//!
//! The layout is a property of the **database file**, not of the
//! binary that opens it. A build without the `vec-ext` Cargo feature
//! can legitimately find a v0 database on disk (someone ran the
//! vec-ext build first), and a build *with* `vec-ext` can
//! legitimately find a v1 database (every project created before the
//! vec-ext build existed).
//!
//! The bug this module fixes: the SQL used to talk to the vector
//! tables was selected with `#[cfg(feature = "vec-ext")]` — a
//! **compile-time** switch standing in for a **runtime** property.
//! That produced a broken 2x2 matrix of (default | vec-ext) build
//! against (v1 | v0) database, in which half the combinations failed
//! outright and one degraded silently.
//!
//! # The fix
//!
//! Every vector-related statement resolves the table it needs from
//! the layout detected here, at runtime, from the open connection.
//!
//! The two layouts expose *identical metadata columns* — `node_id`,
//! `model`, `created_at` — in different tables:
//!
//! | Layout | Metadata table | Payload table |
//! | ------ | -------------- | ------------- |
//! | v1     | `vectors`      | `vectors` (`.embedding` column) |
//! | v0     | `vectors_meta` | `vectors_vec` |
//!
//! So every *metadata* query (counts, staleness, unembedded-node
//! coverage) can be written once against a resolved table name, and
//! only *payload* queries (store / load / KNN search) genuinely need
//! the vec0 module.

use std::io;

use rusqlite::Connection;
use serde::Serialize;

use crate::db::io_other;

/// The on-disk layout of a project's embedding storage.
///
/// Detected at runtime from the open database, never from Cargo
/// features. See the module docs for the full rationale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorLayout {
    /// No vector tables exist yet — the project has never stored an
    /// embedding (or has been freshly initialized).
    Absent,
    /// v1 in-house BLOB layout: a `vectors` table with an
    /// `embedding BLOB NOT NULL` column.
    V1Blob,
    /// v0 sqlite-vec layout: a `vectors_vec` `vec0` virtual table plus
    /// a `vectors_meta` companion table.
    V0Vec,
}

impl VectorLayout {
    /// Detect the vector storage layout of the open database.
    ///
    /// Queries `sqlite_master` for the three tables that make up the
    /// two layouts and classifies the result. Detection never fails on
    /// a missing table — a project with no vector storage at all
    /// reports [`VectorLayout::Absent`].
    ///
    /// # Errors
    ///
    /// Returns an error if the schema query itself fails.
    pub fn detect(conn: &Connection) -> io::Result<Self> {
        // `sqlite_master` stores the original `CREATE` text, so we can
        // tell a `vec0` virtual table (`CREATE VIRTUAL TABLE ... USING
        // VEC0`) from an ordinary table (`CREATE TABLE vectors (...)`).
        //
        // We look up the three tables *by name* rather than with a
        // `LIKE 'vectors_v%'` pattern on purpose: a vec0 virtual table
        // creates shadow tables (`vectors_vec_info`,
        // `vectors_vec_chunks`, `vectors_vec_rowids`,
        // `vectors_vec_vector_chunks00`) that a wildcard would also
        // match, and those shadow tables are ordinary tables whose
        // `CREATE` text does not mention `VEC0`.
        let mut stmt = conn
            .prepare(
                "SELECT name, sql FROM sqlite_master \
                 WHERE type IN ('table', 'view') \
                   AND name IN ('vectors', 'vectors_meta', 'vectors_vec')",
            )
            .map_err(io_other)?;

        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .map_err(io_other)?;

        let mut has_vectors = false;
        let mut has_meta = false;
        let mut has_vec = false;

        for row in rows {
            let (name, sql) = row.map_err(io_other)?;
            let upper = sql.unwrap_or_default().to_ascii_uppercase();
            match name.as_str() {
                "vectors" => has_vectors = true,
                "vectors_meta" => has_meta = true,
                // Only a genuine `vec0` virtual table counts. A
                // plain table with the same name would be something
                // else entirely and must not be treated as vec0
                // storage.
                "vectors_vec" if upper.contains("USING VEC0") => has_vec = true,
                _ => {}
            }
        }

        Ok(if has_vec || has_meta {
            Self::V0Vec
        } else if has_vectors {
            Self::V1Blob
        } else {
            Self::Absent
        })
    }

    /// The table holding embedding **metadata** (`node_id`, `model`,
    /// `created_at`), if the layout has one.
    ///
    /// Both layouts expose identical metadata columns, so a single
    /// query shape works against either — only the table name differs.
    pub const fn metadata_table(self) -> Option<&'static str> {
        match self {
            Self::Absent => None,
            Self::V1Blob => Some("vectors"),
            Self::V0Vec => Some("vectors_meta"),
        }
    }

    /// The table holding embedding **payloads** (the `f32` vectors),
    /// if the layout has one.
    ///
    /// - v1: the `vectors` table itself, payload in its `embedding`
    ///   column.
    /// - v0: the `vectors_vec` `vec0` virtual table, payload in the
    ///   `embedding` column of a row identified by `rowid`.
    pub const fn payload_table(self) -> Option<&'static str> {
        match self {
            Self::Absent => None,
            Self::V1Blob => Some("vectors"),
            Self::V0Vec => Some("vectors_vec"),
        }
    }

    /// `true` when the layout stores vectors in a sqlite-vec `vec0`
    /// virtual table.
    ///
    /// Reading and writing `vec0` payloads requires the sqlite-vec
    /// extension to be loaded, which only the `vec-ext` build does.
    pub const fn uses_vec0(self) -> bool {
        matches!(self, Self::V0Vec)
    }

    /// `true` when this layout can store a vector payload with the
    /// current binary.
    ///
    /// A `vec0` payload cannot be written or read without the
    /// sqlite-vec extension, so on the default build a v0 database is
    /// readable for metadata but *not* writable for payloads.
    pub const fn payload_writable(self) -> bool {
        match self {
            Self::Absent | Self::V1Blob => true,
            Self::V0Vec => cfg!(feature = "vec-ext"),
        }
    }

    /// Human-readable name, used in error messages and diagnostics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::V1Blob => "v1-blob",
            Self::V0Vec => "v0-vec",
        }
    }

    /// The SQL relation to join against for embedding **metadata**.
    ///
    /// Both layouts expose identical metadata columns (`node_id`,
    /// `model`, `created_at`), so a single query shape works against
    /// either — only the relation differs. This is the whole point of
    /// the module: one query, two layouts.
    ///
    /// - v1: the `vectors` table itself.
    /// - v0: the `vectors_meta` table.
    /// - absent: an always-empty inline relation with the same column
    ///   names, so "no vector storage" yields zero rows rather than a
    ///   `no such table` error.
    pub const fn metadata_relation(self) -> &'static str {
        match self {
            Self::Absent => "(SELECT NULL AS node_id, NULL AS model, NULL AS created_at WHERE 0)",
            Self::V1Blob => "vectors",
            Self::V0Vec => "vectors_meta",
        }
    }
}

/// Resolve the metadata table name for the open database.
///
/// Returns `None` when the project has no vector storage at all —
/// callers should treat that as "zero embeddings", not as an error.
pub fn metadata_table(conn: &Connection) -> io::Result<Option<&'static str>> {
    Ok(VectorLayout::detect(conn)?.metadata_table())
}

/// `true` when the database actually holds a `vectors_vec` `vec0`
/// virtual table.
///
/// A v0 project normally has both `vectors_vec` and `vectors_meta`,
/// but a half-migrated database can have only the metadata table. The
/// distinction matters when clearing vector storage: with no `vec0`
/// payload table there is nothing that a build without the sqlite-vec
/// module would be unable to remove.
pub fn vec0_payload_table_exists(conn: &Connection) -> io::Result<bool> {
    let mut stmt = conn
        .prepare(
            "SELECT 1 FROM sqlite_master \
             WHERE type = 'table' AND name = 'vectors_vec' \
               AND sql LIKE '%VEC0%' \
             LIMIT 1",
        )
        .map_err(io_other)?;
    let mut rows = stmt.query([]).map_err(io_other)?;
    Ok(rows.next().map_err(io_other)?.is_some())
}

/// `true` when the sqlite-vec `vec0` virtual-table module is loaded on
/// **this** connection.
///
/// This is a runtime property of the live connection, not of the
/// binary. It matters because `sqlite3_auto_extension` only fires when
/// a connection is *opened*: registering the auto-extension after a
/// connection already exists does not retroactively load `vec0` into
/// it. A connection opened before registration answers
/// `no such module: vec0` even in a `vec-ext` build.
///
/// `PRAGMA module_list` lists the virtual-table modules registered on
/// the connection, so it is the authoritative probe. It is available in
/// every SQLite build that has not opted out of the introspection
/// pragmas (`SQLITE_OMIT_INTROSPECTION_PRAGMAS`), and the bundled
/// SQLite that coraline links against does not.
pub fn vec0_module_loaded(conn: &Connection) -> io::Result<bool> {
    let mut stmt = conn.prepare("PRAGMA module_list").map_err(io_other)?;
    let mut rows = stmt.query([]).map_err(io_other)?;
    while let Some(row) = rows.next().map_err(io_other)? {
        let name: String = row.get(0).map_err(io_other)?;
        if name == "vec0" {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `true` when this connection can both read and write `vec0` payloads.
///
/// That needs two independent things to hold: the `vec0` module must
/// be loaded on the connection, and the `vectors_vec` payload table
/// must actually exist. The first is a property of the connection, the
/// second of the database file.
pub fn vec0_payload_accessible(conn: &Connection) -> io::Result<bool> {
    Ok(vec0_module_loaded(conn)? && vec0_payload_table_exists(conn)?)
}

/// Decode a `vec0` `float[]` column value into `Vec<f32>`.
///
/// sqlite-vec returns `float[]` columns as raw little-endian `f32`
/// bytes, i.e. a BLOB — *not* as the JSON text that was handed to
/// `vec_f32()` on the way in. Reading the column as text fails with
/// `InvalidColumnType`, which is what the v0 -> v1 migration used to
/// do.
///
/// A JSON-text value is still accepted, so a database written by a
/// build that stored text keeps working.
pub fn decode_vec0_embedding(raw: &rusqlite::types::Value) -> io::Result<Vec<f32>> {
    match raw {
        rusqlite::types::Value::Blob(bytes) => Ok(bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| f32::from_le_bytes(*chunk))
            .collect()),
        rusqlite::types::Value::Text(text) => Ok(parse_float_array(text)),
        other => Err(io::Error::other(format!(
            "vec0 returned unsupported embedding type: {other:?}"
        ))),
    }
}

/// Parse a JSON array of floats into a `Vec<f32>`.
///
/// Used for `vec0` columns that hold text rather than the usual
/// little-endian BLOB.
///
/// Returns an empty vector for anything that is not a well-formed
/// array — a corrupt or hand-edited database must not be able to
/// panic the process. Elements that do not parse are skipped, so the
/// result may be shorter than the array it came from; callers that
/// care should check the length.
fn parse_float_array(json: &str) -> Vec<f32> {
    let trimmed = json.trim();
    // `find`/`rfind` rather than a computed range: a string whose
    // `]` comes before its `[` would otherwise produce an
    // out-of-order slice range and panic.
    let Some(start) = trimmed.find('[') else {
        return Vec::new();
    };
    let Some(end) = trimmed.rfind(']') else {
        return Vec::new();
    };
    if end < start {
        return Vec::new();
    }
    let inner = &trimmed[start + 1..end];
    inner
        .split(',')
        .filter_map(|s| s.trim().parse::<f32>().ok())
        .collect()
}

/// Error raised when a payload operation needs the sqlite-vec module
/// that this build does not have.
///
/// The default build cannot read or write `vec0` payloads: the
/// virtual-table module is simply not registered, so SQLite answers
/// `no such module: vec0`. Silently skipping the affected nodes
/// (which is what the code did before) turns a build mismatch into a
/// no-op, so we surface it with a remediation hint instead.
pub fn vec0_unavailable_error(operation: &str) -> io::Error {
    io::Error::other(format!(
        "cannot {operation}: this project's embeddings are stored in the sqlite-vec \
         (v0) layout, but this binary was built without the `vec-ext` feature. \
         Rebuild with `--features vec-ext` to use this project, or run \
         `coraline migrate vec-ext` on a `vec-ext` build to convert it back to \
         the v1 BLOB layout."
    ))
}

/// Error raised when a payload operation needs the v1 BLOB layout but
/// the project is on the v0 `vec0` layout.
pub fn v1_unavailable_error(operation: &str) -> io::Error {
    io::Error::other(format!(
        "cannot {operation}: this project's embeddings are stored in the v1 BLOB \
         layout, which this binary was built without support for. Rebuild without \
         `--features vec-ext` to use this project."
    ))
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "test assertions: panicking on setup failure is the correct behavior"
    )]
    use super::*;

    fn in_memory() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(crate::db::SCHEMA_SQL)
            .expect("apply schema");
        conn
    }

    #[test]
    fn fresh_schema_reports_v1_blob() {
        let conn = in_memory();
        assert_eq!(
            VectorLayout::detect(&conn).expect("detect"),
            VectorLayout::V1Blob
        );
    }

    #[test]
    fn empty_database_reports_absent() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        assert_eq!(
            VectorLayout::detect(&conn).expect("detect"),
            VectorLayout::Absent
        );
    }

    #[test]
    fn metadata_table_names_match_layout() {
        assert_eq!(VectorLayout::V1Blob.metadata_table(), Some("vectors"));
        assert_eq!(VectorLayout::V0Vec.metadata_table(), Some("vectors_meta"));
        assert_eq!(VectorLayout::Absent.metadata_table(), None);
    }

    #[test]
    fn payload_table_names_match_layout() {
        assert_eq!(VectorLayout::V1Blob.payload_table(), Some("vectors"));
        assert_eq!(VectorLayout::V0Vec.payload_table(), Some("vectors_vec"));
        assert_eq!(VectorLayout::Absent.payload_table(), None);
    }

    #[test]
    fn vec0_shadow_tables_are_not_mistaken_for_v1() {
        // A vec0 virtual table creates shadow tables whose names start
        // with `vectors_v`. A wildcard `LIKE 'vectors_v%'` match would
        // pick these up; the by-name lookup must not.
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "CREATE TABLE vectors_vec_info (key text primary key, value text);
             CREATE TABLE vectors_vec_chunks (chunk_id integer primary key);
             CREATE TABLE vectors_vec_rowids (rowid integer primary key);
             CREATE TABLE vectors_vec_vector_chunks00 (rowid integer primary key);",
        )
        .expect("create shadow tables");

        // No real `vectors_vec` virtual table and no `vectors_meta`,
        // so this is not a v0 layout.
        assert_eq!(
            VectorLayout::detect(&conn).expect("detect"),
            VectorLayout::Absent
        );
    }

    #[test]
    fn vectors_meta_alone_is_enough_to_detect_v0() {
        // A half-migrated database (metadata table present, vec0
        // table missing) must still be recognised as v0 so we do not
        // try to write v1 BLOB rows into it.
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "CREATE TABLE vectors_meta (
                 rowid INTEGER PRIMARY KEY,
                 node_id TEXT UNIQUE NOT NULL,
                 model TEXT NOT NULL,
                 created_at INTEGER NOT NULL
             );",
        )
        .expect("create vectors_meta");

        assert_eq!(
            VectorLayout::detect(&conn).expect("detect"),
            VectorLayout::V0Vec
        );
    }

    #[test]
    fn as_str_is_stable() {
        assert_eq!(VectorLayout::Absent.as_str(), "absent");
        assert_eq!(VectorLayout::V1Blob.as_str(), "v1-blob");
        assert_eq!(VectorLayout::V0Vec.as_str(), "v0-vec");
    }
}
