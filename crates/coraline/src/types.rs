#![forbid(unsafe_code)]

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    File,
    Module,
    Class,
    Struct,
    Interface,
    Trait,
    Protocol,
    Function,
    Method,
    Property,
    Field,
    Variable,
    Constant,
    Enum,
    EnumMember,
    TypeAlias,
    Namespace,
    Parameter,
    Import,
    Export,
    Route,
    Component,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Contains,
    Calls,
    Imports,
    Exports,
    Extends,
    Implements,
    References,
    TypeOf,
    Returns,
    Instantiates,
    Overrides,
    Decorates,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    TypeScript,
    JavaScript,
    Tsx,
    Jsx,
    Python,
    Go,
    Rust,
    Java,
    C,
    Cpp,
    CSharp,
    Php,
    Ruby,
    Swift,
    Kotlin,
    Liquid,
    Blazor,
    // New languages (tree-sitter support)
    Bash,
    Dart,
    Elixir,
    Elm,
    Erlang,
    Fortran,
    Groovy,
    Haskell,
    Julia,
    Lua,
    Markdown,
    Matlab,
    Nix,
    Perl,
    Powershell,
    R,
    Scala,
    Toml,
    Yaml,
    Zig,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Public,
    Private,
    Protected,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub kind: NodeKind,
    pub name: String,
    pub qualified_name: String,
    pub file_path: String,
    pub language: Language,
    pub start_line: i64,
    pub end_line: i64,
    pub start_column: i64,
    pub end_column: i64,
    pub docstring: Option<String>,
    pub signature: Option<String>,
    pub visibility: Option<Visibility>,
    pub is_exported: bool,
    pub is_async: bool,
    pub is_static: bool,
    pub is_abstract: bool,
    pub decorators: Option<Vec<String>>,
    pub type_parameters: Option<Vec<String>>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub source: String,
    pub target: String,
    pub kind: EdgeKind,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    pub line: Option<i64>,
    pub column: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRecord {
    pub path: String,
    pub content_hash: String,
    pub language: Language,
    pub size: u64,
    pub modified_at: i64,
    pub indexed_at: i64,
    pub node_count: i64,
    pub errors: Option<Vec<ExtractionError>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub unresolved_references: Vec<UnresolvedReference>,
    pub errors: Vec<ExtractionError>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionErrorSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionError {
    pub message: String,
    pub line: Option<i64>,
    pub column: Option<i64>,
    pub severity: ExtractionErrorSeverity,
    pub code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnresolvedReference {
    pub from_node_id: String,
    pub reference_name: String,
    pub reference_kind: EdgeKind,
    pub line: i64,
    pub column: i64,
    pub candidates: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subgraph {
    pub nodes: HashMap<String, Node>,
    pub edges: Vec<Edge>,
    pub roots: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraversalDirection {
    Outgoing,
    Incoming,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraversalOptions {
    pub max_depth: Option<usize>,
    pub edge_kinds: Option<Vec<EdgeKind>>,
    pub node_kinds: Option<Vec<NodeKind>>,
    pub direction: Option<TraversalDirection>,
    pub limit: Option<usize>,
    pub include_start: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchOptions {
    pub kinds: Option<Vec<NodeKind>>,
    pub languages: Option<Vec<Language>>,
    pub include_patterns: Option<Vec<String>>,
    pub exclude_patterns: Option<Vec<String>>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub case_sensitive: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub node: Node,
    pub score: f32,
    pub highlights: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeEdge {
    pub node: Node,
    pub edge: Edge,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Context {
    pub focal: Node,
    pub ancestors: Vec<Node>,
    pub children: Vec<Node>,
    pub incoming_refs: Vec<NodeEdge>,
    pub outgoing_refs: Vec<NodeEdge>,
    pub types: Vec<Node>,
    pub imports: Vec<Node>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeBlock {
    pub content: String,
    pub file_path: String,
    pub start_line: i64,
    pub end_line: i64,
    pub language: Language,
    pub node: Option<Node>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameworkHintPatterns {
    pub components: Option<Vec<String>>,
    pub routes: Option<Vec<String>>,
    pub models: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameworkHint {
    pub name: String,
    pub version: Option<String>,
    pub patterns: Option<FrameworkHintPatterns>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomPattern {
    pub name: String,
    pub pattern: String,
    pub kind: NodeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGraphConfig {
    pub version: i64,
    pub root_dir: String,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub languages: Vec<Language>,
    pub frameworks: Vec<FrameworkHint>,
    pub max_file_size: u64,
    pub extract_docstrings: bool,
    pub track_call_sites: bool,
    pub enable_embeddings: bool,
    pub custom_patterns: Option<Vec<CustomPattern>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskInput {
    Text(String),
    Detailed {
        title: String,
        description: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildContextOptions {
    pub max_nodes: Option<usize>,
    pub max_code_blocks: Option<usize>,
    pub max_code_block_size: Option<usize>,
    pub include_code: Option<bool>,
    pub format: Option<ContextFormat>,
    pub search_limit: Option<usize>,
    pub traversal_depth: Option<usize>,
    pub min_score: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextFormat {
    Markdown,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskContext {
    pub query: String,
    pub subgraph: Subgraph,
    pub entry_points: Vec<Node>,
    pub code_blocks: Vec<CodeBlock>,
    pub related_files: Vec<String>,
    pub summary: String,
    pub stats: ContextStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextStats {
    pub node_count: usize,
    pub edge_count: usize,
    pub file_count: usize,
    pub code_block_count: usize,
    pub total_code_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindRelevantContextOptions {
    pub search_limit: Option<usize>,
    pub traversal_depth: Option<usize>,
    pub max_nodes: Option<usize>,
    pub min_score: Option<f32>,
    pub edge_kinds: Option<Vec<EdgeKind>>,
    pub node_kinds: Option<Vec<NodeKind>>,
}
