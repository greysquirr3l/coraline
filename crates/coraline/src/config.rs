#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::types::{CodeGraphConfig, FrameworkHint, Language, NodeKind};

pub const CONFIG_FILENAME: &str = "config.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigFile {
    #[serde(flatten)]
    pub config: CodeGraphConfig,
}

pub fn config_path(project_root: &Path) -> PathBuf {
    project_root.join(".coraline").join(CONFIG_FILENAME)
}

pub fn create_default_config(project_root: &Path) -> CodeGraphConfig {
    CodeGraphConfig {
        version: 1,
        root_dir: project_root.to_string_lossy().to_string(),
        include: default_include_patterns(),
        exclude: default_exclude_patterns(),
        languages: Vec::new(),
        frameworks: Vec::new(),
        max_file_size: 1024 * 1024,
        extract_docstrings: true,
        track_call_sites: true,
        enable_embeddings: true,
        custom_patterns: None,
    }
}

pub fn load_config(project_root: &Path) -> std::io::Result<CodeGraphConfig> {
    let path = config_path(project_root);
    if !path.exists() {
        return Ok(create_default_config(project_root));
    }

    let raw = fs::read_to_string(&path)?;
    let mut config: CodeGraphConfig = serde_json::from_str(&raw)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    config.root_dir = project_root.to_string_lossy().to_string();
    Ok(config)
}

pub fn save_config(project_root: &Path, config: &CodeGraphConfig) -> std::io::Result<()> {
    let path = config_path(project_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut to_save = config.clone();
    to_save.root_dir = ".".to_string();
    let raw = serde_json::to_string_pretty(&to_save)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    fs::write(path, raw)
}

pub fn add_include_patterns(config: &mut CodeGraphConfig, patterns: &[String]) {
    for pattern in patterns {
        if !config.include.contains(pattern) {
            config.include.push(pattern.clone());
        }
    }
}

pub fn add_exclude_patterns(config: &mut CodeGraphConfig, patterns: &[String]) {
    for pattern in patterns {
        if !config.exclude.contains(pattern) {
            config.exclude.push(pattern.clone());
        }
    }
}

pub fn add_custom_pattern(config: &mut CodeGraphConfig, name: &str, pattern: &str, kind: NodeKind) {
    let entry = config.custom_patterns.get_or_insert_with(Vec::new);
    if let Some(existing) = entry.iter_mut().find(|p| p.name == name) {
        existing.pattern = pattern.to_string();
        existing.kind = kind;
        return;
    }

    entry.push(crate::types::CustomPattern {
        name: name.to_string(),
        pattern: pattern.to_string(),
        kind,
    });
}

pub const fn default_frameworks() -> Vec<FrameworkHint> {
    Vec::new()
}

pub fn default_include_patterns() -> Vec<String> {
    vec![
        "**/*.ts",
        "**/*.tsx",
        "**/*.js",
        "**/*.jsx",
        "**/*.py",
        "**/*.go",
        "**/*.rs",
        "**/*.java",
        "**/*.c",
        "**/*.h",
        "**/*.cpp",
        "**/*.hpp",
        "**/*.cc",
        "**/*.cxx",
        "**/*.cs",
        "**/*.php",
        "**/*.rb",
        "**/*.liquid",
        "**/*.razor",
    ]
    .into_iter()
    .map(std::string::ToString::to_string)
    .collect()
}

pub fn default_exclude_patterns() -> Vec<String> {
    vec![
        "**/.git/**",
        "**/node_modules/**",
        "**/vendor/**",
        "**/Pods/**",
        "**/dist/**",
        "**/build/**",
        "**/out/**",
        "**/bin/**",
        "**/obj/**",
        "**/target/**",
        "**/*.min.js",
        "**/*.bundle.js",
        "**/.next/**",
        "**/.nuxt/**",
        "**/.svelte-kit/**",
        "**/.output/**",
        "**/.turbo/**",
        "**/.cache/**",
        "**/.parcel-cache/**",
        "**/.vite/**",
        "**/.astro/**",
        "**/.docusaurus/**",
        "**/.gatsby/**",
        "**/.webpack/**",
        "**/.nx/**",
        "**/.yarn/cache/**",
        "**/.pnpm-store/**",
        "**/storybook-static/**",
        "**/.expo/**",
        "**/web-build/**",
        "**/ios/Pods/**",
        "**/ios/build/**",
        "**/android/build/**",
        "**/android/.gradle/**",
        "**/__pycache__/**",
        "**/.venv/**",
        "**/venv/**",
        "**/*_venv/**",
        "**/*-venv/**",
        "**/env/**",
        "**/.env/**",
        "**/.pytest_cache/**",
        "**/.mypy_cache/**",
        "**/.ruff_cache/**",
        "**/.tox/**",
        "**/.nox/**",
        "**/*.egg-info/**",
        "**/.eggs/**",
        "**/go/pkg/mod/**",
        "**/target/debug/**",
        "**/target/release/**",
        "**/.gradle/**",
        "**/.m2/**",
        "**/generated-sources/**",
        "**/.kotlin/**",
        "**/.vs/**",
        "**/.nuget/**",
        "**/artifacts/**",
        "**/publish/**",
        "**/cmake-build-*/**",
        "**/CMakeFiles/**",
        "**/bazel-*/**",
        "**/vcpkg_installed/**",
        "**/.conan/**",
        "**/Debug/**",
        "**/Release/**",
        "**/x64/**",
        "**/release/**",
        "**/*.app/**",
        "**/*.asar",
        "**/DerivedData/**",
        "**/.build/**",
        "**/.swiftpm/**",
        "**/xcuserdata/**",
        "**/Carthage/Build/**",
        "**/SourcePackages/**",
        "**/.composer/**",
        "**/storage/framework/**",
        "**/bootstrap/cache/**",
        "**/.bundle/**",
        "**/tmp/cache/**",
        "**/public/assets/**",
        "**/public/packs/**",
        "**/.yardoc/**",
        "**/coverage/**",
        "**/htmlcov/**",
        "**/.nyc_output/**",
        "**/test-results/**",
        "**/.coverage/**",
        "**/.idea/**",
        "**/logs/**",
        "**/tmp/**",
        "**/temp/**",
        "**/_build/**",
        "**/docs/_build/**",
        "**/site/**",
    ]
    .into_iter()
    .map(std::string::ToString::to_string)
    .collect()
}

pub const fn is_language_supported(language: &Language) -> bool {
    matches!(
        language,
        Language::TypeScript
            | Language::JavaScript
            | Language::Tsx
            | Language::Jsx
            | Language::Python
            | Language::Go
            | Language::Rust
            | Language::Java
            | Language::C
            | Language::Cpp
            | Language::CSharp
            | Language::Php
            | Language::Ruby
            | Language::Swift
            | Language::Kotlin
            | Language::Liquid
            | Language::Blazor
            | Language::Unknown
    )
}

// ── Extended TOML configuration ───────────────────────────────────────────────

/// Filename for the user-editable TOML configuration.
pub const TOML_CONFIG_FILENAME: &str = "config.toml";

pub fn toml_config_path(project_root: &Path) -> PathBuf {
    project_root.join(".coraline").join(TOML_CONFIG_FILENAME)
}

/// Context-builder settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ContextConfig {
    /// Maximum graph nodes to include in context output.
    pub max_nodes: usize,
    /// Maximum code blocks to attach.
    pub max_code_blocks: usize,
    /// Maximum characters per code block.
    pub max_code_block_size: usize,
    /// Graph traversal depth from entry nodes.
    pub traversal_depth: usize,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_nodes: 20,
            max_code_blocks: 5,
            max_code_block_size: 1500,
            traversal_depth: 1,
        }
    }
}

