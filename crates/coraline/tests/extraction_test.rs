//! Integration tests for code extraction
#![allow(clippy::expect_used)]

use std::path::Path;

use coraline::types::{Node, Visibility};
use coraline::{config, db, extraction};
use tempfile::TempDir;

fn setup_test_db() -> (TempDir, String) {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let project_root = temp_dir
        .path()
        .to_str()
        .expect("Failed to convert path to string")
        .to_string();

    // Initialize database
    db::initialize_database(temp_dir.path()).expect("Failed to initialize database");

    (temp_dir, project_root)
}

#[test]
fn test_extract_typescript_functions() {
    let (_temp, project_root) = setup_test_db();
    let project_path = Path::new(&project_root);

    // Copy TypeScript fixture
    let fixture_src = Path::new("tests/fixtures/typescript-simple");
    let fixture_dst = project_path.join("src");
    std::fs::create_dir_all(&fixture_dst).expect("Failed to create fixture directory");

    for entry in std::fs::read_dir(fixture_src).expect("Failed to read fixture directory") {
        let entry = entry.expect("Failed to read directory entry");
        let dest = fixture_dst.join(entry.file_name());
        std::fs::copy(entry.path(), dest).expect("Failed to copy fixture file");
    }

    // Create config
    let cfg = config::create_default_config(project_path);

    // Run extraction
    let result =
        extraction::index_all(project_path, &cfg, false, None).expect("Failed to index project");

    // Verify extraction results
    assert!(result.files_indexed > 0, "Should index at least one file");
    assert!(result.nodes_created > 0, "Should create at least one node");

    // Verify extracted nodes
    let conn = db::open_database(project_path).expect("Failed to open database");

    // Should find the 'add' function
    let results =
        db::search_nodes(&conn, "add", None, 10).expect("Failed to search for 'add' function");
    assert!(!results.is_empty(), "Should find 'add' function");

    let add_node = results.iter().find(|r| r.node.name == "add");
    assert!(add_node.is_some(), "Should find exact 'add' function");

    // Should find the Calculator class
    let results = db::search_nodes(&conn, "Calculator", None, 10)
        .expect("Failed to search for Calculator class");
    assert!(!results.is_empty(), "Should find 'Calculator' class");

    // Should find the UserService class
    let results = db::search_nodes(&conn, "UserService", None, 10)
        .expect("Failed to search for UserService class");
    assert!(!results.is_empty(), "Should find 'UserService' class");
}

#[test]
fn test_extract_rust_code() {
    let (_temp, project_root) = setup_test_db();
    let project_path = Path::new(&project_root);

    // Copy Rust fixture
    let fixture_src = Path::new("tests/fixtures/rust-crate/src");
    let fixture_dst = project_path.join("src");
    std::fs::create_dir_all(&fixture_dst).expect("Failed to create fixture directory");

    for entry in std::fs::read_dir(fixture_src).expect("Failed to read fixture directory") {
        let entry = entry.expect("Failed to read directory entry");
        let dest = fixture_dst.join(entry.file_name());
        std::fs::copy(entry.path(), dest).expect("Failed to copy fixture file");
    }

    // Create config
    let cfg = config::create_default_config(project_path);

    // Run extraction
    let result = extraction::index_all(project_path, &cfg, false, None)
        .expect("Failed to index Rust project");

    // Verify extraction results
    assert!(result.files_indexed > 0, "Should index at least one file");
    assert!(result.nodes_created > 0, "Should create at least one node");

    // Verify extracted nodes
    let conn = db::open_database(project_path).expect("Failed to open database");

    // Should find the 'add' function
    let results =
        db::search_nodes(&conn, "add", None, 10).expect("Failed to search for 'add' function");
    assert!(!results.is_empty(), "Should find 'add' function");

    // Should find the Calculator struct
    let results = db::search_nodes(&conn, "Calculator", None, 10)
        .expect("Failed to search for Calculator struct");
    assert!(!results.is_empty(), "Should find 'Calculator' struct");

    // Should find the UserService struct
    let results = db::search_nodes(&conn, "UserService", None, 10)
        .expect("Failed to search for UserService struct");
    assert!(!results.is_empty(), "Should find 'UserService' struct");

    // Should find the App struct
    let results =
        db::search_nodes(&conn, "App", None, 10).expect("Failed to search for App struct");
    assert!(!results.is_empty(), "Should find 'App' struct");
}

