#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::db;
use crate::graph;
use crate::types::{
    BuildContextOptions, CodeBlock, ContextFormat, ContextStats, EdgeKind, SearchResult, Subgraph,
    TaskContext, TraversalDirection, TraversalOptions,
};

#[derive(Debug, Default)]
pub struct ContextBuilder;

pub fn build_context(
    project_root: &Path,
    task: &str,
    options: &BuildContextOptions,
) -> std::io::Result<String> {
    let conn = db::open_database(project_root)?;
    let max_nodes = options.max_nodes.unwrap_or(20);
    let max_code_blocks = options.max_code_blocks.unwrap_or(5);
    let max_code_block_size = options.max_code_block_size.unwrap_or(1500);
    let include_code = options.include_code.unwrap_or(true);
    let format = options.format.unwrap_or(ContextFormat::Markdown);

    let results = db::search_nodes(&conn, task, None, max_nodes)?;
    let entry_points: Vec<_> = results.iter().map(|r| r.node.clone()).collect();
    let traversal = TraversalOptions {
        max_depth: options.traversal_depth.or(Some(1)),
        edge_kinds: Some(vec![EdgeKind::Contains, EdgeKind::Calls]),
        node_kinds: None,
        direction: Some(TraversalDirection::Both),
        limit: Some(max_nodes.saturating_mul(4)),
        include_start: Some(true),
    };

    let subgraph = graph::build_subgraph(
        &conn,
        &entry_points
            .iter()
            .map(|n| n.id.clone())
            .collect::<Vec<_>>(),
        &traversal,
    )
    .unwrap_or_else(|_| Subgraph {
        nodes: entry_points
            .iter()
            .map(|node| (node.id.clone(), node.clone()))
            .collect::<HashMap<_, _>>(),
        edges: Vec::new(),
        roots: entry_points.iter().map(|n| n.id.clone()).collect(),
    });

    let code_blocks = if include_code {
        extract_code_blocks(project_root, &results, max_code_blocks, max_code_block_size)
    } else {
        Vec::new()
    };

    let related_files = subgraph
        .nodes
        .values()
        .map(|node| node.file_path.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    let summary = format!(
        "Found {} relevant symbols across {} files.",
        entry_points.len(),
        related_files.len()
    );

    let stats = ContextStats {
        node_count: subgraph.nodes.len(),
        edge_count: subgraph.edges.len(),
        file_count: related_files.len(),
        code_block_count: code_blocks.len(),
        total_code_size: code_blocks.iter().map(|b| b.content.len()).sum(),
    };

    let context = TaskContext {
        query: task.to_string(),
        subgraph,
        entry_points,
        code_blocks,
        related_files,
        summary,
        stats,
    };

    Ok(match format {
        ContextFormat::Markdown => format_context_markdown(&context),
        ContextFormat::Json => serde_json::to_string_pretty(&context).unwrap_or_default(),
    })
}

fn extract_code_blocks(
    project_root: &Path,
    results: &[SearchResult],
    max_blocks: usize,
    max_block_size: usize,
) -> Vec<CodeBlock> {
    let mut blocks = Vec::new();

    for result in results.iter().take(max_blocks) {
        let node = &result.node;
        let file_path = project_root.join(&node.file_path);
        let Ok(content) = fs::read_to_string(&file_path) else {
            continue;
        };

        let lines: Vec<&str> = content.lines().collect();
        let start_idx = usize::try_from(node.start_line.saturating_sub(1)).unwrap_or(0);
        let max_end = i64::try_from(lines.len()).unwrap_or(i64::MAX);
        let end_idx = usize::try_from(node.end_line.min(max_end)).unwrap_or(lines.len());
        let slice = lines
            .get(start_idx..end_idx)
            .map_or_else(String::new, |slice| slice.join("\n"));

        let truncated = if slice.len() > max_block_size {
            let prefix = slice.get(..max_block_size).unwrap_or(&slice);
            format!("{prefix}\n// ... truncated ...")
        } else {
            slice
        };

        blocks.push(CodeBlock {
            content: truncated,
            file_path: node.file_path.clone(),
            start_line: node.start_line,
            end_line: node.end_line,
            language: node.language,
            node: Some(node.clone()),
        });
    }

    blocks
}

fn format_context_markdown(context: &TaskContext) -> String {
    let mut lines = Vec::new();
    lines.push("## Code Context".to_string());
    lines.push(String::new());
    lines.push(format!("**Query:** {}", context.query));
    lines.push(String::new());

    if !context.entry_points.is_empty() {
        lines.push("### Entry Points".to_string());
        lines.push(String::new());
        for node in &context.entry_points {
            lines.push(format!(
                "- **{}** ({:?}) - {}:{}",
                node.name, node.kind, node.file_path, node.start_line
            ));
        }
        lines.push(String::new());
    }

    if !context.code_blocks.is_empty() {
        lines.push("### Code".to_string());
        lines.push(String::new());
        for block in &context.code_blocks {
            let header = block.node.as_ref().map_or_else(
                || block.file_path.clone(),
                |n| format!("{} ({})", n.name, block.file_path),
            );
            lines.push(format!("#### {header}"));
            lines.push(String::new());
            lines.push(format!("```{:?}", block.language));
            lines.push(block.content.clone());
            lines.push("```".to_string());
            lines.push(String::new());
        }
    }

    lines.join("\n")
}
