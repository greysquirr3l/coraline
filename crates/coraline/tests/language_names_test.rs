#![allow(clippy::expect_used)]

//! One name per language, everywhere.
//!
//! A language's name is a public contract: it is what the MCP tools
//! report, what the database stores, and what a user types to ask for
//! a language by name. It used to have three spellings at once —
//!
//! - `Language::name` did not exist;
//! - `tools::language_to_string` said `typescript`;
//! - `db::language_to_string` went through serde, which said
//!   `type_script`.
//!
//! So `coraline_search` reported `javascript` for a node that
//! `coraline_node` reported as `java_script`, and the database stored
//! the serde spelling. These tests pin the single canonical spelling
//! and check that the older one still reads back, so an index written
//! by an earlier version is not silently mislabelled.

use coraline::db;
use coraline::types::{ALL_LANGUAGES, Language};
use rusqlite::Connection;

/// The canonical names, pinned.
///
/// These are the names the MCP tools report and the database stores.
/// Changing one changes a published contract, so it has to be a
/// deliberate act and not a side effect of renaming a variant.
const CANONICAL_NAMES: &str = "\
    bash blazor c cpp csharp dart elixir elm erlang fortran go groovy haskell \
    java javascript jsx julia kotlin liquid lua markdown matlab nix perl php \
    powershell python r ruby rust scala swift toml tsx typescript yaml zig";

/// The names older indexes were written with, and the languages they
/// stand for.
const LEGACY_NAMES: &[(&str, Language)] = &[
    ("type_script", Language::TypeScript),
    ("java_script", Language::JavaScript),
    ("c_sharp", Language::CSharp),
];

#[test]
fn the_canonical_names_are_pinned() {
    let mut actual: Vec<&str> = ALL_LANGUAGES
        .iter()
        .map(|language| language.name())
        .collect();
    let mut expected: Vec<&str> = CANONICAL_NAMES.split_whitespace().collect();
    actual.sort_unstable();
    expected.sort_unstable();

    assert_eq!(
        actual, expected,
        "the canonical language names changed; update CANONICAL_NAMES and the \
         documentation if that is intended"
    );
}

#[test]
fn the_unknown_catch_all_is_named() {
    assert_eq!(Language::Unknown.name(), "unknown");
}

#[test]
fn every_language_is_reachable_by_name() {
    for language in ALL_LANGUAGES {
        let name = language.name();
        assert!(!name.is_empty(), "{language:?} has an empty name");
        assert_eq!(
            Language::from_name(name),
            *language,
            "{name} does not resolve back to {language:?}"
        );
    }
}

#[test]
fn the_name_lookup_ignores_case() {
    for language in ALL_LANGUAGES {
        let shouted = language.name().to_uppercase();
        assert_eq!(
            Language::from_name(&shouted),
            *language,
            "{shouted} does not resolve to {language:?}"
        );
    }
}

#[test]
fn an_unknown_name_is_unknown() {
    assert_eq!(Language::from_name("not-a-language"), Language::Unknown);
    assert_eq!(Language::from_name(""), Language::Unknown);
}

#[test]
fn the_legacy_names_still_resolve() {
    for (name, language) in LEGACY_NAMES {
        assert_eq!(
            Language::from_name(name),
            *language,
            "the legacy name {name} no longer resolves"
        );
    }
}

#[test]
fn a_language_serialises_to_its_canonical_name() {
    for language in ALL_LANGUAGES {
        let json = serde_json::to_string(language).expect("serialise language");
        assert_eq!(
            json,
            format!("\"{}\"", language.name()),
            "{language:?} does not serialise to its canonical name"
        );
    }
}

#[test]
fn a_language_deserialises_from_either_spelling() {
    for language in ALL_LANGUAGES {
        let from_canonical = serde_json::from_str::<Language>(&format!("\"{}\"", language.name()))
            .expect("deserialise the canonical name");
        assert_eq!(from_canonical, *language);
    }

    for (name, language) in LEGACY_NAMES {
        let from_legacy = serde_json::from_str::<Language>(&format!("\"{name}\""))
            .expect("deserialise the legacy name");
        assert_eq!(from_legacy, *language);
    }
}

#[test]
fn an_unknown_serialised_name_is_rejected() {
    assert!(serde_json::from_str::<Language>("\"klingon\"").is_err());
}

/// The database stores the canonical name, so a fresh index agrees with
/// what the tools report.
#[test]
fn the_database_stores_the_canonical_name() {
    let conn = Connection::open_in_memory().expect("open in-memory database");
    conn.execute_batch(db::SCHEMA_SQL).expect("apply schema");
    db::apply_incremental_migrations(&conn).expect("apply migrations");

    conn.execute(
        "INSERT INTO nodes (id, kind, name, qualified_name, file_path, language,
            start_line, end_line, start_column, end_column, updated_at)
         VALUES ('n1', 'function', 'f', 'f', 'a.ts', ?1, 1, 1, 0, 0, 0)",
        rusqlite::params![db::language_to_string(Language::TypeScript)],
    )
    .expect("insert node");

    let stored: String = conn
        .query_row("SELECT language FROM nodes WHERE id = 'n1'", [], |row| {
            row.get(0)
        })
        .expect("read the stored language");
    assert_eq!(stored, "typescript");
}

/// An index written before the names were unified still reads back with
/// the right language, rather than `Unknown`.
#[test]
fn a_legacy_row_still_reads_back_as_its_language() {
    let conn = Connection::open_in_memory().expect("open in-memory database");
    conn.execute_batch(db::SCHEMA_SQL).expect("apply schema");
    db::apply_incremental_migrations(&conn).expect("apply migrations");

    for (index, (name, language)) in LEGACY_NAMES.iter().enumerate() {
        let id = format!("n{index}");
        conn.execute(
            "INSERT INTO nodes (id, kind, name, qualified_name, file_path, language,
                start_line, end_line, start_column, end_column, updated_at)
             VALUES (?1, 'function', 'f', 'f', 'a.js', ?2, 1, 1, 0, 0, 0)",
            rusqlite::params![id, name],
        )
        .expect("insert node");

        let node = conn
            .query_row(
                "SELECT id, kind, name, qualified_name, file_path, language,
                    start_line, end_line, start_column, end_column, docstring,
                    signature, visibility, is_exported, is_async, is_static,
                    is_abstract, decorators, type_parameters, updated_at
                 FROM nodes WHERE id = ?1",
                rusqlite::params![id],
                db::row_to_node,
            )
            .expect("build the node");
        assert_eq!(
            node.language, *language,
            "{name} did not read back correctly"
        );
    }
}