/// Incremental-sync and git-hook settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncConfig {
    /// Whether to install / honour git post-commit hooks.
    pub git_hooks_enabled: bool,
    /// Enable watch mode (re-index on file changes) — not yet implemented.
    pub watch_mode: bool,
    /// Debounce delay in milliseconds for watch mode.
    pub debounce_ms: u64,
    /// Interval in seconds for the MCP background auto-sync check.
    /// Set to 0 to disable. Default: 120 (2 minutes).
    pub auto_sync_interval_secs: u64,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            git_hooks_enabled: true,
            watch_mode: false,
            debounce_ms: 500,
            auto_sync_interval_secs: 120,
        }
    }
}

/// Vector-embedding settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VectorsConfig {
    /// Enable vector embeddings (requires ONNX model).
    pub enabled: bool,
    /// Model identifier.
    pub model: String,
    /// Embedding dimension (must match the model).
    pub dimension: usize,
    /// Batch size for embedding generation.
    pub batch_size: usize,
    /// Path to the model directory (containing an ONNX file + tokenizer.json).
    /// Defaults to `.coraline/models/nomic-embed-text-v1.5/`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_dir: Option<String>,
    /// Specific ONNX filename to use (e.g. `model_int8.onnx`).
    /// When unset, Coraline auto-detects the best available variant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_file: Option<String>,
    /// Maximum sequence length in tokens (default 512).
    pub max_seq_len: usize,
}

impl Default for VectorsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: "nomic-embed-text-v1.5".to_string(),
            dimension: 768,
            batch_size: 32,
            model_dir: None,
            model_file: None,
            max_seq_len: 512,
        }
    }
}

/// Indexing settings (superset of the legacy `CodeGraphConfig` fields).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IndexingConfig {
    /// Maximum file size to index in bytes.
    pub max_file_size: u64,
    /// Number of files processed per batch.
    pub batch_size: usize,
    /// Glob patterns to include.
    pub include_patterns: Vec<String>,
    /// Glob patterns to exclude.
    pub exclude_patterns: Vec<String>,
}

impl Default for IndexingConfig {
    fn default() -> Self {
        Self {
            max_file_size: 1024 * 1024,
            batch_size: 100,
            include_patterns: default_include_patterns(),
            exclude_patterns: default_exclude_patterns(),
        }
    }
}

