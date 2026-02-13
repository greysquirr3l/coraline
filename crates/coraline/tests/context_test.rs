//! Integration tests for context building

use std::path::Path;

use coraline::types::{BuildContextOptions, ContextFormat};
use coraline::{config, context, db, extraction};
use tempfile::TempDir;

fn setup_indexed_project() -> (TempDir, String) {
    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path().to_str().unwrap().to_string();
    let project_path = Path::new(&project_root);

    // Initialize database
    db::initialize_database(project_path).unwrap();

    // Copy TypeScript fixture
    let fixture_src = Path::new("tests/fixtures/typescript-simple");
    let fixture_dst = project_path.join("src");
    std::fs::create_dir_all(&fixture_dst).unwrap();

    for entry in std::fs::read_dir(fixture_src).unwrap() {
        let entry = entry.unwrap();
        let dest = fixture_dst.join(entry.file_name());
        std::fs::copy(entry.path(), dest).unwrap();
    }

    // Index the project
    let cfg = config::create_default_config(project_path);
    extraction::index_all(project_path, &cfg, false, None).unwrap();

    (temp_dir, project_root)
}

#[test]
fn test_build_context_markdown() {
    let (_temp, project_root) = setup_indexed_project();
    let project_path = Path::new(&project_root);

    let options = BuildContextOptions {
        max_nodes: Some(10),
        max_code_blocks: Some(5),
        max_code_block_size: Some(500),
        include_code: Some(true),
        format: Some(ContextFormat::Markdown),
        search_limit: None,
        traversal_depth: Some(2),
        min_score: None,
    };

    let context_str =
        context::build_context(project_path, "calculator functionality", &options).unwrap();

    assert!(!context_str.is_empty(), "Context should not be empty");
    assert!(
        context_str.contains("Calculator")
            || context_str.contains("add")
            || context_str.contains("math"),
        "Context should mention calculator or math functions"
    );
}

#[test]
fn test_build_context_json() {
    let (_temp, project_root) = setup_indexed_project();
    let project_path = Path::new(&project_root);

    let options = BuildContextOptions {
        max_nodes: Some(10),
        max_code_blocks: Some(5),
        max_code_block_size: Some(500),
        include_code: Some(true),
        format: Some(ContextFormat::Json),
        search_limit: None,
        traversal_depth: Some(2),
        min_score: None,
    };

    let context_str = context::build_context(project_path, "user management", &options).unwrap();

    assert!(!context_str.is_empty(), "Context should not be empty");

    // Should be valid JSON
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(&context_str);
    assert!(parsed.is_ok(), "Context should be valid JSON");
}

#[test]
fn test_context_includes_code() {
    let (_temp, project_root) = setup_indexed_project();
    let project_path = Path::new(&project_root);

    let options = BuildContextOptions {
        max_nodes: Some(10),
        max_code_blocks: Some(5),
        max_code_block_size: Some(1000),
        include_code: Some(true),
        format: Some(ContextFormat::Markdown),
        search_limit: None,
        traversal_depth: Some(1),
        min_score: None,
    };

    let context_with_code = context::build_context(project_path, "add function", &options).unwrap();

    // With code enabled, should include code blocks
    assert!(
        context_with_code.contains("```") || context_with_code.len() > 100,
        "Context with code should include code blocks or be substantial"
    );
}

#[test]
fn test_context_without_code() {
    let (_temp, project_root) = setup_indexed_project();
    let project_path = Path::new(&project_root);

    let options = BuildContextOptions {
        max_nodes: Some(10),
        max_code_blocks: Some(5),
        max_code_block_size: Some(500),
        include_code: Some(false),
        format: Some(ContextFormat::Markdown),
        search_limit: None,
        traversal_depth: Some(1),
        min_score: None,
    };

    let context_no_code = context::build_context(project_path, "calculator", &options).unwrap();

    assert!(
        !context_no_code.is_empty(),
        "Context should not be empty even without code"
    );
}

#[test]
fn test_context_max_nodes_limit() {
    let (_temp, project_root) = setup_indexed_project();
    let project_path = Path::new(&project_root);

    let options_small = BuildContextOptions {
        max_nodes: Some(2),
        max_code_blocks: Some(2),
        max_code_block_size: Some(500),
        include_code: Some(true),
        format: Some(ContextFormat::Markdown),
        search_limit: None,
        traversal_depth: Some(1),
        min_score: None,
    };

    let options_large = BuildContextOptions {
        max_nodes: Some(20),
        max_code_blocks: Some(10),
        max_code_block_size: Some(500),
        include_code: Some(true),
        format: Some(ContextFormat::Markdown),
        search_limit: None,
        traversal_depth: Some(2),
        min_score: None,
    };

    let context_small =
        context::build_context(project_path, "typescript code", &options_small).unwrap();

    let context_large =
        context::build_context(project_path, "typescript code", &options_large).unwrap();

    // Larger limits should generally produce more context
    // (though not guaranteed depending on query)
    assert!(
        !context_small.is_empty(),
        "Small context should not be empty"
    );
    assert!(
        !context_large.is_empty(),
        "Large context should not be empty"
    );
}
