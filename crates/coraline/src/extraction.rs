#![forbid(unsafe_code)]
#![allow(
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::collapsible_if,
    clippy::equatable_if_let,
    clippy::indexing_slicing,
    clippy::manual_let_else,
    clippy::match_same_arms,
    clippy::missing_const_for_fn,
    clippy::needless_pass_by_value,
    clippy::option_if_let_else,
    clippy::redundant_clone,
    clippy::redundant_closure_for_method_calls,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::uninlined_format_args,
    clippy::used_underscore_binding
)]

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use tree_sitter::{Node as TsNode, Parser};

use crate::config::is_language_supported;
use crate::db;
use crate::resolution::ReferenceResolver;
use crate::types::{
    CodeGraphConfig, Edge, EdgeKind, ExtractionError, ExtractionErrorSeverity, FileRecord,
    Language, Node, NodeKind, UnresolvedReference,
};
use crate::utils::{hash_sha256, node_id_for_symbol};

#[derive(Debug, Clone, Copy)]
pub enum IndexPhase {
    Scanning,
    Parsing,
    Storing,
    Resolving,
}

#[derive(Debug, Clone)]
pub struct IndexProgress {
    pub phase: IndexPhase,
    pub current: usize,
    pub total: usize,
    pub current_file: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IndexResult {
    pub success: bool,
    pub files_indexed: usize,
    pub files_skipped: usize,
    pub nodes_created: usize,
    pub edges_created: usize,
    pub errors: Vec<ExtractionError>,
    pub duration_ms: u128,
}

#[derive(Debug, Clone)]
pub struct SyncResult {
    pub files_checked: usize,
    pub files_added: usize,
    pub files_modified: usize,
    pub files_removed: usize,
    pub nodes_updated: usize,
    pub duration_ms: u128,
}

pub fn index_all(
    project_root: &Path,
    config: &CodeGraphConfig,
    force: bool,
    on_progress: Option<&dyn Fn(IndexProgress)>,
) -> std::io::Result<IndexResult> {
    let start = Instant::now();
    let mut errors = Vec::new();
    let mut files_indexed = 0;
    let mut files_skipped = 0;
    let mut nodes_created = 0;
    let mut edges_created = 0;

    let files = scan_directory(project_root, config, |current, file| {
        if let Some(cb) = on_progress {
            cb(IndexProgress {
                phase: IndexPhase::Scanning,
                current,
                total: 0,
                current_file: Some(file.to_string()),
            });
        }
    });

    let mut conn = db::open_database(project_root)?;
    if force {
        db::clear_database(&conn)?;
    }

    for (idx, file) in files.iter().enumerate() {
        if let Some(cb) = on_progress {
            cb(IndexProgress {
                phase: IndexPhase::Parsing,
                current: idx + 1,
                total: files.len(),
                current_file: Some(file.clone()),
            });
        }

        match index_file(project_root, config, &mut conn, file) {
            Ok(Some((node_count, edge_count))) => {
                if node_count > 0 {
                    files_indexed += 1;
                    nodes_created += node_count;
                    edges_created += edge_count;
                } else {
                    files_skipped += 1;
                }
            }
            Ok(None) => {
                files_skipped += 1;
            }
            Err(err) => {
                errors.push(ExtractionError {
                    message: err.to_string(),
                    line: None,
                    column: None,
                    severity: ExtractionErrorSeverity::Error,
                    code: None,
                });
            }
        }
    }

    if let Err(err) = ReferenceResolver::resolve_unresolved(&mut conn, 10_000) {
        errors.push(ExtractionError {
            message: format!("Resolver failed: {err}"),
            line: None,
            column: None,
            severity: ExtractionErrorSeverity::Warning,
            code: Some("resolver_failed".to_string()),
        });
    }

    Ok(IndexResult {
        success: errors
            .iter()
            .all(|e| e.severity != ExtractionErrorSeverity::Error),
        files_indexed,
        files_skipped,
        nodes_created,
        edges_created,
        errors,
        duration_ms: start.elapsed().as_millis(),
    })
}

pub fn sync(
    project_root: &Path,
    config: &CodeGraphConfig,
    on_progress: Option<&dyn Fn(IndexProgress)>,
) -> std::io::Result<SyncResult> {
    let start = Instant::now();
    let mut conn = db::open_database(project_root)?;

    let current_files: HashSet<String> = scan_directory(project_root, config, |_current, _file| {})
        .into_iter()
        .collect();
    let tracked_files = db::list_files(&conn)?;

    let mut files_added = 0;
    let mut files_modified = 0;
    let mut files_removed = 0;
    let mut nodes_updated = 0;

    for tracked in &tracked_files {
        if !current_files.contains(&tracked.path) {
            db::delete_file(&mut conn, &tracked.path)?;
            files_removed += 1;
        }
    }

    for (idx, file) in current_files.iter().enumerate() {
        if let Some(cb) = on_progress {
            cb(IndexProgress {
                phase: IndexPhase::Parsing,
                current: idx + 1,
                total: current_files.len(),
                current_file: Some(file.clone()),
            });
        }

        let full_path = project_root.join(file);
        let content = fs::read_to_string(&full_path)?;
        let content_hash = hash_sha256(&content);
        let tracked = tracked_files.iter().find(|f| f.path == *file);

        if let Some(tracked) = tracked {
            if tracked.content_hash != content_hash {
                if let Some((node_count, _edge_count)) =
                    index_file(project_root, config, &mut conn, file)?
                {
                    files_modified += 1;
                    nodes_updated += node_count;
                }
            }
        } else if let Some((node_count, _edge_count)) =
            index_file(project_root, config, &mut conn, file)?
        {
            files_added += 1;
            nodes_updated += node_count;
        }
    }

    let _ = ReferenceResolver::resolve_unresolved(&mut conn, 10_000);

    Ok(SyncResult {
        files_checked: current_files.len(),
        files_added,
        files_modified,
        files_removed,
        nodes_updated,
        duration_ms: start.elapsed().as_millis(),
    })
}

fn index_file(
    project_root: &Path,
    config: &CodeGraphConfig,
    conn: &mut rusqlite::Connection,
    relative_path: &str,
) -> std::io::Result<Option<(usize, usize)>> {
    let full_path = project_root.join(relative_path);
    let content = fs::read_to_string(&full_path)?;

    if (content.len() as u64) > config.max_file_size {
        return Ok(None);
    }

    let language = detect_language(relative_path);
    if !is_language_supported(&language) {
        return Ok(None);
    }

    let content_hash = hash_sha256(&content);
    if let Some(existing) = db::get_file_record(conn, relative_path)? {
        if existing.content_hash == content_hash {
            return Ok(None);
        }
        db::delete_file(conn, relative_path)?;
    }

    let file_name = Path::new(relative_path)
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or(relative_path);
    let qualified_name = relative_path.to_string();
    let node_id = node_id_for_symbol(relative_path, "file", &qualified_name, 1);
    let file_node_id = node_id.clone();

    let now_ms = now_millis();
    let mut nodes = Vec::new();
    let file_node = Node {
        id: node_id,
        kind: NodeKind::File,
        name: file_name.to_string(),
        qualified_name,
        file_path: relative_path.to_string(),
        language,
        start_line: 1,
        end_line: 1,
        start_column: 0,
        end_column: 0,
        docstring: None,
        signature: None,
        visibility: None,
        is_exported: false,
        is_async: false,
        is_static: false,
        is_abstract: false,
        decorators: None,
        type_parameters: None,
        updated_at: now_ms,
    };
    nodes.push(file_node);

    let (mut extracted_nodes, extracted_edges, unresolved_refs) = extract_nodes(
        project_root,
        relative_path,
        &content,
        language,
        now_ms,
        &file_node_id,
    );
    nodes.append(&mut extracted_nodes);

    if !nodes.is_empty() {
        db::insert_nodes(conn, &nodes)?;
    }
    if !extracted_edges.is_empty() {
        db::insert_edges(conn, &extracted_edges)?;
    }
    if !unresolved_refs.is_empty() {
        db::insert_unresolved_refs(conn, &unresolved_refs)?;
    }

    let metadata = fs::metadata(&full_path)?;
    let file_record = FileRecord {
        path: relative_path.to_string(),
        content_hash,
        language,
        size: metadata.len(),
        modified_at: metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX)),
        indexed_at: now_ms,
        node_count: nodes.len() as i64,
        errors: None,
    };
    db::upsert_file(conn, &file_record)?;

    Ok(Some((nodes.len(), extracted_edges.len())))
}

