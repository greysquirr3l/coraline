//! Integration tests for declaration documentation and signatures.
//!
//! `Node::docstring` and `Node::signature` are what make
//! `coraline_search_by_docstring` and `coraline_search_by_signature` work.
//! Before this module existed both fields were always `NULL`, so both
//! tools answered "no matches" no matter what the project contained.
//!
//! The tests index a fixture and assert on the extracted text, so they
//! cover the whole path: grammar → contract → extractor → database →
//! MCP tool.
#![expect(clippy::expect_used, reason = "test setup unwraps are intentional")]

use std::fs;
use std::path::Path;

use coraline::config;
use coraline::tools::Tool;
use coraline::tools::graph_tools::{SearchByDocstringTool, SearchBySignatureTool};
use coraline::{db, extraction};
use serde_json::json;

/// Index the named fixture directory into a fresh project.
///
/// Returns the temporary directory — which keeps the project on disk for
/// as long as the test runs — and the open database.
fn indexed_fixture(name: &str) -> (tempfile::TempDir, rusqlite::Connection) {
    let temp = tempfile::TempDir::new().expect("create temp dir");
    let root = temp.path().to_path_buf();

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let source = fixtures.join(name);
    copy_tree(&source, &root);

    db::initialize_database(&root).expect("initialise database");
    let cfg = config::create_default_config(&root);
    extraction::index_all(&root, &cfg, true, None).expect("index fixture");
    let conn = db::open_database(&root).expect("open database");
    (temp, conn)
}

fn copy_tree(src: &Path, dst: &Path) {
    let entries = fs::read_dir(src).expect("read fixture dir");
    for entry in entries {
        let entry = entry.expect("read fixture entry");
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            fs::create_dir_all(&target).expect("create fixture subdir");
            copy_tree(&path, &target);
        } else {
            fs::copy(&path, &target).expect("copy fixture file");
        }
    }
}

/// Return the names of the nodes in a tool's result.
fn result_names(result: &serde_json::Value) -> Vec<String> {
    result
        .get("results")
        .and_then(serde_json::Value::as_array)
        .expect("results array")
        .iter()
        .filter_map(|node| node.get("name").and_then(serde_json::Value::as_str))
        .map(ToString::to_string)
        .collect()
}

/// Return the `(docstring, signature)` of the first node named `name`.
fn doc_and_signature(conn: &rusqlite::Connection, name: &str) -> (Option<String>, Option<String>) {
    let mut stmt = conn
        .prepare("SELECT docstring, signature FROM nodes WHERE name = ?1 LIMIT 1")
        .expect("prepare query");
    let mut rows = stmt.query([name]).expect("query nodes");
    let row = rows.next().expect("query rows").expect("row for node");
    (
        row.get::<_, Option<String>>(0).expect("read docstring"),
        row.get::<_, Option<String>>(1).expect("read signature"),
    )
}

#[test]
fn rust_declarations_carry_their_doc_comment_and_header() {
    let (_temp, conn) = indexed_fixture("rust-crate");

    let (doc, sig) = doc_and_signature(&conn, "add");
    assert_eq!(doc.as_deref(), Some("Add two numbers"));
    assert_eq!(sig.as_deref(), Some("pub fn add(a: i32, b: i32) -> i32"));

    let (doc, sig) = doc_and_signature(&conn, "divide");
    assert_eq!(doc.as_deref(), Some("Divide two numbers"));
    assert_eq!(
        sig.as_deref(),
        Some("pub fn divide(a: i32, b: i32) -> Result<i32, String>")
    );
}

#[test]
fn rust_module_docs_belong_to_the_file() {
    let (_temp, conn) = indexed_fixture("rust-crate");

    let mut stmt = conn
        .prepare("SELECT docstring FROM nodes WHERE name = 'math.rs' AND kind = 'file'")
        .expect("prepare query");
    let mut rows = stmt.query([]).expect("query nodes");
    let row = rows.next().expect("query rows").expect("file row");
    let doc: Option<String> = row.get(0).expect("read docstring");
    assert_eq!(doc.as_deref(), Some("Mathematical operations"));
}

#[test]
fn python_declarations_carry_their_docstring() {
    let (_temp, conn) = indexed_fixture("python-multipackage");

    let (doc, sig) = doc_and_signature(&conn, "Runner");
    assert_eq!(doc.as_deref(), Some("Runs the app."));
    assert_eq!(sig.as_deref(), Some("class Runner:"));
}

