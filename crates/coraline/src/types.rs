#![deny(unsafe_code)]

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

/// Define the `Language` enum, its canonical names, and the list of
/// every language, from one list.
///
/// The three have to agree: a language without a name cannot be
/// reported, and a name without a language cannot be looked up. Writing
/// them out separately is how they drift apart, so they are written
/// once here and the compiler checks that they match.
macro_rules! languages {
    ($($variant:ident => $name:literal),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Language {
            $($variant,)*
            /// The catch-all for a file Coraline does not recognise.
            Unknown,
        }

        impl Language {
            /// The language's canonical name.
            ///
            /// Lower case, and the same name the `grammar_probe`
            /// example takes on the command line.
            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)*
                    Self::Unknown => "unknown",
                }
            }
        }

        /// Every language except the `Unknown` catch-all.
        ///
        /// `Unknown` is not a language: it is what an unrecognised
        /// extension maps to, so it has no grammar and no contract.
        pub const ALL_LANGUAGES: &[Language] = &[$(Language::$variant,)*];
    };
}

languages! {
    TypeScript => "typescript",
    JavaScript => "javascript",
    Tsx => "tsx",
    Jsx => "jsx",
    Python => "python",
    Go => "go",
    Rust => "rust",
    Java => "java",
    C => "c",
    Cpp => "cpp",
    CSharp => "csharp",
    Php => "php",
    Ruby => "ruby",
    Swift => "swift",
    Kotlin => "kotlin",
    Liquid => "liquid",
    Blazor => "blazor",
    Bash => "bash",
    Dart => "dart",
    Elixir => "elixir",
    Elm => "elm",
    Erlang => "erlang",
    Fortran => "fortran",
    Groovy => "groovy",
    Haskell => "haskell",
    Julia => "julia",
    Lua => "lua",
    Markdown => "markdown",
    Matlab => "matlab",
    Nix => "nix",
    Perl => "perl",
    Powershell => "powershell",
    R => "r",
    Scala => "scala",
    Toml => "toml",
    Yaml => "yaml",
    Zig => "zig",
}

impl Language {
    /// The language with the given name, or `Unknown`.
    ///
    /// The lookup is case-insensitive, so `Fortran` and `fortran` are
    /// the same language. The older snake-case spellings that earlier
    /// indexes were written with are accepted too, so an index built by
    /// an older version still reads back correctly.
    pub fn from_name(name: &str) -> Self {
        Self::lookup(name).unwrap_or(Self::Unknown)
    }

    /// The language with the given name, if the name is one Coraline
    /// knows.
    ///
    /// Accepts the canonical names from [`Language::name`], and the
    /// legacy spellings `type_script`, `java_script` and `c_sharp` that
    /// older indexes stored.
    fn lookup(name: &str) -> Option<Self> {
        let name = name.to_ascii_lowercase();
        match name.as_str() {
            "type_script" => Some(Self::TypeScript),
            "java_script" => Some(Self::JavaScript),
            "c_sharp" => Some(Self::CSharp),
            _ => ALL_LANGUAGES
                .iter()
                .copied()
                .find(|language| language.name() == name),
        }
    }
}

impl Serialize for Language {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for Language {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Self::lookup(&name)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown language: {name}")))
    }
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
    /// Louvain community id assigned at index time (Phase 5.1). `None`
    /// means "not clustered" — either the graph was too sparse, the node
    /// isn't connected, or clustering hasn't been run yet on this DB.
    #[serde(default)]
    pub cluster_id: Option<i64>,
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
    /// Resolution confidence in `[0.0, 1.0]`.
    ///
    /// - `1.0`  — directly extracted by the AST pass (caller wrote the symbol
    ///   syntactically; no resolution was needed).
    /// - `0.95` — resolved via a strongly-typed Rust path
    ///   (`crate::`, `super::`, `self::`).
    /// - `0.5`  — generic name match / framework fallback / heuristic ranker.
    ///
    /// Older rows created before this column existed default to `1.0` so the
    /// column can be added additively without backfill logic.
    #[serde(default = "default_edge_confidence")]
    pub confidence: f32,
    /// Execution-flow trace id assigned by the post-extraction process
    /// tracer (Phase 5.1). `None` means "not part of any traced process"
    /// — either the edge isn't a `Calls` edge, or the trace from the
    /// relevant entry point didn't reach this edge within
    /// `max_process_depth`.
    #[serde(default)]
    pub process_id: Option<i64>,
}

const fn default_edge_confidence() -> f32 {
    1.0
}

/// Metadata key under which an edge records the receiver of the reference
/// it was resolved from.
///
/// For a qualified call such as `util.Area()` the receiver is `util`. It is
/// the only thing that distinguishes `util.Area` from an unrelated `Area`
/// with the same name, so it has to outlive the resolution: when the
/// target file is re-indexed the edge is rebuilt as an unresolved
/// reference, and a reference that has lost its qualifier can be
/// re-pointed at a different symbol.
pub const EDGE_QUALIFIER_METADATA: &str = "qualifier";

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
    /// The receiver of a qualified reference, when the call site had one.
    ///
    /// For `Helper.doThing()` the reference name is `doThing` and the
    /// qualifier is `Helper`. The resolver uses the qualifier to match the
    /// reference against the local name of an import, which is what lets
    /// cross-package calls resolve.
    pub qualifier: Option<String>,
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