fn extract_nodes(
    project_root: &Path,
    file_path: &str,
    source: &str,
    language: Language,
    now_ms: i64,
    root_id: &str,
) -> (Vec<Node>, Vec<Edge>, Vec<UnresolvedReference>) {
    let mut parser = Parser::new();
    let ts_lang = match language_to_parser(language) {
        Some(ts_lang) => ts_lang,
        None => return (Vec::new(), Vec::new(), Vec::new()),
    };

    if parser.set_language(&ts_lang).is_err() {
        return (Vec::new(), Vec::new(), Vec::new());
    }

    let tree = match parser.parse(source, None) {
        Some(tree) => tree,
        None => return (Vec::new(), Vec::new(), Vec::new()),
    };

    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut symbol_index = SymbolIndex::default();
    let mut stack = Vec::new();
    let mut unresolved_refs = Vec::new();
    walk_tree_collect(
        tree.root_node(),
        source,
        project_root,
        file_path,
        language,
        &mut stack,
        Some(root_id.to_string()),
        &mut nodes,
        &mut edges,
        &mut symbol_index,
        now_ms,
    );
    walk_tree_calls(
        tree.root_node(),
        source,
        file_path,
        language,
        &symbol_index,
        &mut edges,
        &mut unresolved_refs,
        &mut Vec::new(),
    );
    (nodes, edges, unresolved_refs)
}

