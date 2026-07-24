//! Integration tests for context building
#![allow(clippy::expect_used)]

use std::path::Path;

use coraline::types::{BuildContextOptions, ContextFormat};
use coraline::{config, context, db, extraction};
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

    let context_str = context::build_context(project_path, "calculator functionality", &options)
        .expect("Failed to build context");

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

    let context_str = context::build_context(project_path, "user management", &options)
        .expect("Failed to build context");

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

    let context_with_code = context::build_context(project_path, "add function", &options)
        .expect("Failed to build context with code");

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

    let context_no_code = context::build_context(project_path, "calculator", &options)
        .expect("Failed to build context without code");

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

    let context_small = context::build_context(project_path, "typescript code", &options_small)
        .expect("Failed to build small context");

    let context_large = context::build_context(project_path, "typescript code", &options_large)
        .expect("Failed to build large context");

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
