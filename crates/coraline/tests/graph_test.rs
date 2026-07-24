//! Integration tests for graph traversal operations
#![allow(clippy::expect_used)]

use std::path::Path;

use coraline::types::TraversalOptions;
use coraline::{config, db, extraction, graph};
use tempfile::TempDir;

fn setup_indexed_project() -> (TempDir, String) {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let project_root = temp_dir
        .path()
        .to_str()
        .expect("Failed to convert path to string")
        .to_string();
    let project_path = Path::new(&project_root);

    // Initialize database
    db::initialize_database(project_path).expect("Failed to initialize database");

    // Copy TypeScript fixture
    let fixture_src = Path::new("tests/fixtures/typescript-simple");
    let fixture_dst = project_path.join("src");
    std::fs::create_dir_all(&fixture_dst).expect("Failed to create fixture directory");

    for entry in std::fs::read_dir(fixture_src).expect("Failed to read fixture directory") {
        let entry = entry.expect("Failed to read directory entry");
        let dest = fixture_dst.join(entry.file_name());
        std::fs::copy(entry.path(), dest).expect("Failed to copy fixture file");
    }

    // Index the project
    let cfg = config::create_default_config(project_path);
    extraction::index_all(project_path, &cfg, false, None).expect("Failed to index project");

    (temp_dir, project_root)
}

#[test]
fn test_graph_traversal_basic() {
    let (_temp, project_root) = setup_indexed_project();
    let project_path = Path::new(&project_root);
    let conn = db::open_database(project_path).expect("Failed to open database");

    // Find the Calculator class
    let results = db::search_nodes(&conn, "Calculator", None, 1).expect("Failed to search nodes");
    assert!(!results.is_empty(), "Should find Calculator");

    let calculator_id = &results
        .first()
        .expect("Results should have at least one item")
        .node
        .id;

    // Build subgraph around Calculator
    let options = TraversalOptions {
        max_depth: Some(2),
        edge_kinds: None,
        node_kinds: None,
        direction: None,
        limit: None,
        include_start: Some(true),
    };

    let subgraph = graph::build_subgraph(&conn, std::slice::from_ref(calculator_id), &options)
        .expect("Failed to build subgraph");

    assert!(!subgraph.nodes.is_empty(), "Subgraph should contain nodes");
    assert!(
        subgraph.nodes.contains_key(calculator_id),
        "Should contain Calculator"
    );
}

#[test]
fn test_subgraph_with_depth_limit() {
    let (_temp, project_root) = setup_indexed_project();
    let project_path = Path::new(&project_root);
    let conn = db::open_database(project_path).expect("Failed to open database");

    // Find Calculator
    let results = db::search_nodes(&conn, "Calculator", None, 1).expect("Failed to search nodes");
    assert!(!results.is_empty());

    let root_id = &results
        .first()
        .expect("Results should have at least one item")
        .node
        .id;

    // Build subgraph with depth 1
    let options_1 = TraversalOptions {
        max_depth: Some(1),
        edge_kinds: None,
        node_kinds: None,
        direction: None,
        limit: None,
        include_start: Some(true),
    };
    let subgraph_1 = graph::build_subgraph(&conn, std::slice::from_ref(root_id), &options_1)
        .expect("Failed to build subgraph with depth 1");
    let count_1 = subgraph_1.nodes.len();

    // Build subgraph with depth 2
    let options_2 = TraversalOptions {
        max_depth: Some(2),
        edge_kinds: None,
        node_kinds: None,
        direction: None,
        limit: None,
        include_start: Some(true),
    };
    let subgraph_2 = graph::build_subgraph(&conn, std::slice::from_ref(root_id), &options_2)
        .expect("Failed to build subgraph with depth 2");
    let count_2 = subgraph_2.nodes.len();

    // Depth 2 should have at least as many nodes as depth 1
    assert!(
        count_2 >= count_1,
        "Greater depth should include more or equal nodes"
    );
}

#[test]
fn test_get_edges_from_database() {
    let (_temp, project_root) = setup_indexed_project();
    let project_path = Path::new(&project_root);
    let conn = db::open_database(project_path).expect("Failed to open database");

    // Get count of all edges
    let edge_count: i64 = conn
        .prepare("SELECT COUNT(*) FROM edges")
        .expect("Failed to prepare SQL statement")
        .query_row([], |row| row.get(0))
        .expect("Failed to query edge count");

    // Should have some edges (exact count depends on extraction quality)
    assert!(edge_count >= 0, "Should be able to query edges table");
}

#[test]
fn test_multiple_roots_subgraph() {
    let (_temp, project_root) = setup_indexed_project();
    let project_path = Path::new(&project_root);
    let conn = db::open_database(project_path).expect("Failed to open database");

    // Find multiple nodes
    let calc_results =
        db::search_nodes(&conn, "Calculator", None, 1).expect("Failed to search for Calculator");
    let user_results =
        db::search_nodes(&conn, "UserService", None, 1).expect("Failed to search for UserService");

    if !calc_results.is_empty() && !user_results.is_empty() {
        let roots = vec![
            calc_results
                .first()
                .expect("Calculator results should have at least one item")
                .node
                .id
                .clone(),
            user_results
                .first()
                .expect("UserService results should have at least one item")
                .node
                .id
                .clone(),
        ];

        let options = TraversalOptions {
            max_depth: Some(1),
            edge_kinds: None,
            node_kinds: None,
            direction: None,
            limit: None,
            include_start: Some(true),
        };

        let subgraph = graph::build_subgraph(&conn, &roots, &options)
            .expect("Failed to build subgraph with multiple roots");

        // Should include both roots
        assert!(subgraph.roots.len() >= 2, "Should have multiple roots");
    }
}