fn language_to_parser(language: Language) -> Option<tree_sitter::Language> {
    match language {
        Language::Rust => Some(tree_sitter::Language::new(tree_sitter_rust::LANGUAGE)),
        Language::JavaScript | Language::Jsx => {
            Some(tree_sitter::Language::new(tree_sitter_javascript::LANGUAGE))
        }
        Language::TypeScript => Some(tree_sitter::Language::new(
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        )),
        Language::Tsx => Some(tree_sitter::Language::new(
            tree_sitter_typescript::LANGUAGE_TSX,
        )),
        Language::Python => Some(tree_sitter::Language::new(tree_sitter_python::LANGUAGE)),
        Language::Go => Some(tree_sitter::Language::new(tree_sitter_go::LANGUAGE)),
        Language::Java => Some(tree_sitter::Language::new(tree_sitter_java::LANGUAGE)),
        Language::C => Some(tree_sitter::Language::new(tree_sitter_c::LANGUAGE)),
        Language::Cpp => Some(tree_sitter::Language::new(tree_sitter_cpp::LANGUAGE)),
        Language::CSharp | Language::Blazor => {
            // Use C# parser for both C# and Blazor files
            Some(tree_sitter::Language::new(tree_sitter_c_sharp::LANGUAGE))
        }
        Language::Ruby => Some(tree_sitter::Language::new(tree_sitter_ruby::LANGUAGE)),
        // New language parsers
        Language::Bash => Some(tree_sitter::Language::new(tree_sitter_bash::LANGUAGE)),
        Language::Dart => Some(tree_sitter_dart::language()),
        Language::Elixir => Some(tree_sitter::Language::new(tree_sitter_elixir::LANGUAGE)),
        Language::Elm => Some(tree_sitter::Language::new(tree_sitter_elm::LANGUAGE)),
        Language::Erlang => Some(tree_sitter::Language::new(tree_sitter_erlang::LANGUAGE)),
        Language::Fortran => Some(tree_sitter::Language::new(tree_sitter_fortran::LANGUAGE)),
        Language::Groovy => Some(tree_sitter::Language::new(tree_sitter_groovy::LANGUAGE)),
        Language::Haskell => Some(tree_sitter::Language::new(tree_sitter_haskell::LANGUAGE)),
        Language::Julia => Some(tree_sitter::Language::new(tree_sitter_julia::LANGUAGE)),
        Language::Lua => Some(tree_sitter::Language::new(tree_sitter_lua::LANGUAGE)),
        Language::Matlab => Some(tree_sitter::Language::new(tree_sitter_matlab::LANGUAGE)),
        Language::Nix => Some(tree_sitter::Language::new(tree_sitter_nix::LANGUAGE)),
        Language::Perl => Some(tree_sitter::Language::new(tree_sitter_perl::LANGUAGE)),
        Language::Powershell => Some(tree_sitter::Language::new(tree_sitter_powershell::LANGUAGE)),
        Language::R => Some(tree_sitter::Language::new(tree_sitter_r::LANGUAGE)),
        Language::Scala => Some(tree_sitter::Language::new(tree_sitter_scala::LANGUAGE)),
        Language::Yaml => Some(tree_sitter::Language::new(tree_sitter_yaml::LANGUAGE)),
        Language::Zig => Some(tree_sitter::Language::new(tree_sitter_zig::LANGUAGE)),
        // TODO: PHP, Swift, Kotlin, Markdown, and TOML parsers have API compatibility issues
        // Markdown and TOML require older tree-sitter versions (0.19 and 0.20)
        Language::Php
        | Language::Swift
        | Language::Kotlin
        | Language::Liquid
        | Language::Markdown
        | Language::Toml
        | Language::Unknown => None,
    }
}

#[derive(Debug, Default)]
struct SymbolIndex {
    by_name: HashMap<String, Vec<String>>,
    by_key: HashMap<String, String>,
    callable_ids: HashSet<String>,
}

