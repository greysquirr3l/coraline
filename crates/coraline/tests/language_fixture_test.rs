#![allow(clippy::expect_used)]

//! Per-language, multi-package fixture tests.
//!
//! Each fixture is a small project split across several directories (or
//! packages) so that cross-package references actually have to resolve. The
//! tests assert three things per language:
//!
//! 1. **Declarations** are extracted (the language's kinds are recognised).
//! 2. **Call edges** exist, including ones that cross a package boundary.
//! 3. **Import edges** exist and point at the imported module.
//!
//! The multi-package shape matters: a single-directory fixture would let a
//! name-only resolver pass, which is exactly the bug in issue #109.

use std::path::{Path, PathBuf};

use coraline::types::NodeKind;
use coraline::{config, db, extraction};
use tempfile::TempDir;

/// Copy a fixture directory into a fresh temp project and index it.
fn index_fixture(fixture: &str) -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new().expect("create temp dir");
    let project_root = temp_dir.path().to_path_buf();

    let fixture_src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    copy_tree(&fixture_src, &project_root);

    db::initialize_database(&project_root).expect("initialize database");

    let cfg = config::create_default_config(&project_root);
    let result = extraction::index_all(&project_root, &cfg, false, None).expect("index project");
    assert!(
        result.files_indexed > 0,
        "{fixture}: expected at least one file to be indexed, got {}",
        result.files_indexed
    );
    assert!(
        result.errors.is_empty(),
        "{fixture}: indexing reported errors: {:?}",
        result.errors
    );

    (temp_dir, project_root)
}

fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("create destination dir");
    for entry in std::fs::read_dir(src).expect("read fixture dir") {
        let entry = entry.expect("read dir entry");
        let target = dst.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("copy fixture file");
        }
    }
}

/// A snapshot of the graph for a fixture, for assertions.
struct Graph {
    /// Every node, as `(name, kind)` — kept paired so a name lookup can
    /// check the kind of the *same* node.
    nodes: Vec<(String, NodeKind)>,
    call_edges: Vec<(String, String)>,
    import_edges: Vec<(String, String)>,
    extends_edges: Vec<(String, String)>,
    implements_edges: Vec<(String, String)>,
    instantiates_edges: Vec<(String, String)>,
}

fn load_graph(project_root: &Path) -> Graph {
    let conn = db::open_database(project_root).expect("open database");

    let mut nodes = Vec::new();
    let mut call_edges = Vec::new();
    let mut import_edges = Vec::new();
    let mut extends_edges = Vec::new();
    let mut implements_edges = Vec::new();
    let mut instantiates_edges = Vec::new();

    let mut stmt = conn
        .prepare("SELECT id, name, kind FROM nodes")
        .expect("prepare nodes query");
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .expect("query nodes");

    let mut names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for row in rows {
        let (id, name, kind) = row.expect("read node row");
        names.insert(id, name.clone());
        if let Ok(kind) = serde_json::from_value::<NodeKind>(serde_json::Value::String(kind)) {
            nodes.push((name, kind));
        }
    }

    let mut stmt = conn
        .prepare("SELECT source, target, kind FROM edges")
        .expect("prepare edges query");
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .expect("query edges");

    for row in rows {
        let (source, target, kind) = row.expect("read edge row");
        let source_name = names.get(&source).cloned().unwrap_or_default();
        let target_name = names.get(&target).cloned().unwrap_or_default();
        match kind.as_str() {
            "calls" => call_edges.push((source_name, target_name)),
            "imports" => import_edges.push((source_name, target_name)),
            "extends" => extends_edges.push((source_name, target_name)),
            "implements" => implements_edges.push((source_name, target_name)),
            "instantiates" => instantiates_edges.push((source_name, target_name)),
            _ => {}
        }
    }

    Graph {
        nodes,
        call_edges,
        import_edges,
        extends_edges,
        implements_edges,
        instantiates_edges,
    }
}

impl Graph {
    fn has_node(&self, name: &str) -> bool {
        self.nodes.iter().any(|(n, _)| n == name)
    }

    /// Does the graph contain a node of `kind` named `name`?
    fn has_node_of_kind(&self, name: &str, kind: NodeKind) -> bool {
        self.nodes.iter().any(|(n, k)| n == name && *k == kind)
    }

    fn has_call(&self, from: &str, to: &str) -> bool {
        Self::has_edge(&self.call_edges, from, to)
    }

    /// The names of every node in the graph, for failure messages.
    fn node_names(&self) -> Vec<&str> {
        self.nodes.iter().map(|(n, _)| n.as_str()).collect()
    }

