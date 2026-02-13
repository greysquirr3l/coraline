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