fn walk_tree_collect(
    node: TsNode,
    source: &str,
    project_root: &Path,
    file_path: &str,
    language: Language,
    stack: &mut Vec<String>,
    parent_id: Option<String>,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    symbol_index: &mut SymbolIndex,
    now_ms: i64,
) {
    let (kind, is_container) = map_node_kind(node.kind(), language);

    if let Some(NodeKind::Import) = kind {
        if let Some(parent_id) = parent_id.clone() {
            add_import_nodes(
                &node, source, language, file_path, parent_id, nodes, edges, now_ms,
            );
            return;
        }
    }

    if let Some(NodeKind::Module) = kind {
        if let Some(parent_id) = parent_id.clone() {
            add_module_node(
                &node,
                source,
                project_root,
                language,
                file_path,
                parent_id,
                nodes,
                edges,
                now_ms,
            );
            return;
        }
    }

    let mut handled_export = false;
    if let Some(NodeKind::Export) = kind {
        if let Some(parent_id) = parent_id.clone() {
            add_export_nodes(
                &node, source, language, file_path, parent_id, nodes, edges, now_ms,
            );
            handled_export = true;
        }
    }

    let name = if handled_export {
        None
    } else {
        match kind {
            Some(_) => node_name(&node, source),
            None => None,
        }
    };

    let mut next_parent_id = parent_id.clone();

    if let (Some(kind), Some(name)) = (kind, name.clone()) {
        let qualified_name = if stack.is_empty() {
            format!("{}::{}", file_path, name)
        } else {
            format!("{}::{}::{}", file_path, stack.join("::"), name)
        };
        let id = node_id_for_symbol(
            file_path,
            &format!("{:?}", kind).to_ascii_lowercase(),
            &qualified_name,
            node.start_position().row as i64 + 1,
        );
        let start = node.start_position();
        let end = node.end_position();

        nodes.push(Node {
            id: id.clone(),
            kind,
            name: name.clone(),
            qualified_name,
            file_path: file_path.to_string(),
            language,
            start_line: start.row as i64 + 1,
            end_line: end.row as i64 + 1,
            start_column: start.column as i64,
            end_column: end.column as i64,
            docstring: None,
            signature: None,
            visibility: None,
            is_exported: false,
            is_async: false,
            is_static: false,
            is_abstract: false,
            decorators: None,
            type_parameters: None,
            updated_at: now_ms,
        });

        if is_callable_kind(kind) {
            let key = node_key(kind, start, &name);
            symbol_index.by_key.insert(key, id.clone());
            symbol_index
                .by_name
                .entry(name.clone())
                .or_default()
                .push(id.clone());
            symbol_index.callable_ids.insert(id.clone());
        }

        if let Some(parent_id) = parent_id.clone() {
            edges.push(Edge {
                source: parent_id.clone(),
                target: id.clone(),
                kind: EdgeKind::Contains,
                metadata: None,
                line: Some(start.row as i64 + 1),
                column: Some(start.column as i64),
            });

            if kind == NodeKind::Import {
                edges.push(Edge {
                    source: parent_id.clone(),
                    target: id.clone(),
                    kind: EdgeKind::Imports,
                    metadata: None,
                    line: Some(start.row as i64 + 1),
                    column: Some(start.column as i64),
                });
            }

            if kind == NodeKind::Export {
                edges.push(Edge {
                    source: parent_id.clone(),
                    target: id.clone(),
                    kind: EdgeKind::Exports,
                    metadata: None,
                    line: Some(start.row as i64 + 1),
                    column: Some(start.column as i64),
                });
            }
        }

        if is_container {
            stack.push(name);
            next_parent_id = Some(id);
        }
    }

    for child in node.children(&mut node.walk()) {
        walk_tree_collect(
            child,
            source,
            project_root,
            file_path,
            language,
            stack,
            next_parent_id.clone(),
            nodes,
            edges,
            symbol_index,
            now_ms,
        );
    }

    if is_container && name.is_some() {
        stack.pop();
    }
}