    /// The kinds of every node in the graph, for failure messages.
    fn node_kinds(&self) -> Vec<&NodeKind> {
        self.nodes.iter().map(|(_, k)| k).collect()
    }

    fn has_edge(edges: &[(String, String)], from: &str, to: &str) -> bool {
        edges.iter().any(|(s, t)| s == from && t == to)
    }
}

// ── Kotlin ───────────────────────────────────────────────────────────────────

#[test]
fn kotlin_extracts_declarations_calls_and_imports() {
    let (_temp, root) = index_fixture("kotlin-multipackage");
    let graph = load_graph(&root);

    // Declarations.
    for name in [
        "Shape",
        "BaseFigure",
        "Point",
        "Direction",
        "Formatter",
        "Circle",
        "Square",
        "run",
        "shapeArea",
        "area",
        "format",
        "describe",
        "label",
    ] {
        assert!(
            graph.has_node(name),
            "kotlin: expected node {name:?} to be extracted; got {:?}",
            graph.node_names()
        );
    }

    // Calls inside a single file.
    assert!(
        graph.has_call("label", "describe"),
        "kotlin: expected label -> describe call edge; got {:?}",
        graph.call_edges
    );

    // Cross-package calls.
    assert!(
        graph.has_call("run", "area"),
        "kotlin: expected run -> Circle.area cross-package call; got {:?}",
        graph.call_edges
    );
    assert!(
        graph.has_call("run", "shapeArea"),
        "kotlin: expected run -> shapeArea cross-package call; got {:?}",
        graph.call_edges
    );
    assert!(
        graph.has_call("run", "format"),
        "kotlin: expected run -> Formatter.format cross-package call; got {:?}",
        graph.call_edges
    );
    // Imports.
    assert!(
        !graph.import_edges.is_empty(),
        "kotlin: expected import edges, got none"
    );

    // `interface` is an anonymous keyword token in Kotlin's grammar, not a
    // node kind: it has to be discriminated from `class_declaration`.
    assert!(
        graph.has_node_of_kind("Shape", NodeKind::Interface),
        "kotlin: expected Shape to be an Interface node; kinds present: {:?}",
        graph.node_kinds()
    );
    assert!(
        graph.has_node_of_kind("BaseFigure", NodeKind::Class),
        "kotlin: expected BaseFigure to be a Class node"
    );
}

#[test]
fn kotlin_records_instantiation_edges() {
    let (_temp, root) = index_fixture("kotlin-multipackage");
    let graph = load_graph(&root);

    assert!(
        !graph.instantiates_edges.is_empty(),
        "kotlin: expected instantiation edges (Circle(2.0)); got none"
    );
}

#[test]
fn kotlin_records_heritage_edges() {
    let (_temp, root) = index_fixture("kotlin-multipackage");
    let graph = load_graph(&root);

    assert!(
        Graph::has_edge(&graph.extends_edges, "Square", "BaseFigure")
            || Graph::has_edge(&graph.implements_edges, "Square", "BaseFigure"),
        "kotlin: expected Square -> BaseFigure heritage edge; extends={:?} implements={:?}",
        graph.extends_edges,
        graph.implements_edges
    );
}

// ── Java ────────────────────────────────────────────────────────────────────

#[test]
fn java_extracts_declarations_calls_and_imports() {
    let (_temp, root) = index_fixture("java-multipackage");
    let graph = load_graph(&root);

    for name in ["Shape", "BaseFigure", "Helper", "Point", "Main", "Square"] {
        assert!(
            graph.has_node(name),
            "java: expected node {name:?} to be extracted; got {:?}",
            graph.node_names()
        );
    }

    // Cross-package calls: `run` calls `Helper.format`, which lives in a
    // different package.
    assert!(
        graph.has_call("run", "format"),
        "java: expected run -> Helper.format cross-package call; got {:?}",
        graph.call_edges
    );

    // Heritage: `Square` extends `BaseFigure`.
    assert!(
        Graph::has_edge(&graph.extends_edges, "Square", "BaseFigure"),
        "java: expected Square -> BaseFigure extends edge; got {:?}",
        graph.extends_edges
    );
    assert!(
        Graph::has_edge(&graph.implements_edges, "BaseFigure", "Shape"),
        "java: expected BaseFigure -> Shape implements edge; got {:?}",
        graph.implements_edges
    );

    assert!(
        !graph.call_edges.is_empty(),
        "java: expected call edges, got none"
    );
    assert!(
        !graph.import_edges.is_empty(),
        "java: expected import edges, got none"
    );
}

// ── Swift ───────────────────────────────────────────────────────────────────

