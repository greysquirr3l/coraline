//! Benchmarks for the core indexing and query pipeline.
//!
//! Run with:
//!   cargo bench --bench indexing
//!
//! Or a specific group:
//!   cargo bench --bench indexing -- search

#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use std::path::Path;

use coraline::types::{
    BuildContextOptions, ContextFormat, EdgeKind, TraversalDirection, TraversalOptions,
};
use coraline::{config, context, db, extraction, graph};
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Copy all files in `src_dir` (non-recursively) into `dst_dir`.
fn copy_fixture(src_dir: &Path, dst_dir: &Path) {
    for entry in std::fs::read_dir(src_dir).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            std::fs::copy(entry.path(), dst_dir.join(entry.file_name())).unwrap();
        }
    }
}

/// Build a temp project indexed from the TypeScript fixture.
/// Returns `(TempDir, project_root_path_buf)` — caller must keep `TempDir` alive.
fn setup_ts_project() -> (TempDir, std::path::PathBuf) {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    db::initialize_database(&root).unwrap();

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    copy_fixture(Path::new("tests/fixtures/typescript-simple"), &src);

    let cfg = config::create_default_config(&root);
    extraction::index_all(&root, &cfg, false, None).unwrap();

    (temp, root)
}

// ---------------------------------------------------------------------------
// Indexing benchmarks
// ---------------------------------------------------------------------------

fn bench_indexing(c: &mut Criterion) {
    let mut group = c.benchmark_group("indexing");

    group.bench_function("full_index_typescript", |b| {
        b.iter_with_setup(
            || {
                let temp = TempDir::new().unwrap();
                let root = temp.path().to_path_buf();
                db::initialize_database(&root).unwrap();
                let src = root.join("src");
                std::fs::create_dir_all(&src).unwrap();
                copy_fixture(Path::new("tests/fixtures/typescript-simple"), &src);
                (temp, root)
            },
            |setup| {
                let (temp, root) = setup;
                let cfg = config::create_default_config(&root);
                let result = extraction::index_all(&root, &cfg, false, None).unwrap();
                drop(temp);
                result
            },
        );
    });

    group.bench_function("full_index_rust", |b| {
        b.iter_with_setup(
            || {
                let temp = TempDir::new().unwrap();
                let root = temp.path().to_path_buf();
                db::initialize_database(&root).unwrap();
                let src = root.join("src");
                std::fs::create_dir_all(&src).unwrap();
                copy_fixture(Path::new("tests/fixtures/rust-crate/src"), &src);
                (temp, root)
            },
            |setup| {
                let (temp, root) = setup;
                let cfg = config::create_default_config(&root);
                let result = extraction::index_all(&root, &cfg, false, None).unwrap();
                drop(temp);
                result
            },
        );
    });

    group.bench_function("incremental_sync_no_changes", |b| {
        b.iter_with_setup(
            || {
                let temp = TempDir::new().unwrap();
                let root = temp.path().to_path_buf();
                db::initialize_database(&root).unwrap();
                let src = root.join("src");
                std::fs::create_dir_all(&src).unwrap();
                copy_fixture(Path::new("tests/fixtures/typescript-simple"), &src);
                let cfg = config::create_default_config(&root);
                extraction::index_all(&root, &cfg, false, None).unwrap();
                (temp, root, cfg)
            },
            |setup| {
                let (temp, root, cfg) = setup;
                let result = extraction::sync(&root, &cfg, None).unwrap();
                drop(temp);
                result
            },
        );
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Search / query benchmarks
// ---------------------------------------------------------------------------

fn bench_search(c: &mut Criterion) {
    let (_temp, root) = setup_ts_project();
    let conn = db::open_database(&root).unwrap();

    let mut group = c.benchmark_group("search");

    for query in &["add", "Calculator", "User", "calculate"] {
        group.bench_with_input(BenchmarkId::new("fts", query), query, |b, q| {
            b.iter(|| db::search_nodes(&conn, q, None, 20).unwrap());
        });
    }

    group.bench_function("find_by_exact_name", |b| {
        b.iter(|| db::find_nodes_by_name(&conn, "add").unwrap());
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Graph traversal benchmarks
// ---------------------------------------------------------------------------

fn bench_graph(c: &mut Criterion) {
    let (_temp, root) = setup_ts_project();
    let conn = db::open_database(&root).unwrap();

    // Pick any function node to traverse from.
    let start_id = db::search_nodes(&conn, "add", None, 1)
        .unwrap()
        .into_iter()
        .next()
        .map(|r| r.node.id)
        .unwrap_or_default();

    let mut group = c.benchmark_group("graph");

    group.bench_function("subgraph_calls_outgoing_depth3", |b| {
        b.iter(|| {
            graph::build_subgraph(
                &conn,
                &[start_id.clone()],
                &TraversalOptions {
                    max_depth: Some(3),
                    edge_kinds: Some(vec![EdgeKind::Calls]),
                    node_kinds: None,
                    direction: Some(TraversalDirection::Outgoing),
                    limit: Some(50),
                    include_start: Some(true),
                },
            )
            .unwrap()
        });
    });

    group.bench_function("subgraph_both_depth2", |b| {
        b.iter(|| {
            graph::build_subgraph(
                &conn,
                &[start_id.clone()],
                &TraversalOptions {
                    max_depth: Some(2),
                    edge_kinds: None,
                    node_kinds: None,
                    direction: Some(TraversalDirection::Both),
                    limit: Some(100),
                    include_start: Some(true),
                },
            )
            .unwrap()
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Context building benchmarks
// ---------------------------------------------------------------------------

fn bench_context(c: &mut Criterion) {
    let (_temp, root) = setup_ts_project();

    let mut group = c.benchmark_group("context");

    group.bench_function("build_context_markdown", |b| {
        b.iter(|| {
            context::build_context(
                &root,
                "add two numbers",
                &BuildContextOptions {
                    max_nodes: Some(10),
                    max_code_blocks: Some(3),
                    max_code_block_size: Some(500),
                    traversal_depth: Some(1),
                    include_code: Some(false),
                    format: Some(ContextFormat::Markdown),
                    search_limit: None,
                    min_score: None,
                },
            )
            .unwrap()
        });
    });

    group.bench_function("build_context_with_code", |b| {
        b.iter(|| {
            context::build_context(
                &root,
                "Calculator",
                &BuildContextOptions {
                    max_nodes: Some(15),
                    max_code_blocks: Some(5),
                    max_code_block_size: Some(800),
                    traversal_depth: Some(2),
                    include_code: Some(true),
                    format: Some(ContextFormat::Markdown),
                    search_limit: None,
                    min_score: None,
                },
            )
            .unwrap()
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Criterion entry point
// ---------------------------------------------------------------------------

criterion_group!(
    bench_all,
    bench_indexing,
    bench_search,
    bench_graph,
    bench_context,
);
criterion_main!(bench_all);