fn walk_tree_calls(
    node: TsNode,
    source: &str,
    _file_path: &str,
    language: Language,
    symbol_index: &SymbolIndex,
    edges: &mut Vec<Edge>,
    unresolved_refs: &mut Vec<UnresolvedReference>,
    scope_stack: &mut Vec<String>,
) {
    let (kind, _) = map_node_kind(node.kind(), language);
    let name = if kind.is_some() {
        node_name(&node, source)
    } else {
        None
    };

    if let (Some(kind), Some(name)) = (kind, name.clone()) {
        if is_callable_kind(kind) {
            let key = node_key(kind, node.start_position(), &name);
            if let Some(id) = symbol_index.by_key.get(&key) {
                scope_stack.push(id.clone());
            }
        }
    }

    if is_call_expression(node.kind(), language) {
        if let Some(source_id) = scope_stack.last() {
            if let Some(callee_name) = call_name(&node, source, language) {
                let start = node.start_position();
                match symbol_index.by_name.get(&callee_name) {
                    Some(targets) if targets.len() == 1 => {
                        edges.push(Edge {
                            source: source_id.clone(),
                            target: targets[0].clone(),
                            kind: EdgeKind::Calls,
                            metadata: None,
                            line: Some(start.row as i64 + 1),
                            column: Some(start.column as i64),
                        });
                    }
                    Some(targets) => {
                        unresolved_refs.push(UnresolvedReference {
                            from_node_id: source_id.clone(),
                            reference_name: callee_name.clone(),
                            reference_kind: EdgeKind::Calls,
                            line: start.row as i64 + 1,
                            column: start.column as i64,
                            candidates: Some(targets.clone()),
                        });
                    }
                    None => {
                        unresolved_refs.push(UnresolvedReference {
                            from_node_id: source_id.clone(),
                            reference_name: callee_name.clone(),
                            reference_kind: EdgeKind::Calls,
                            line: start.row as i64 + 1,
                            column: start.column as i64,
                            candidates: None,
                        });
                    }
                }
            }
        }
    }

    for child in node.children(&mut node.walk()) {
        walk_tree_calls(
            child,
            source,
            _file_path,
            language,
            symbol_index,
            edges,
            unresolved_refs,
            scope_stack,
        );
    }

    if let (Some(kind), Some(name)) = (kind, name) {
        if is_callable_kind(kind) {
            let key = node_key(kind, node.start_position(), &name);
            if symbol_index.by_key.contains_key(&key) {
                scope_stack.pop();
            }
        }
    }
}

fn node_name(node: &TsNode, source: &str) -> Option<String> {
    let name_node = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("identifier"))
        .or_else(|| node.child_by_field_name("property"))
        .or_else(|| node.child_by_field_name("tag_name"));

    name_node
        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
        .map(|s| s.to_string())
}

#[derive(Debug, Clone)]
struct ImportSymbol {
    local_name: String,
    module_path: String,
    export_name: Option<String>,
}

#[derive(Debug, Clone)]
struct ExportSymbol {
    name: String,
    module_path: Option<String>,
}

fn add_import_nodes(
    node: &TsNode,
    source: &str,
    language: Language,
    file_path: &str,
    parent_id: String,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    now_ms: i64,
) {
    let imports = import_symbols(node, source, language);
    if imports.is_empty() {
        return;
    }

    let start = node.start_position();
    let end = node.end_position();

    for import in imports {
        let qualified_name = format!(
            "{}::import::{}::{}",
            file_path, import.local_name, import.module_path
        );
        let id = node_id_for_symbol(file_path, "import", &qualified_name, start.row as i64 + 1);
        let signature = build_import_signature(&import.module_path, import.export_name.as_deref());

        nodes.push(Node {
            id: id.clone(),
            kind: NodeKind::Import,
            name: import.local_name,
            qualified_name,
            file_path: file_path.to_string(),
            language,
            start_line: start.row as i64 + 1,
            end_line: end.row as i64 + 1,
            start_column: start.column as i64,
            end_column: end.column as i64,
            docstring: None,
            signature: Some(signature),
            visibility: None,
            is_exported: false,
            is_async: false,
            is_static: false,
            is_abstract: false,
            decorators: None,
            type_parameters: None,
            updated_at: now_ms,
        });

        edges.push(Edge {
            source: parent_id.clone(),
            target: id.clone(),
            kind: EdgeKind::Contains,
            metadata: None,
            line: Some(start.row as i64 + 1),
            column: Some(start.column as i64),
        });
        edges.push(Edge {
            source: parent_id.clone(),
            target: id,
            kind: EdgeKind::Imports,
            metadata: None,
            line: Some(start.row as i64 + 1),
            column: Some(start.column as i64),
        });
    }
}

fn import_symbols(node: &TsNode, source: &str, language: Language) -> Vec<ImportSymbol> {
    let Some(module_path) = import_module_path(node, source, language) else {
        return Vec::new();
    };

    if matches!(
        language,
        Language::JavaScript | Language::Jsx | Language::TypeScript | Language::Tsx
    ) {
        let mut imports = Vec::new();
        if let Some(clause) = node.child_by_field_name("import_clause") {
            collect_import_symbols(clause, source, &module_path, &mut imports);
        }

        if imports.is_empty() {
            imports.push(ImportSymbol {
                local_name: module_path.clone(),
                module_path,
                export_name: None,
            });
        }
        return imports;
    }

    let original_name = module_path
        .rsplit("::")
        .next()
        .unwrap_or(&module_path)
        .to_string();
    let alias = rust_use_alias(node, source);

    vec![ImportSymbol {
        local_name: alias.clone().unwrap_or_else(|| original_name.clone()),
        module_path,
        export_name: alias.map(|_| original_name),
    }]
}

