//! Acceptance tests for callee precision and stale-edge hygiene.
#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};

use coraline::{config, db, extraction, tools};
use serde_json::json;
use tempfile::TempDir;

fn setup_empty_project() -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    db::initialize_database(temp_dir.path()).expect("Failed to initialize database");
    temp_dir
}

fn copy_fixture_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("Failed to create destination fixture directory");

    for entry in std::fs::read_dir(src).expect("Failed to read fixture source directory") {
        let entry = entry.expect("Failed to read fixture entry");
        let file_type = entry.file_type().expect("Failed to read fixture file type");
        let dest_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_fixture_dir(&entry.path(), &dest_path);
        } else {
            std::fs::copy(entry.path(), dest_path).expect("Failed to copy fixture file");
        }
    }
}

fn node_id_by_name_and_path(
    conn: &rusqlite::Connection,
    file_path: &str,
    symbol_name: &str,
) -> Option<String> {
    let expected_suffix = file_path.replace('\\', "/");

    db::get_all_nodes(conn)
        .ok()?
        .into_iter()
        .find(|n| {
            n.name == symbol_name && n.file_path.replace('\\', "/").ends_with(&expected_suffix)
        })
        .map(|n| n.id)
}

fn file_nodes_by_suffix(
    conn: &rusqlite::Connection,
    file_path: &str,
) -> Vec<coraline::types::Node> {
    let expected_suffix = file_path.replace('\\', "/");
    db::get_all_nodes(conn)
        .unwrap_or_default()
        .into_iter()
        .filter(|n| n.file_path.replace('\\', "/").ends_with(&expected_suffix))
        .collect()
}

fn callee_paths_for_node(project_root: &Path, node_id: &str) -> Vec<String> {
    let registry = tools::create_default_registry(project_root);
    let output = registry
        .execute(
            "coraline_callees",
            json!({
                "node_id": node_id,
                "limit": 25,
            }),
        )
        .expect("Failed to execute coraline_callees");

    let mut paths: Vec<String> = output
        .get("callees")
        .and_then(serde_json::Value::as_array)
        .expect("callees should be an array")
        .iter()
        .filter_map(|item| {
            item.get("file_path")
                .and_then(serde_json::Value::as_str)
                .map(std::string::ToString::to_string)
        })
        .collect();

    paths.sort_unstable();
    paths
}

#[test]
fn test_callees_prioritize_active_scope_over_legacy() {
    let temp = setup_empty_project();
    let project_root = temp.path();

    let fixture_root = PathBuf::from("tests/fixtures/graph_precision/active_legacy");
    copy_fixture_dir(&fixture_root, project_root);

    let cfg = config::create_default_config(project_root);
    extraction::index_all(project_root, &cfg, false, None).expect("Failed to index fixture");

    let conn = db::open_database(project_root).expect("Failed to open database");
    let run_id = node_id_by_name_and_path(&conn, "src/runtime.rs", "run")
        .expect("Expected to find run symbol");

    let callee_paths = callee_paths_for_node(project_root, &run_id);
    assert_eq!(callee_paths, vec!["src/api.rs".to_string()]);
    assert!(
        !callee_paths.iter().any(|path| path.contains("legacy/")),
        "callees should not include legacy targets when an active scoped target exists"
    );
}

#[test]
fn test_callees_avoid_unscoped_global_fallback_false_positives() {
    let temp = setup_empty_project();
    let project_root = temp.path();

    std::fs::create_dir_all(project_root.join("src")).expect("Failed to create src directory");
    std::fs::create_dir_all(project_root.join("legacy"))
        .expect("Failed to create legacy directory");

    std::fs::write(
        project_root.join("src/runtime.rs"),
        "pub fn run() {\n    post();\n}\n",
    )
    .expect("Failed to write runtime.rs");
    std::fs::write(project_root.join("legacy/api.rs"), "pub fn post() {}\n")
        .expect("Failed to write legacy/api.rs");

    let cfg = config::create_default_config(project_root);
    extraction::index_all(project_root, &cfg, false, None).expect("Failed to index fixture");

    let conn = db::open_database(project_root).expect("Failed to open database");
    let run_id = node_id_by_name_and_path(&conn, "src/runtime.rs", "run")
        .expect("Expected to find run symbol");

    let callee_paths = callee_paths_for_node(project_root, &run_id);
    assert!(
        callee_paths.is_empty(),
        "unscoped calls should remain unresolved instead of linking to legacy/global name matches"
    );
}

#[test]
fn test_stale_file_deletion_removes_call_edges_and_is_stable() {
    let temp = setup_empty_project();
    let project_root = temp.path();

    std::fs::create_dir_all(project_root.join("src")).expect("Failed to create src directory");
    std::fs::write(
        project_root.join("src/runtime.rs"),
        "pub fn run() {\n    post();\n}\n",
    )
    .expect("Failed to write runtime.rs");
    std::fs::write(project_root.join("src/api.rs"), "pub fn post() {}\n")
        .expect("Failed to write api.rs");

    let cfg = config::create_default_config(project_root);
    extraction::index_all(project_root, &cfg, false, None).expect("Failed initial index");

    let conn = db::open_database(project_root).expect("Failed to open database");
    let run_id = node_id_by_name_and_path(&conn, "src/runtime.rs", "run")
        .expect("Expected to find run symbol");

    let before = callee_paths_for_node(project_root, &run_id);
    let before_repeat = callee_paths_for_node(project_root, &run_id);
    assert_eq!(
        before, before_repeat,
        "callees output should be stable per generation"
    );
    assert_eq!(before, vec!["src/api.rs".to_string()]);

    std::fs::remove_file(project_root.join("src/api.rs")).expect("Failed to remove api.rs");
    extraction::sync(project_root, &cfg, None).expect("Failed sync after deleting api.rs");

    let conn = db::open_database(project_root).expect("Failed to reopen database");
    let api_nodes = file_nodes_by_suffix(&conn, "src/api.rs");
    assert!(api_nodes.is_empty(), "deleted file nodes must be pruned");

    let after = callee_paths_for_node(project_root, &run_id);
    let after_repeat = callee_paths_for_node(project_root, &run_id);
    assert_eq!(
        after, after_repeat,
        "callees output should remain stable after sync"
    );
    assert!(
        after.is_empty(),
        "deleted callee targets must not be returned"
    );

    let dangling_calls: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM edges e
             LEFT JOIN nodes s ON s.id = e.source
             LEFT JOIN nodes t ON t.id = e.target
             WHERE e.kind = 'calls' AND (s.id IS NULL OR t.id IS NULL)",
            [],
            |row| row.get(0),
        )
        .expect("Failed to check dangling call edges");
    assert_eq!(dangling_calls, 0, "dangling call edges should never remain");
}