#[test]
fn typescript_exports_carry_the_doc_comment_that_precedes_the_wrapper() {
    let (_temp, conn) = indexed_fixture("typescript-barrel");

    let mut stmt = conn
        .prepare(
            "SELECT docstring, signature FROM nodes \
             WHERE name = 'multiply' AND file_path LIKE '%math.ts' AND kind = 'function'",
        )
        .expect("prepare query");
    let mut rows = stmt.query([]).expect("query nodes");
    let row = rows.next().expect("query rows").expect("multiply row");
    let doc: Option<String> = row.get(0).expect("read docstring");
    let sig: Option<String> = row.get(1).expect("read signature");
    assert_eq!(
        doc.as_deref(),
        Some("The module the barrel re-exports from.")
    );
    assert_eq!(
        sig.as_deref(),
        Some("function multiply(a: number, b: number): number")
    );
}

#[test]
fn a_magic_comment_is_not_documentation() {
    let (_temp, conn) = indexed_fixture("ruby-multipackage");

    let mut stmt = conn
        .prepare(
            "SELECT docstring FROM nodes WHERE name = 'Helper' AND file_path LIKE '%helper.rb'",
        )
        .expect("prepare query");
    let mut rows = stmt.query([]).expect("query nodes");
    let row = rows.next().expect("query rows").expect("helper row");
    let doc: Option<String> = row.get(0).expect("read docstring");
    assert_eq!(doc.as_deref(), Some("Formats values."));
}

#[test]
fn search_by_signature_finds_declarations_by_their_header() {
    let (temp, _conn) = indexed_fixture("rust-crate");
    let tool = SearchBySignatureTool::new(temp.path().to_path_buf());

    let result = tool
        .execute(json!({ "pattern": "-> Result<i32, String>", "kind": "function" }))
        .expect("search by signature");
    let names = result_names(&result);

    assert!(
        names.contains(&"divide".to_string()),
        "expected `divide` in {names:?}"
    );
}

#[test]
fn search_by_docstring_finds_declarations_by_their_documentation() {
    let (temp, _conn) = indexed_fixture("rust-crate");
    let tool = SearchByDocstringTool::new(temp.path().to_path_buf());

    let result = tool
        .execute(json!({ "query": "two numbers", "kind": "function" }))
        .expect("search by docstring");
    let names = result_names(&result);

    assert!(
        names.contains(&"add".to_string()),
        "expected `add` in {names:?}"
    );
}

#[test]
fn a_doc_comment_on_a_nested_declaration_still_reaches_it() {
    let (_temp, conn) = indexed_fixture("zig-module");

    // `pub const Circle = struct { … }` — the struct is nested in the
    // constant that names it, so the comment precedes the constant.
    let (doc, sig) = doc_and_signature(&conn, "Circle");
    assert_eq!(doc.as_deref(), Some("A circle with a radius."));
    assert_eq!(sig.as_deref(), Some("struct {"));

    let (doc, sig) = doc_and_signature(&conn, "area");
    assert_eq!(doc.as_deref(), Some("The area of the circle."));
    assert_eq!(sig.as_deref(), Some("pub fn area(self: Circle) f64"));
}

#[test]
fn a_triple_slash_marker_is_stripped_whole() {
    let (_temp, conn) = indexed_fixture("swift-multipackage");

    let mut stmt = conn
        .prepare(
            "SELECT docstring FROM nodes \
             WHERE name = 'Circle' AND file_path LIKE '%Helper.swift'",
        )
        .expect("prepare query");
    let mut rows = stmt.query([]).expect("query nodes");
    let row = rows.next().expect("query rows").expect("circle row");
    let doc: Option<String> = row.get(0).expect("read docstring");
    assert_eq!(doc.as_deref(), Some("A circle with a radius."));
}

#[test]
fn xml_documentation_comments_keep_their_tags_but_not_their_marker() {
    let (_temp, conn) = indexed_fixture("csharp-multipackage");

    let mut stmt = conn
        .prepare(
            "SELECT docstring FROM nodes \
             WHERE name = 'IShape' AND file_path LIKE '%Shape.cs'",
        )
        .expect("prepare query");
    let mut rows = stmt.query([]).expect("query nodes");
    let row = rows.next().expect("query rows").expect("ishape row");
    let doc: Option<String> = row.get(0).expect("read docstring");
    assert_eq!(
        doc.as_deref(),
        Some("<summary>A shape with an area.</summary>")
    );
}