fn import_module_path(node: &TsNode, source: &str, language: Language) -> Option<String> {
    let field = match language {
        Language::Rust => "path",
        Language::JavaScript | Language::Jsx | Language::TypeScript | Language::Tsx => "source",
        _ => "source",
    };

    let child = node.child_by_field_name(field)?;
    let raw = child.utf8_text(source.as_bytes()).ok()?.trim().to_string();
    let trimmed = raw.trim_matches(['"', '\''].as_ref()).to_string();

    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn collect_import_symbols(
    node: TsNode,
    source: &str,
    module_path: &str,
    imports: &mut Vec<ImportSymbol>,
) {
    for child in node.children(&mut node.walk()) {
        match child.kind() {
            "identifier" => {
                if let Ok(text) = child.utf8_text(source.as_bytes()) {
                    imports.push(ImportSymbol {
                        local_name: text.to_string(),
                        module_path: module_path.to_string(),
                        export_name: None,
                    });
                }
            }
            "namespace_import" => {
                let name = child
                    .child_by_field_name("name")
                    .or_else(|| child.child_by_field_name("alias"))
                    .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                    .map(|s| s.to_string());
                if let Some(name) = name {
                    imports.push(ImportSymbol {
                        local_name: name,
                        module_path: module_path.to_string(),
                        export_name: None,
                    });
                }
            }
            "named_imports" => collect_named_imports(child, source, module_path, imports),
            "import_specifier" => collect_import_specifier(child, source, module_path, imports),
            _ => {}
        }
    }
}

fn collect_named_imports(
    node: TsNode,
    source: &str,
    module_path: &str,
    imports: &mut Vec<ImportSymbol>,
) {
    for child in node.children(&mut node.walk()) {
        if child.kind() == "import_specifier" {
            collect_import_specifier(child, source, module_path, imports);
        }
    }
}

fn collect_import_specifier(
    node: TsNode,
    source: &str,
    module_path: &str,
    imports: &mut Vec<ImportSymbol>,
) {
    let export_name = node
        .child_by_field_name("name")
        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
        .map(|s| s.to_string());
    let alias = node
        .child_by_field_name("alias")
        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
        .map(|s| s.to_string());

    if let Some(export_name) = export_name {
        let local_name = alias.clone().unwrap_or_else(|| export_name.clone());
        imports.push(ImportSymbol {
            local_name,
            module_path: module_path.to_string(),
            export_name: Some(export_name),
        });
    }
}

fn add_module_node(
    node: &TsNode,
    source: &str,
    project_root: &Path,
    language: Language,
    file_path: &str,
    parent_id: String,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    now_ms: i64,
) {
    let Some(name) = module_name(node, source, language) else {
        return;
    };

    let start = node.start_position();
    let end = node.end_position();
    let qualified_name = format!("{}::{}", file_path, name);
    let id = node_id_for_symbol(file_path, "module", &qualified_name, start.row as i64 + 1);

    let signature = match language {
        Language::Rust => rust_module_target(project_root, file_path, &name),
        _ => None,
    };

    nodes.push(Node {
        id: id.clone(),
        kind: NodeKind::Module,
        name,
        qualified_name,
        file_path: file_path.to_string(),
        language,
        start_line: start.row as i64 + 1,
        end_line: end.row as i64 + 1,
        start_column: start.column as i64,
        end_column: end.column as i64,
        docstring: None,
        signature,
        visibility: None,
        is_exported: false,
        is_async: false,
        is_static: false,
        is_abstract: false,
        decorators: None,
        type_parameters: None,
        updated_at: now_ms,
    });

    edges.push(Edge {
        source: parent_id.clone(),
        target: id.clone(),
        kind: EdgeKind::Contains,
        metadata: None,
        line: Some(start.row as i64 + 1),
        column: Some(start.column as i64),
    });
}

fn module_name(node: &TsNode, source: &str, language: Language) -> Option<String> {
    match language {
        Language::Rust => node
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
            .map(|s| s.to_string()),
        _ => None,
    }
}

fn rust_module_target(project_root: &Path, file_path: &str, name: &str) -> Option<String> {
    let base_dir = Path::new(file_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let candidate_file = base_dir.join(format!("{name}.rs"));
    let candidate_mod = base_dir.join(name).join("mod.rs");

    if project_root.join(&candidate_file).is_file() {
        Some(candidate_file.to_string_lossy().to_string())
    } else if project_root.join(&candidate_mod).is_file() {
        Some(candidate_mod.to_string_lossy().to_string())
    } else {
        None
    }
}

fn add_export_nodes(
    node: &TsNode,
    source: &str,
    language: Language,
    file_path: &str,
    parent_id: String,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    now_ms: i64,
) {
    let exports = export_symbols(node, source, language);
    if exports.is_empty() {
        return;
    }

    let start = node.start_position();
    let end = node.end_position();

    for export in exports {
        let qualified_name = format!("{}::export::{}", file_path, export.name);
        let id = node_id_for_symbol(file_path, "export", &qualified_name, start.row as i64 + 1);

        nodes.push(Node {
            id: id.clone(),
            kind: NodeKind::Export,
            name: export.name,
            qualified_name,
            file_path: file_path.to_string(),
            language,
            start_line: start.row as i64 + 1,
            end_line: end.row as i64 + 1,
            start_column: start.column as i64,
            end_column: end.column as i64,
            docstring: None,
            signature: export.module_path,
            visibility: None,
            is_exported: true,
            is_async: false,
            is_static: false,
            is_abstract: false,
            decorators: None,
            type_parameters: None,
            updated_at: now_ms,
        });

        edges.push(Edge {
            source: parent_id.clone(),
            target: id.clone(),
            kind: EdgeKind::Contains,
            metadata: None,
            line: Some(start.row as i64 + 1),
            column: Some(start.column as i64),
        });
        edges.push(Edge {
            source: parent_id.clone(),
            target: id,
            kind: EdgeKind::Exports,
            metadata: None,
            line: Some(start.row as i64 + 1),
            column: Some(start.column as i64),
        });
    }
}

fn export_symbols(node: &TsNode, source: &str, language: Language) -> Vec<ExportSymbol> {
    match language {
        Language::JavaScript | Language::Jsx | Language::TypeScript | Language::Tsx => {
            let module_path = export_module_path(node, source);
            let mut names = Vec::new();
            collect_export_names(*node, source, &mut names);

            if names.is_empty() {
                return Vec::new();
            }

            names
                .into_iter()
                .map(|name| ExportSymbol {
                    name,
                    module_path: module_path.clone(),
                })
                .collect()
        }
        Language::Rust => {
            let Some(path) = rust_use_path(node, source) else {
                return Vec::new();
            };
            let name = path.rsplit("::").next().unwrap_or(&path).to_string();
            vec![ExportSymbol {
                name,
                module_path: Some(path),
            }]
        }
        _ => Vec::new(),
    }
}

fn rust_use_path(node: &TsNode, source: &str) -> Option<String> {
    let child = node.child_by_field_name("path")?;
    let raw = child.utf8_text(source.as_bytes()).ok()?.trim().to_string();
    if raw.is_empty() { None } else { Some(raw) }
}

fn rust_use_alias(node: &TsNode, source: &str) -> Option<String> {
    node.child_by_field_name("alias")
        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
        .map(|s| s.to_string())
}

fn export_module_path(node: &TsNode, source: &str) -> Option<String> {
    let child = node.child_by_field_name("source")?;
    let raw = child.utf8_text(source.as_bytes()).ok()?.trim().to_string();
    let trimmed = raw.trim_matches(['"', '\''].as_ref()).to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn collect_export_names(node: TsNode, source: &str, names: &mut Vec<String>) {
    if node.kind() == "export_specifier" {
        let alias = node
            .child_by_field_name("alias")
            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
            .map(|s| s.to_string());
        let name = alias.or_else(|| {
            node.child_by_field_name("name")
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                .map(|s| s.to_string())
        });
        if let Some(name) = name {
            names.push(name);
        }
        return;
    }

    if matches!(
        node.kind(),
        "function_declaration"
            | "class_declaration"
            | "interface_declaration"
            | "type_alias_declaration"
            | "enum_declaration"
            | "variable_declarator"
    ) {
        let name = node
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
            .map(|s| s.to_string());
        if let Some(name) = name {
            names.push(name);
        }
        return;
    }

    for child in node.children(&mut node.walk()) {
        collect_export_names(child, source, names);
    }
}

fn build_import_signature(module_path: &str, export_name: Option<&str>) -> String {
    match export_name {
        Some(name) => format!("{module_path}|export={name}"),
        None => module_path.to_string(),
    }
}

fn node_key(kind: NodeKind, start: tree_sitter::Point, name: &str) -> String {
    format!("{:?}:{}:{}:{}", kind, start.row, start.column, name)
}

fn is_callable_kind(kind: NodeKind) -> bool {
    matches!(kind, NodeKind::Function | NodeKind::Method)
}

fn is_call_expression(kind: &str, language: Language) -> bool {
    match language {
        Language::Rust => matches!(kind, "call_expression" | "macro_invocation"),
        Language::JavaScript | Language::Jsx | Language::TypeScript | Language::Tsx => {
            matches!(kind, "call_expression")
        }
        _ => false,
    }
}

fn call_name(node: &TsNode, source: &str, language: Language) -> Option<String> {
    let callee = match language {
        Language::Rust => node.child_by_field_name("function"),
        Language::JavaScript | Language::Jsx | Language::TypeScript | Language::Tsx => node
            .child_by_field_name("function")
            .or_else(|| node.child_by_field_name("callee")),
        _ => None,
    }?;

    let raw = callee.utf8_text(source.as_bytes()).ok()?.to_string();
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let name = trimmed
        .rsplit("::")
        .next()
        .unwrap_or(trimmed)
        .rsplit('.')
        .next()
        .unwrap_or(trimmed)
        .to_string();

    if name.is_empty() { None } else { Some(name) }
}

fn map_node_kind(kind: &str, language: Language) -> (Option<NodeKind>, bool) {
    match language {
        Language::Rust => match kind {
            "function_item" => (Some(NodeKind::Function), false),
            "struct_item" => (Some(NodeKind::Struct), true),
            "enum_item" => (Some(NodeKind::Enum), true),
            "trait_item" => (Some(NodeKind::Trait), true),
            "use_declaration" => (Some(NodeKind::Import), false),
            "mod_item" => (Some(NodeKind::Module), true),
            "use_item" => (Some(NodeKind::Export), false),
            _ => (None, false),
        },
        Language::JavaScript | Language::Jsx | Language::TypeScript | Language::Tsx => match kind {
            "function_declaration" => (Some(NodeKind::Function), false),
            "class_declaration" => (Some(NodeKind::Class), true),
            "method_definition" => (Some(NodeKind::Method), false),
            "interface_declaration" => (Some(NodeKind::Interface), true),
            "type_alias_declaration" => (Some(NodeKind::TypeAlias), false),
            "import_declaration" => (Some(NodeKind::Import), false),
            "export_statement" | "export_declaration" => (Some(NodeKind::Export), false),
            _ => (None, false),
        },
        Language::Blazor => match kind {
            "element" => (Some(NodeKind::Component), true),
            _ => (None, false),
        },
        _ => (None, false),
    }
}

fn scan_directory(
    root_dir: &Path,
    config: &CodeGraphConfig,
    mut on_progress: impl FnMut(usize, &str),
) -> Vec<String> {
    let mut files = Vec::new();
    let mut count = 0;

    let mut stack = vec![root_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let rel_path = match path.strip_prefix(root_dir) {
                Ok(rel) => rel,
                Err(_) => continue,
            };
            let rel_str = rel_path.to_string_lossy().to_string();

            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let dir_pattern = format!("{}/", rel_str);
                if config.exclude.iter().any(|p| matches_glob(&dir_pattern, p)) {
                    continue;
                }
                stack.push(path);
            } else if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                if should_include_file(&rel_str, config) {
                    files.push(rel_str.clone());
                    count += 1;
                    on_progress(count, &rel_str);
                }
            }
        }
    }

    files
}