#[test]
fn test_extract_rust_visibility_modifiers() {
    let (_temp, project_root) = setup_test_db();
    let project_path = Path::new(&project_root);

    let src_dir = project_path.join("src");
    std::fs::create_dir_all(&src_dir).expect("Failed to create src directory");
    let rust_source = "\
pub fn public_function() -> i32 {\n    42\n}\n\n\
fn private_function() -> i32 {\n    0\n}\n\n\
pub(crate) fn crate_visible_function() -> i32 {\n    1\n}\n\n\
pub(super) fn super_visible_function() -> i32 {\n    2\n}\n";
    std::fs::write(src_dir.join("lib.rs"), rust_source).expect("Failed to write lib.rs");

    let cfg = config::create_default_config(project_path);
    let _result = extraction::index_all(project_path, &cfg, false, None)
        .expect("Failed to index Rust project");

    let conn = db::open_database(project_path).expect("Failed to open database");

    // Direct SQL lookup (bypasses FTS tokenization on underscores, which
    // would split `crate_visible_function` into separate terms).
    let lookup = |name: &str| -> Option<Node> {
        let mut stmt = conn
            .prepare(
                "SELECT id, kind, name, qualified_name, file_path, language,
                        start_line, end_line, start_column, end_column,
                        docstring, signature, visibility,
                        is_exported, is_async, is_static, is_abstract,
                        decorators, type_parameters, updated_at, cluster_id
                 FROM nodes
                 WHERE name = ?1 AND kind = 'function'
                 LIMIT 1",
            )
            .expect("prepare");
        let mut rows = stmt.query(rusqlite::params![name]).expect("query");
        rows.next()
            .expect("row")
            .map(|row| db::row_to_node(row).expect("row_to_node"))
    };

    // `pub fn` → exported, Visibility::Public
    let pub_fn = lookup("public_function").expect("public_function not extracted");
    assert!(pub_fn.is_exported, "pub fn should be exported");
    assert_eq!(pub_fn.visibility, Some(Visibility::Public));

    // Private `fn` → not exported, no visibility
    let priv_fn = lookup("private_function").expect("private_function not extracted");
    assert!(!priv_fn.is_exported, "private fn should NOT be exported");
    assert_eq!(priv_fn.visibility, None);

    // `pub(crate) fn` → internal only, not exported
    let crate_fn = lookup("crate_visible_function").expect("crate_visible_function not extracted");
    assert!(
        !crate_fn.is_exported,
        "pub(crate) fn should NOT be exported (internal only)"
    );
    assert_eq!(crate_fn.visibility, Some(Visibility::Internal));

    // `pub(super) fn` → internal only, not exported
    let super_fn = lookup("super_visible_function").expect("super_visible_function not extracted");
    assert!(
        !super_fn.is_exported,
        "pub(super) fn should NOT be exported (internal only)"
    );
    assert_eq!(super_fn.visibility, Some(Visibility::Internal));
}

#[test]
fn test_incremental_sync() {
    let (_temp, project_root) = setup_test_db();
    let project_path = Path::new(&project_root);

    // Copy TypeScript fixture
    let fixture_src = Path::new("tests/fixtures/typescript-simple");
    let fixture_dst = project_path.join("src");
    std::fs::create_dir_all(&fixture_dst).expect("Failed to create fixture directory");

    for entry in std::fs::read_dir(fixture_src).expect("Failed to read fixture directory") {
        let entry = entry.expect("Failed to read directory entry");
        let dest = fixture_dst.join(entry.file_name());
        std::fs::copy(entry.path(), dest).expect("Failed to copy fixture file");
    }

    // Create config and do initial index
    let cfg = config::create_default_config(project_path);
    let initial =
        extraction::index_all(project_path, &cfg, false, None).expect("Failed to do initial index");
    assert!(initial.files_indexed > 0);

    // Sleep briefly to ensure timestamp difference
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Modify a file
    let math_file = fixture_dst.join("math.ts");
    let mut content = std::fs::read_to_string(&math_file).expect("Failed to read math.ts file");
    content.push_str("\n\nexport function power(x: number, y: number): number {\n    return Math.pow(x, y);\n}\n");
    std::fs::write(&math_file, content).expect("Failed to write modified math.ts file");

    // Run sync
    let sync_result = extraction::sync(project_path, &cfg, None).expect("Failed to sync project");

    // Should detect the modification
    assert_eq!(
        sync_result.files_modified, 1,
        "Should detect 1 modified file"
    );

    // Should find the new function
    let conn = db::open_database(project_path).expect("Failed to open database");
    let results =
        db::search_nodes(&conn, "power", None, 10).expect("Failed to search for 'power' function");
    assert!(
        !results.is_empty(),
        "Should find newly added 'power' function"
    );
}