/// Top-level TOML configuration for a Coraline project.
///
/// Stored at `.coraline/config.toml`.  All sections are optional with
/// sensible defaults so that an empty file is perfectly valid.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CoralineConfig {
    pub indexing: IndexingConfig,
    pub context: ContextConfig,
    pub sync: SyncConfig,
    pub vectors: VectorsConfig,
}

impl CoralineConfig {
    /// Return defaults identical to those used when no config file is present.
    pub fn default_config() -> Self {
        Self::default()
    }
}

/// Load the TOML config from `.coraline/config.toml`, returning defaults if
/// the file does not exist.  Returns an error only on parse failures.
pub fn load_toml_config(project_root: &Path) -> std::io::Result<CoralineConfig> {
    let path = toml_config_path(project_root);
    if !path.exists() {
        return Ok(CoralineConfig::default_config());
    }
    let raw = fs::read_to_string(&path)?;
    toml::from_str(&raw).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Persist the TOML config to `.coraline/config.toml`.
pub fn save_toml_config(project_root: &Path, cfg: &CoralineConfig) -> std::io::Result<()> {
    let path = toml_config_path(project_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = toml::to_string_pretty(cfg).map_err(std::io::Error::other)?;
    fs::write(path, raw)
}

/// Merge TOML config settings into a `CodeGraphConfig`.
///
/// TOML values override the code-graph config only when the TOML config
/// differs from its own defaults, which means user-set values win but an
/// untouched `config.toml` leaves the existing `CodeGraphConfig` unchanged.
pub fn apply_toml_to_code_graph(code_cfg: &mut CodeGraphConfig, toml_cfg: &CoralineConfig) {
    let def = IndexingConfig::default();

    if toml_cfg.indexing.max_file_size != def.max_file_size {
        code_cfg.max_file_size = toml_cfg.indexing.max_file_size;
    }
    if toml_cfg.indexing.include_patterns != def.include_patterns {
        code_cfg
            .include
            .clone_from(&toml_cfg.indexing.include_patterns);
    }
    if toml_cfg.indexing.exclude_patterns != def.exclude_patterns {
        code_cfg
            .exclude
            .clone_from(&toml_cfg.indexing.exclude_patterns);
    }
}

/// Write a well-commented default `config.toml` template.
pub fn write_toml_template(project_root: &Path) -> std::io::Result<()> {
    let path = toml_config_path(project_root);
    if path.exists() {
        return Ok(()); // Never clobber an existing config.
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, DEFAULT_TOML_TEMPLATE)
}

const DEFAULT_TOML_TEMPLATE: &str = r#"# Coraline project configuration
# All settings are optional — defaults are shown below.

[indexing]
max_file_size = 1048576   # 1 MB
batch_size    = 100
include_patterns = [
  "**/*.rs", "**/*.ts", "**/*.tsx", "**/*.js", "**/*.jsx",
  "**/*.py", "**/*.go", "**/*.java", "**/*.cs", "**/*.cpp",
  "**/*.c", "**/*.h", "**/*.rb", "**/*.php", "**/*.swift",
  "**/*.kt", "**/*.razor",
]
exclude_patterns = [
  "**/.git/**", "**/target/**", "**/node_modules/**",
  "**/dist/**", "**/build/**", "**/.coraline/**",
  # Python virtualenvs — covers .venv/, venv/, env/, and named venvs
  # like cluster_venv/, memory_venv/, project-venv/, etc.
  "**/.venv/**", "**/venv/**", "**/*_venv/**", "**/*-venv/**",
  "**/env/**", "**/.env/**", "**/__pycache__/**",
]

[context]
max_nodes          = 20
max_code_blocks    = 5
max_code_block_size = 1500
traversal_depth    = 1

[sync]
git_hooks_enabled        = true
watch_mode               = false
debounce_ms              = 500
auto_sync_interval_secs  = 120

[vectors]
# Full vector search requires an ONNX model.
# Download any variant from huggingface.co/nomic-ai/nomic-embed-text-v1.5
# (also copy tokenizer.json) into the directory below:
#   model_int8.onnx      137 MB  — int8 quantized (recommended)
#   model_quantized.onnx 137 MB  — same as int8
#   model_uint8.onnx     137 MB  — uint8 quantized
#   model_q4f16.onnx     111 MB  — smallest (Q4 + fp16)
#   model_q4.onnx        165 MB  — Q4 quantized
#   model_fp16.onnx      274 MB  — fp16
#   model.onnx           547 MB  — full f32
# Coraline auto-selects the best available file; set model_file to override.
# Then run: coraline embed
enabled    = false
model      = "nomic-embed-text-v1.5"
dimension  = 768
batch_size = 32
max_seq_len = 512
# model_dir  = ".coraline/models/nomic-embed-text-v1.5"  # override default path
# model_file = "model_int8.onnx"                          # pin a specific variant
"#;