fn should_include_file(file_path: &str, config: &CodeGraphConfig) -> bool {
    for pattern in &config.exclude {
        if matches_glob(file_path, pattern) {
            return false;
        }
    }

    for pattern in &config.include {
        if matches_glob(file_path, pattern) {
            return true;
        }
    }

    false
}

fn matches_glob(file_path: &str, pattern: &str) -> bool {
    globset::Glob::new(pattern)
        .ok()
        .and_then(|glob| glob.compile_matcher().is_match(file_path).then_some(true))
        .unwrap_or(false)
}

fn detect_language(path: &str) -> Language {
    let ext = Path::new(path)
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "ts" => Language::TypeScript,
        "tsx" => Language::Tsx,
        "js" => Language::JavaScript,
        "jsx" => Language::Jsx,
        "py" => Language::Python,
        "go" => Language::Go,
        "rs" => Language::Rust,
        "java" => Language::Java,
        "c" => Language::C,
        "h" => Language::C,
        "cpp" | "cc" | "cxx" | "hpp" => Language::Cpp,
        "cs" => Language::CSharp,
        "php" => Language::Php,
        "rb" => Language::Ruby,
        "swift" => Language::Swift,
        "kt" => Language::Kotlin,
        "liquid" => Language::Liquid,
        "razor" | "cshtml" => Language::Blazor,
        // New languages
        "sh" | "bash" => Language::Bash,
        "dart" => Language::Dart,
        "ex" | "exs" => Language::Elixir,
        "elm" => Language::Elm,
        "erl" | "hrl" => Language::Erlang,
        "f" | "f90" | "f95" => Language::Fortran,
        "groovy" | "gradle" => Language::Groovy,
        "hs" => Language::Haskell,
        "jl" => Language::Julia,
        "lua" => Language::Lua,
        "md" | "markdown" => Language::Markdown,
        "m" => Language::Matlab,
        "nix" => Language::Nix,
        "pl" | "pm" => Language::Perl,
        "ps1" => Language::Powershell,
        "r" => Language::R,
        "scala" | "sc" => Language::Scala,
        "toml" => Language::Toml,
        "yml" | "yaml" => Language::Yaml,
        "zig" => Language::Zig,
        _ => Language::Unknown,
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