#[test]
fn test_cross_file_references() {
    let (_temp, project_root) = setup_test_db();
    let project_path = Path::new(&project_root);

    // Copy TypeScript fixture
    let fixture_src = Path::new("tests/fixtures/typescript-simple");
    let fixture_dst = project_path.join("src");
    std::fs::create_dir_all(&fixture_dst).expect("Failed to create fixture directory");

    for entry in std::fs::read_dir(fixture_src).expect("Failed to read fixture directory") {
        let entry = entry.expect("Failed to read directory entry");
        let dest = fixture_dst.join(entry.file_name());
        std::fs::copy(entry.path(), dest).expect("Failed to copy fixture file");
    }

    // Create config and index
    let cfg = config::create_default_config(project_path);
    extraction::index_all(project_path, &cfg, false, None).expect("Failed to index project");

    // Verify cross-file imports
    let conn = db::open_database(project_path).expect("Failed to open database");

    // Check if import edges exist
    let edges: Vec<_> = conn
        .prepare("SELECT source, target FROM edges WHERE kind = 'imports'")
        .expect("Failed to prepare SQL statement")
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("Failed to query edges")
        .filter_map(Result::ok)
        .collect();

    assert!(!edges.is_empty(), "Should have import edges");
}

/// Markdown documents must produce a doc structure the audit can work with.
///
/// The Markdown extractor was dropped by the config.toml consolidation
/// (`b079ac5`), which left `coraline audit-docs` reporting zero findings
/// on every project: a Markdown file produced only a `file` node, so
/// there were no heading sections to attribute references to and no
/// `code_span` references to resolve.
#[test]
fn test_markdown_doc_structure_is_extracted() {
    let (_temp, project_root) = setup_test_db();
    let project_path = Path::new(&project_root);

    let fixture_src = Path::new("tests/fixtures/markdown-doc");
    for entry in std::fs::read_dir(fixture_src).expect("Failed to read fixture directory") {
        let entry = entry.expect("Failed to read directory entry");
        let dest = project_path.join(entry.file_name());
        if entry
            .file_type()
            .expect("Failed to read file type")
            .is_dir()
        {
            std::fs::create_dir_all(&dest).expect("Failed to create fixture directory");
            for child in std::fs::read_dir(entry.path()).expect("Failed to read dir") {
                let child = child.expect("Failed to read directory entry");
                std::fs::copy(child.path(), dest.join(child.file_name()))
                    .expect("Failed to copy fixture file");
            }
        } else {
            std::fs::copy(entry.path(), dest).expect("Failed to copy fixture file");
        }
    }

    let mut cfg = config::create_default_config(project_path);
    cfg.include = vec!["**/*.rs".to_string(), "**/*.md".to_string()];

    extraction::index_all(project_path, &cfg, false, None).expect("Failed to index project");

    let conn = db::open_database(project_path).expect("Failed to open database");

    // Every heading becomes a `module` node — this is what the audit
    // counts as a doc section.
    let sections: Vec<String> = conn
        .prepare("SELECT name FROM nodes WHERE language = 'markdown' AND kind = 'module'")
        .expect("Failed to prepare SQL statement")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("Failed to query nodes")
        .filter_map(Result::ok)
        .collect();

    assert!(
        sections.contains(&"Widget Guide".to_string()),
        "ATX heading should become a doc section, got {sections:?}"
    );
    assert!(
        sections.contains(&"Advanced".to_string()),
        "nested ATX heading should become a doc section, got {sections:?}"
    );

    // The coverage stats the audit reports come from these nodes.
    let (files, section_count) =
        db::get_doc_coverage_stats(&conn).expect("Failed to read doc coverage stats");
    assert_eq!(files, 1, "one Markdown file should be indexed");
    assert_eq!(section_count, 2, "two headings should be indexed");

    // A `code_span` that names a real symbol resolves to a `references`
    // edge; one that names nothing stays unresolved.
    let resolved: Vec<String> = conn
        .prepare(
            "SELECT t.name FROM edges e
             JOIN nodes s ON e.source = s.id
             JOIN nodes t ON e.target = t.id
             WHERE e.kind = 'references' AND s.language = 'markdown'",
        )
        .expect("Failed to prepare SQL statement")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("Failed to query edges")
        .filter_map(Result::ok)
        .collect();

    assert!(
        resolved.contains(&"Widget".to_string()),
        "`Widget` should resolve to the struct, got {resolved:?}"
    );
    assert!(
        resolved.contains(&"resize".to_string()),
        "`resize` should resolve to the method, got {resolved:?}"
    );

    let stale = db::list_doc_unresolved_refs(&conn).expect("Failed to list unresolved doc refs");
    assert!(
        stale.iter().any(|r| r.reference_name == "MissingHelper"),
        "`MissingHelper` should be reported as a stale reference"
    );

    // `Widget` is documented, so it must not be listed as undocumented.
    let undocumented =
        db::list_undocumented_exports(&conn).expect("Failed to list undocumented exports");
    assert!(
        !undocumented.iter().any(|n| n.name == "Widget"),
        "a documented export must not be reported as undocumented"
    );
}

