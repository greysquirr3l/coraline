//! Integration tests for code extraction

use std::path::Path;

use coraline::{config, db, extraction};
use tempfile::TempDir;

fn setup_test_db() -> (TempDir, String) {
    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path().to_str().unwrap().to_string();

    // Initialize database
    db::initialize_database(temp_dir.path()).unwrap();

    (temp_dir, project_root)
}

#[test]
fn test_extract_typescript_functions() {
    let (_temp, project_root) = setup_test_db();
    let project_path = Path::new(&project_root);

    // Copy TypeScript fixture
    let fixture_src = Path::new("tests/fixtures/typescript-simple");
    let fixture_dst = project_path.join("src");
    std::fs::create_dir_all(&fixture_dst).unwrap();

    for entry in std::fs::read_dir(fixture_src).unwrap() {
        let entry = entry.unwrap();
        let dest = fixture_dst.join(entry.file_name());
        std::fs::copy(entry.path(), dest).unwrap();
    }

    // Create config
    let cfg = config::create_default_config(project_path);

    // Run extraction
    let result = extraction::index_all(project_path, &cfg, false, None).unwrap();

    // Verify extraction results
    assert!(result.files_indexed > 0, "Should index at least one file");
    assert!(result.nodes_created > 0, "Should create at least one node");

    // Verify extracted nodes
    let conn = db::open_database(project_path).unwrap();

    // Should find the 'add' function
    let results = db::search_nodes(&conn, "add", None, 10).unwrap();
    assert!(!results.is_empty(), "Should find 'add' function");

    let add_node = results.iter().find(|r| r.node.name == "add");
    assert!(add_node.is_some(), "Should find exact 'add' function");

    // Should find the Calculator class
    let results = db::search_nodes(&conn, "Calculator", None, 10).unwrap();
    assert!(!results.is_empty(), "Should find 'Calculator' class");

    // Should find the UserService class
    let results = db::search_nodes(&conn, "UserService", None, 10).unwrap();
    assert!(!results.is_empty(), "Should find 'UserService' class");
}

#[test]
fn test_extract_rust_code() {
    let (_temp, project_root) = setup_test_db();
    let project_path = Path::new(&project_root);

    // Copy Rust fixture
    let fixture_src = Path::new("tests/fixtures/rust-crate/src");
    let fixture_dst = project_path.join("src");
    std::fs::create_dir_all(&fixture_dst).unwrap();

    for entry in std::fs::read_dir(fixture_src).unwrap() {
        let entry = entry.unwrap();
        let dest = fixture_dst.join(entry.file_name());
        std::fs::copy(entry.path(), dest).unwrap();
    }

    // Create config
    let cfg = config::create_default_config(project_path);

    // Run extraction
    let result = extraction::index_all(project_path, &cfg, false, None).unwrap();

    // Verify extraction results
    assert!(result.files_indexed > 0, "Should index at least one file");
    assert!(result.nodes_created > 0, "Should create at least one node");

    // Verify extracted nodes
    let conn = db::open_database(project_path).unwrap();

    // Should find the 'add' function
    let results = db::search_nodes(&conn, "add", None, 10).unwrap();
    assert!(!results.is_empty(), "Should find 'add' function");

    // Should find the Calculator struct
    let results = db::search_nodes(&conn, "Calculator", None, 10).unwrap();
    assert!(!results.is_empty(), "Should find 'Calculator' struct");

    // Should find the UserService struct
    let results = db::search_nodes(&conn, "UserService", None, 10).unwrap();
    assert!(!results.is_empty(), "Should find 'UserService' struct");

    // Should find the App struct
    let results = db::search_nodes(&conn, "App", None, 10).unwrap();
    assert!(!results.is_empty(), "Should find 'App' struct");
}

#[test]
fn test_incremental_sync() {
    let (_temp, project_root) = setup_test_db();
    let project_path = Path::new(&project_root);

    // Copy TypeScript fixture
    let fixture_src = Path::new("tests/fixtures/typescript-simple");
    let fixture_dst = project_path.join("src");
    std::fs::create_dir_all(&fixture_dst).unwrap();

    for entry in std::fs::read_dir(fixture_src).unwrap() {
        let entry = entry.unwrap();
        let dest = fixture_dst.join(entry.file_name());
        std::fs::copy(entry.path(), dest).unwrap();
    }

    // Create config and do initial index
    let cfg = config::create_default_config(project_path);
    let initial = extraction::index_all(project_path, &cfg, false, None).unwrap();
    assert!(initial.files_indexed > 0);

    // Sleep briefly to ensure timestamp difference
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Modify a file
    let math_file = fixture_dst.join("math.ts");
    let mut content = std::fs::read_to_string(&math_file).unwrap();
    content.push_str("\n\nexport function power(x: number, y: number): number {\n    return Math.pow(x, y);\n}\n");
    std::fs::write(&math_file, content).unwrap();

    // Run sync
    let sync_result = extraction::sync(project_path, &cfg, None).unwrap();

    // Should detect the modification
    assert_eq!(
        sync_result.files_modified, 1,
        "Should detect 1 modified file"
    );

    // Should find the new function
    let conn = db::open_database(project_path).unwrap();
    let results = db::search_nodes(&conn, "power", None, 10).unwrap();
    assert!(
        !results.is_empty(),
        "Should find newly added 'power' function"
    );
}

#[test]
#[ignore = "Import edge extraction not yet fully implemented"]
fn test_cross_file_references() {
    let (_temp, project_root) = setup_test_db();
    let project_path = Path::new(&project_root);

    // Copy TypeScript fixture
    let fixture_src = Path::new("tests/fixtures/typescript-simple");
    let fixture_dst = project_path.join("src");
    std::fs::create_dir_all(&fixture_dst).unwrap();

    for entry in std::fs::read_dir(fixture_src).unwrap() {
        let entry = entry.unwrap();
        let dest = fixture_dst.join(entry.file_name());
        std::fs::copy(entry.path(), dest).unwrap();
    }

    // Create config and index
    let cfg = config::create_default_config(project_path);
    extraction::index_all(project_path, &cfg, false, None).unwrap();

    // Verify cross-file imports
    let conn = db::open_database(project_path).unwrap();

    // Check if import edges exist
    let edges: Vec<_> = conn
        .prepare("SELECT * FROM edges WHERE kind = 'imports'")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .filter_map(Result::ok)
        .collect();

    assert!(!edges.is_empty(), "Should have import edges");
}