#[test]
fn swift_extracts_declarations_calls_and_imports() {
    let (_temp, root) = index_fixture("swift-multipackage");
    let graph = load_graph(&root);

    for name in ["Shape", "Circle", "Runner"] {
        assert!(
            graph.has_node(name),
            "swift: expected node {name:?} to be extracted; got {:?}",
            graph.node_names()
        );
    }

    assert!(
        graph.has_call("run", "format"),
        "swift: expected run -> format cross-module call; got {:?}",
        graph.call_edges
    );
    assert!(
        graph.has_call("run", "area"),
        "swift: expected run -> Circle.area cross-module call; got {:?}",
        graph.call_edges
    );

    // Heritage: `BaseFigure` conforms to `Shape`.
    assert!(
        Graph::has_edge(&graph.implements_edges, "BaseFigure", "Shape"),
        "swift: expected BaseFigure -> Shape implements edge; got {:?}",
        graph.implements_edges
    );
    assert!(
        !graph.import_edges.is_empty(),
        "swift: expected import edges, got none"
    );
}

// ── Go ──────────────────────────────────────────────────────────────────────

#[test]
fn go_extracts_declarations_calls_and_imports() {
    let (_temp, root) = index_fixture("go-multipackage");
    let graph = load_graph(&root);

    for name in ["Helper", "Run"] {
        assert!(
            graph.has_node(name),
            "go: expected node {name:?} to be extracted; got {:?}",
            graph.node_names()
        );
    }

    assert!(
        graph.has_call("Run", "Area"),
        "go: expected Run -> util.Area cross-package call; got {:?}",
        graph.call_edges
    );
    assert!(
        graph.has_call("Run", "Format"),
        "go: expected Run -> util.Format cross-package call; got {:?}",
        graph.call_edges
    );
    assert!(
        !graph.import_edges.is_empty(),
        "go: expected import edges, got none"
    );
}

// ── Python ──────────────────────────────────────────────────────────────────

#[test]
fn python_extracts_declarations_calls_and_imports() {
    let (_temp, root) = index_fixture("python-multipackage");
    let graph = load_graph(&root);

    for name in ["Helper", "run"] {
        assert!(
            graph.has_node(name),
            "python: expected node {name:?} to be extracted; got {:?}",
            graph.node_names()
        );
    }

    assert!(
        graph.has_call("run", "format"),
        "python: expected run -> Helper.format cross-package call; got {:?}",
        graph.call_edges
    );
    assert!(
        graph.has_call("run", "shape_area"),
        "python: expected run -> shape_area cross-package call; got {:?}",
        graph.call_edges
    );
    assert!(
        !graph.import_edges.is_empty(),
        "python: expected import edges, got none"
    );
}

// ── Ruby ────────────────────────────────────────────────────────────────────

#[test]
fn ruby_extracts_declarations_and_calls() {
    let (_temp, root) = index_fixture("ruby-multipackage");
    let graph = load_graph(&root);

    assert!(
        graph.has_node("Helper"),
        "ruby: expected Helper to be extracted; got {:?}",
        graph.node_names()
    );

    assert!(
        graph.has_call("run", "format"),
        "ruby: expected run -> Helper.format cross-file call; got {:?}",
        graph.call_edges
    );
    assert!(
        Graph::has_edge(&graph.instantiates_edges, "run", "Helper"),
        "ruby: expected run -> Helper instantiation edge; got {:?}",
        graph.instantiates_edges
    );
}

// ── C# ──────────────────────────────────────────────────────────────────────

#[test]
fn csharp_extracts_declarations_calls_and_imports() {
    let (_temp, root) = index_fixture("csharp-multipackage");
    let graph = load_graph(&root);

    for name in ["IShape", "BaseFigure", "Circle", "Helper", "Runner"] {
        assert!(
            graph.has_node(name),
            "csharp: expected node {name:?} to be extracted; got {:?}",
            graph.node_names()
        );
    }

    // Cross-package calls: `Run` calls `Helper.Format`.
    assert!(
        graph.has_call("Run", "Format"),
        "csharp: expected Run -> Helper.Format cross-package call; got {:?}",
        graph.call_edges
    );

    // Heritage: `BaseFigure` implements `IShape`.
    assert!(
        Graph::has_edge(&graph.implements_edges, "BaseFigure", "IShape"),
        "csharp: expected BaseFigure -> IShape implements edge; got {:?}",
        graph.implements_edges
    );

    assert!(
        !graph.call_edges.is_empty(),
        "csharp: expected call edges, got none"
    );
    assert!(
        !graph.import_edges.is_empty(),
        "csharp: expected import edges, got none"
    );
}