/// Perl support rests on a grammar swap: `tree-sitter-perl` 1.1.2 pins
/// `tree-sitter` to `^0.26.3` as a runtime dependency, which makes it
/// impossible to move the workspace to `tree-sitter` 0.27 alongside it.
/// `ts-parser-perl` 2.0.0 is the maintained grammar from the
/// `tree-sitter-perl` organisation and depends on `tree-sitter` only as
/// a dev-dependency, so the two can coexist.
///
/// That swap replaces the parser implementation, so the node kinds the
/// extractor keys off have to be asserted rather than assumed. This test
/// is the first coverage Perl has had.
#[test]
fn test_extract_perl_module() {
    let (_temp, project_root) = setup_test_db();
    let project_path = Path::new(&project_root);

    let fixture_src = Path::new("tests/fixtures/perl-module");
    let fixture_dst = project_path.join("src");
    std::fs::create_dir_all(&fixture_dst).expect("Failed to create fixture directory");

    for entry in std::fs::read_dir(fixture_src).expect("Failed to read fixture directory") {
        let entry = entry.expect("Failed to read directory entry");
        let dest = fixture_dst.join(entry.file_name());
        std::fs::copy(entry.path(), dest).expect("Failed to copy fixture file");
    }

    let cfg = config::create_default_config(project_path);

    let result =
        extraction::index_all(project_path, &cfg, false, None).expect("Failed to index project");

    assert!(result.files_indexed > 0, "Should index at least one file");
    assert!(result.nodes_created > 0, "Should create at least one node");

    let conn = db::open_database(project_path).expect("Failed to open database");

    // The package name is a module node.
    let results =
        db::search_nodes(&conn, "Greeter", None, 10).expect("Failed to search for 'Greeter'");
    assert!(
        results.iter().any(|r| r.node.name == "Greeter"),
        "Should find the 'Greeter' package"
    );

    // The subroutines are functions.
    for name in ["greet", "farewell"] {
        let results =
            db::search_nodes(&conn, name, None, 10).expect("Failed to search for a subroutine");
        assert!(
            results.iter().any(|r| r.node.name == name),
            "Should find the '{name}' subroutine"
        );
    }

    // The `@EXPORT_OK` list is how a Perl module advertises its
    // interface, so `greet` and `farewell` are exported symbols. This
    // is the part of the extraction that reads an `assignment` node, and
    // it is the assertion that would catch a grammar change that
    // renames that node kind.
    let mut stmt = conn
        .prepare("SELECT name FROM nodes WHERE kind = 'export'")
        .expect("Failed to prepare the export query");
    let exported: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .expect("Failed to query the exports")
        .filter_map(Result::ok)
        .collect();
    assert!(
        exported.iter().any(|e| e == "greet"),
        "`greet` should be exported via @EXPORT_OK, got {exported:?}"
    );
    assert!(
        exported.iter().any(|e| e == "farewell"),
        "`farewell` should be exported via @EXPORT_OK, got {exported:?}"
    );
}
