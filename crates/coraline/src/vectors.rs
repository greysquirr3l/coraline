#![forbid(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::doc_markdown,
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::items_after_statements,
    clippy::redundant_closure_for_method_calls,
    clippy::uninlined_format_args,
    clippy::unwrap_used
)]

//! Vector embeddings for semantic code search.
//!
//! Generates 768-dimensional embeddings using a locally-stored ONNX model
//! via the `ort` ONNX Runtime bindings. Coraline supports multiple embedding
//! models — see [`SupportedModel`] for the registry.
//!
//! ## Quick start
//!
//! 1. Run `coraline model list` to see supported models, then
//!    `coraline model download` to fetch the default
//!    (`nomic-embed-text-v1.5`) into `~/.config/coraline/models/<model>/`.
//!    Pass `--model jina-embeddings-v2-base-code` to opt into the
//!    code-specialised model instead.
//! 2. Run `coraline embed` to generate embeddings for all indexed nodes.
//! 3. Use the `coraline_semantic_search` MCP tool to search by natural language.
//!
//! Run `coraline doctor` at any point to check model presence, that it
//! loads and runs inference, and embed coverage in one pass.
//!
//! ## Switching models
//!
//! Setting `vectors.model` in `.coraline/config.toml` to a different
//! supported model does not retroactively re-embed existing nodes. Every
//! read path (`coraline_semantic_search`, `coraline doctor`'s coverage
//! probe, staleness checks) filters on the `model` column already stored
//! per-embedding, so a switch makes every node look unembedded *for the
//! newly configured model* until `coraline embed` is re-run — the system
//! self-heals rather than mixing scores across models, but the re-embed
//! step is not automatic.
//!
//! ## Model variant preference order
//!
//! When `model_file` is not configured, Coraline picks the first file found
//! from the configured model's preference order (see [`SupportedModel`]).
//! This prefers well-quantized variants over the full f32 model.

use std::io;
use std::path::{Path, PathBuf};

use ndarray::Array2;
use ort::{
    inputs,
    session::{Session, builder::GraphOptimizationLevel},
    value::TensorRef,
};
use rusqlite::{Connection, params};
use tokenizers::Tokenizer;

use crate::config::VectorsConfig;
use crate::types::SearchResult;

/// Identifier for the default shipped embedding model.
pub const DEFAULT_MODEL: &str = "nomic-embed-text-v1.5";

/// Output dimension for every currently supported model.
///
/// Both `nomic-embed-text-v1.5` and `jina-embeddings-v2-base-code` emit
/// 768-dim vectors; the constant exists so downstream code doesn't have to
/// know the model name to allocate buffers.
pub const EMBEDDING_DIM: usize = 768;

/// Maximum sequence length fed to the model (tokens).
/// Supported models handle much longer contexts, but 512 covers most code
/// snippets without inflating ONNX session memory.
pub const MAX_SEQ_LEN: usize = 512;

/// ONNX preference order for nomic-embed-text-v1.5.
///
/// Preference is given to quantized variants (smaller on disk, faster to
/// load) before falling back to the full f32 model.
const NOMIC_PREFERENCE_ORDER: &[&str] = &[
    "model_int8.onnx",      // 137 MB  — int8 quantized (recommended)
    "model_quantized.onnx", // 137 MB  — same weights, different name
    "model_uint8.onnx",     // 137 MB  — uint8 quantized
    "model_q4f16.onnx",     // 111 MB  — Q4 + fp16 mixed (smallest)
    "model_q4.onnx",        // 165 MB  — Q4 quantized
    "model_bnb4.onnx",      // 158 MB  — 4-bit NF4
    "model_fp16.onnx",      // 274 MB  — fp16
    "model.onnx",           // 547 MB  — full f32 (fallback)
];

/// ONNX preference order for jina-embeddings-v2-base-code.
const JINA_CODE_PREFERENCE_ORDER: &[&str] = &[
    "model_quantized.onnx", // 162 MB — int8 quantized (recommended)
    "model_fp16.onnx",      // 321 MB — fp16
    "model.onnx",           // 642 MB — full f32 (fallback)
];

/// Retained for backward compatibility with callers that assume the
/// default (nomic) model. Prefer calling [`model_spec`] and reading
/// `preference_order` off the result.
pub const MODEL_PREFERENCE_ORDER: &[&str] = NOMIC_PREFERENCE_ORDER;

/// One entry in the embedding-model registry: everything needed to
/// download and run a supported ONNX model.
#[derive(Debug, Clone, Copy)]
pub struct SupportedModel {
    /// Canonical model name (matches the `HuggingFace` repo slug and the
    /// `vectors.model` config value).
    pub name: &'static str,
    /// `HuggingFace` `resolve/main` base URL for the repo.
    pub hf_base_url: &'static str,
    /// ONNX filename to download by default (e.g. when running
    /// `coraline model download` with no `--variant`).
    pub default_filename: &'static str,
    /// Filenames tried in order when auto-detecting an installed variant.
    pub preference_order: &'static [&'static str],
    /// Embedding vector dimension produced by the model.
    pub dimension: usize,
    /// Short human-readable description (shown by `coraline model list`).
    pub description: &'static str,
}

/// Registry of every model Coraline knows how to download and run.
///
/// `nomic-embed-text-v1.5` is the default; other entries are opt-in via
/// `vectors.model` in `.coraline/config.toml`.
pub const SUPPORTED_MODELS: &[SupportedModel] = &[
    SupportedModel {
        name: DEFAULT_MODEL,
        hf_base_url: "https://huggingface.co/nomic-ai/nomic-embed-text-v1.5/resolve/main",
        default_filename: "model_int8.onnx",
        preference_order: NOMIC_PREFERENCE_ORDER,
        dimension: 768,
        description: "General-purpose English text embeddings (137 MB int8, default).",
    },
    SupportedModel {
        name: "jina-embeddings-v2-base-code",
        hf_base_url: "https://huggingface.co/jinaai/jina-embeddings-v2-base-code/resolve/main",
        default_filename: "model_quantized.onnx",
        preference_order: JINA_CODE_PREFERENCE_ORDER,
        dimension: 768,
        description: "Code-specialised embeddings (162 MB int8). \
                       Trained on code/docstring pairs across 30+ languages.",
    },
];

/// Look up a model in [`SUPPORTED_MODELS`] by name.
pub fn model_spec(name: &str) -> io::Result<&'static SupportedModel> {
    SUPPORTED_MODELS
        .iter()
        .find(|m| m.name == name)
        .ok_or_else(|| {
            let known: Vec<&str> = SUPPORTED_MODELS.iter().map(|m| m.name).collect();
            io::Error::other(format!(
                "Unknown embedding model '{name}'. Supported models: {}",
                known.join(", ")
            ))
        })
}

/// Resolve the configured model name and its on-disk directory.
///
/// This is the single source of truth for "which model, and where are its
/// files": every model-aware call site (`coraline doctor`, `coraline
/// status`, `coraline model`, `coraline embed`, semantic search
/// registration) should go through this instead of duplicating the
/// override-else-default pattern.
///
/// Validates `cfg.model` against [`SUPPORTED_MODELS`] (a typo surfaces here,
/// not as a confusing download/load failure downstream). Directory
/// resolution: `cfg.model_dir` override if set, else
/// `~/.config/coraline/models/<model>/`.
pub fn resolve_model_dir(cfg: &VectorsConfig) -> io::Result<(String, PathBuf)> {
    model_spec(&cfg.model)?;
    let dir = cfg
        .model_dir
        .as_deref()
        .map_or_else(|| global_model_dir_for(&cfg.model), PathBuf::from);
    Ok((cfg.model.clone(), dir))
}

/// Find the best available ONNX model file in `model_dir` for `model_name`.
///
/// If `preferred` is `Some(name)`, that exact filename is required (error if
/// absent). Otherwise, the first filename from `model_name`'s preference
/// order that exists in the directory is returned.
pub fn find_model_file(
    model_dir: &Path,
    model_name: &str,
    preferred: Option<&str>,
) -> io::Result<PathBuf> {
    if let Some(name) = preferred {
        let p = model_dir.join(name);
        if p.exists() {
            return Ok(p);
        }
        return Err(io::Error::other(format!(
            "Configured model_file '{name}' not found in {}",
            model_dir.display()
        )));
    }
    let spec = model_spec(model_name)?;
    for name in spec.preference_order {
        let p = model_dir.join(name);
        if p.exists() {
            return Ok(p);
        }
    }
    Err(io::Error::other(format!(
        "No ONNX model file found in {}. \
         Download a model variant (e.g. {}) from {} \
         and copy tokenizer.json alongside it.",
        model_dir.display(),
        spec.default_filename,
        spec.hf_base_url,
    )))
}

// ── HuggingFace download ──────────────────────────────────────────────────────

/// HuggingFace repository base URL for nomic-embed-text-v1.5.
///
/// Retained for callers that don't pass a model name. Prefer looking up
/// `model_spec(name).hf_base_url`.
pub const HF_BASE_URL: &str = "https://huggingface.co/nomic-ai/nomic-embed-text-v1.5/resolve/main";

/// HuggingFace download URL for `tokenizer.json` of `model_name`.
pub fn tokenizer_url_for(model_name: &str) -> io::Result<String> {
    Ok(format!(
        "{}/tokenizer.json",
        model_spec(model_name)?.hf_base_url
    ))
}

/// HuggingFace download URL for `tokenizer_config.json` of `model_name`.
pub fn tokenizer_config_url_for(model_name: &str) -> io::Result<String> {
    Ok(format!(
        "{}/tokenizer_config.json",
        model_spec(model_name)?.hf_base_url
    ))
}

/// HuggingFace download URL for a specific ONNX model variant of `model_name`.
///
/// ONNX files live under the `onnx/` subdirectory in the HF repo.
pub fn model_url_for(model_name: &str, filename: &str) -> io::Result<String> {
    Ok(format!(
        "{}/onnx/{filename}",
        model_spec(model_name)?.hf_base_url
    ))
}

/// HuggingFace download URL for `tokenizer.json` of the default model.
///
/// Retained for callers that don't pass a model name. Prefer [`tokenizer_url_for`].
pub fn tokenizer_url() -> String {
    format!("{HF_BASE_URL}/tokenizer.json")
}

/// HuggingFace download URL for `tokenizer_config.json` of the default model.
///
/// Retained for callers that don't pass a model name. Prefer [`tokenizer_config_url_for`].
pub fn tokenizer_config_url() -> String {
    format!("{HF_BASE_URL}/tokenizer_config.json")
}

/// HuggingFace download URL for a specific ONNX variant of the default model.
///
/// Retained for callers that don't pass a model name. Prefer [`model_url_for`].
pub fn model_url(filename: &str) -> String {
    format!("{HF_BASE_URL}/onnx/{filename}")
}

/// Download a single URL to a local file path with a progress indicator.
///
/// Writes to a `.tmp` sibling first, then renames atomically on success.
/// This avoids leaving a partially-written file if the download is interrupted.
///
/// Available with the `embeddings` and `embeddings-dynamic` features.
#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
pub fn download_to_file(url: &str, dest: &Path, label: &str, quiet: bool) -> io::Result<()> {
    use std::io::{Read, Write};

    let response = ureq::get(url)
        .call()
        .map_err(|e| io::Error::other(format!("GET {url} failed: {e}")))?;

    let body = response.into_body();
    let content_length = body.content_length();

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let tmp = dest.with_extension("tmp");
    {
        let mut file = std::fs::File::create(&tmp)?;
        let mut reader = body.into_reader();
        let mut buf = vec![0u8; 65_536]; // 64 KiB chunks
        let mut downloaded = 0u64;

        loop {
            let n = reader.read(&mut buf).map_err(io::Error::other)?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])?;
            downloaded += n as u64;

            if !quiet {
                match content_length {
                    Some(total) if total > 0 => {
                        let pct = downloaded * 100 / total;
                        print!(
                            "\r  {label}: {pct}% ({}/{} MB)    ",
                            downloaded / 1_000_000,
                            total / 1_000_000
                        );
                    }
                    _ => {
                        print!("\r  {label}: {} MB    ", downloaded / 1_000_000);
                    }
                }
                let _ = std::io::stdout().flush();
            }
        }

        file.flush()?;
        if !quiet {
            println!(
                "\r  {label}: {} MB — done                          ",
                downloaded / 1_000_000
            );
        }
    }

    std::fs::rename(&tmp, dest)?;
    Ok(())
}

/// Download all files needed to run `model_name` into `model_dir`.
///
/// Downloads:
/// - `tokenizer.json` and `tokenizer_config.json` (HF repo root)
/// - `<model_filename>` ONNX weights (HF `onnx/` subdirectory)
///
/// `model_filename` must be one of `model_spec(model_name)?.preference_order`
/// — a filename from a different model family will fail with a clear error
/// before any network request is made.
///
/// Set `skip_existing = true` to skip files that are already present on disk.
///
/// Available with the `embeddings` and `embeddings-dynamic` features.
#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
pub fn download_model(
    model_name: &str,
    model_dir: &Path,
    model_filename: &str,
    skip_existing: bool,
    quiet: bool,
) -> io::Result<()> {
    let spec = model_spec(model_name)?;
    if !spec.preference_order.contains(&model_filename) {
        return Err(io::Error::other(format!(
            "'{model_filename}' is not a known variant of '{model_name}'. Valid variants: {}",
            spec.preference_order.join(", ")
        )));
    }

    std::fs::create_dir_all(model_dir)?;

    // Small files — always at repo root on HF
    let meta_files = [
        ("tokenizer.json", tokenizer_url_for(model_name)?),
        (
            "tokenizer_config.json",
            tokenizer_config_url_for(model_name)?,
        ),
    ];
    for (name, url) in &meta_files {
        let dest = model_dir.join(name);
        if skip_existing && dest.exists() {
            if !quiet {
                println!("  {name}: already present, skipping");
            }
            continue;
        }
        download_to_file(url, &dest, name, quiet)?;
    }

    // ONNX model — under onnx/ on HF
    let dest = model_dir.join(model_filename);
    if skip_existing && dest.exists() {
        if !quiet {
            println!("  {model_filename}: already present, skipping");
        }
    } else {
        let url = model_url_for(model_name, model_filename)?;
        download_to_file(&url, &dest, model_filename, quiet)?;
    }

    Ok(())
}

type AnyError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// ONNX-based vector embedding manager.
pub struct VectorManager {
    session: Session,
    tokenizer: Tokenizer,
    model_name: String,
    /// Name of the output tensor — "sentence_embedding", "pooler_output",
    /// or "last_hidden_state" (requires mean-pooling).
    output_name: String,
    /// Whether the model accepts a `token_type_ids` input.
    has_token_type_ids: bool,
}

impl VectorManager {
    /// Load the manager from an ONNX model file.
    ///
    /// Expects `tokenizer.json` in the same directory as `model_path`.
    /// `model_name` is stored so [`store_embedding`] tags rows with the
    /// model that actually produced them — critical for `search_similar`,
    /// staleness, and coverage filters to stay correct across model
    /// switches.
    pub fn new(model_path: &Path, model_name: &str) -> io::Result<Self> {
        let session = Session::builder()
            .map_err(io::Error::other)?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(io::Error::other)?
            .with_intra_threads(4)
            .map_err(io::Error::other)?
            .commit_from_file(model_path)
            .map_err(io::Error::other)?;

        let tokenizer_path = model_path
            .parent()
            .unwrap_or(model_path)
            .join("tokenizer.json");
        let tokenizer = Tokenizer::from_file(&tokenizer_path).map_err(io::Error::other)?;

        // Detect which output tensor the model produces.
        let output_name = session
            .outputs()
            .iter()
            .find_map(|o| {
                if o.name() == "sentence_embedding" || o.name() == "pooler_output" {
                    Some(o.name().to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "last_hidden_state".to_string());

        let has_token_type_ids = session
            .inputs()
            .iter()
            .any(|i| i.name() == "token_type_ids");

        Ok(Self {
            session,
            tokenizer,
            model_name: model_name.to_string(),
            output_name,
            has_token_type_ids,
        })
    }

    /// Load from a directory, auto-detecting the best available model variant
    /// of `model_name`.
    ///
    /// Uses [`find_model_file`] with no preference, so it picks the first
    /// file from `model_name`'s preference order that exists in `model_dir`.
    pub fn from_dir(model_dir: &Path, model_name: &str) -> io::Result<Self> {
        let model_path = find_model_file(model_dir, model_name, None)?;
        Self::new(&model_path, model_name)
    }

    /// Load using the project's config (falls back to the default model's
    /// global directory).
    ///
    /// Respects `vectors.model`, `vectors.model_dir`, and `vectors.model_file`
    /// from config.toml via [`resolve_model_dir`]. Lazily migrates model
    /// files from the legacy per-project nomic location when the global
    /// nomic directory has no model yet.
    pub fn from_project(project_root: &Path) -> io::Result<Self> {
        let cfg = crate::config::load_toml_config(project_root).unwrap_or_default();
        let (model_name, model_dir) = resolve_model_dir(&cfg.vectors)?;

        maybe_migrate_legacy_model(&global_model_dir(), project_root);

        let model_path =
            find_model_file(&model_dir, &model_name, cfg.vectors.model_file.as_deref())?;
        Self::new(&model_path, &model_name)
    }

    /// Generate a normalised embedding vector for `text`.
    pub fn embed(&mut self, text: &str) -> io::Result<Vec<f32>> {
        self.embed_impl(text).map_err(io::Error::other)
    }

    /// Get the model name.
    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    fn embed_impl(&mut self, text: &str) -> Result<Vec<f32>, AnyError> {
        let encoding = self.tokenizer.encode(text, true)?;
        let seq_len = encoding.get_ids().len().min(MAX_SEQ_LEN);

        let input_ids: Vec<i64> = encoding.get_ids()[..seq_len]
            .iter()
            .map(|&x| i64::from(x))
            .collect();
        let attention_mask: Vec<i64> = encoding.get_attention_mask()[..seq_len]
            .iter()
            .map(|&x| i64::from(x))
            .collect();
        let token_type_ids = vec![0i64; seq_len];

        let input_ids_arr = Array2::from_shape_vec((1, seq_len), input_ids)?;
        let attn_arr = Array2::from_shape_vec((1, seq_len), attention_mask.clone())?;
        let tti_arr = Array2::from_shape_vec((1, seq_len), token_type_ids)?;

        let outputs = if self.has_token_type_ids {
            self.session.run(inputs![
                "input_ids"      => TensorRef::from_array_view(&input_ids_arr)?,
                "attention_mask" => TensorRef::from_array_view(&attn_arr)?,
                "token_type_ids" => TensorRef::from_array_view(&tti_arr)?,
            ])?
        } else {
            self.session.run(inputs![
                "input_ids"      => TensorRef::from_array_view(&input_ids_arr)?,
                "attention_mask" => TensorRef::from_array_view(&attn_arr)?,
            ])?
        };

        let embedding: Vec<f32> = if self.output_name == "last_hidden_state" {
            let arr = outputs["last_hidden_state"].try_extract_array::<f32>()?;
            mean_pool(
                arr.as_slice().ok_or("non-contiguous tensor")?,
                arr.shape(),
                &attention_mask,
            )
        } else {
            let arr = outputs[self.output_name.as_str()].try_extract_array::<f32>()?;
            arr.iter().copied().collect()
        };

        Ok(l2_normalize(embedding))
    }
}

/// Shared model directory: `$XDG_CONFIG_HOME/coraline/models/<model_name>/`.
///
/// Falls back to `~/.config/coraline/models/<model_name>/` when
/// `XDG_CONFIG_HOME` is not set.
pub fn global_model_dir_for(model_name: &str) -> PathBuf {
    crate::config::global_config_dir()
        .join("models")
        .join(model_name)
}

/// Shared model directory for the default (nomic) model.
///
/// Retained for callers that don't pass a model name. Prefer [`global_model_dir_for`].
pub fn global_model_dir() -> PathBuf {
    global_model_dir_for(DEFAULT_MODEL)
}

/// Default model directory for a project.
///
/// Delegates to [`global_model_dir`]; `_project_root` is kept for backward
/// compatibility but is no longer used.
pub fn default_model_dir(_project_root: &Path) -> PathBuf {
    global_model_dir()
}

/// Copy model files from the legacy per-project location to the shared global directory.
///
/// Checks `<project_root>/.coraline/models/nomic-embed-text-v1.5/` for ONNX and
/// tokenizer files. If `global_dir` already contains any model file the function
/// returns immediately. Prints one informational message when files are copied so
/// users know where the model now lives and how to clean up the old location.
///
/// This is deliberately nomic-only: the legacy per-project model layout
/// predates multi-model support entirely, so there is nothing to migrate
/// for any other model.
pub fn maybe_migrate_legacy_model(global_dir: &Path, project_root: &Path) {
    let already_present = MODEL_PREFERENCE_ORDER
        .iter()
        .any(|name| global_dir.join(name).exists());

    if already_present {
        return;
    }

    let legacy_dir = project_root
        .join(".coraline")
        .join("models")
        .join(DEFAULT_MODEL);

    if !legacy_dir.exists() {
        return;
    }

    let has_model = MODEL_PREFERENCE_ORDER
        .iter()
        .any(|name| legacy_dir.join(name).exists());

    if !has_model {
        return;
    }

    if let Err(e) = std::fs::create_dir_all(global_dir) {
        tracing::warn!(
            "Model migration: could not create {}: {e}",
            global_dir.display()
        );
        return;
    }

    let files_to_copy: Vec<&str> = MODEL_PREFERENCE_ORDER
        .iter()
        .copied()
        .chain(["tokenizer.json", "tokenizer_config.json"])
        .filter(|name| legacy_dir.join(name).exists())
        .collect();

    let mut copied = 0usize;

    for name in &files_to_copy {
        let src = legacy_dir.join(name);
        let dst = global_dir.join(name);

        if dst.exists() {
            continue;
        }

        match std::fs::copy(&src, &dst) {
            Ok(_) => copied += 1,
            Err(e) => tracing::warn!("Model migration: failed to copy {name}: {e}"),
        }
    }

    if copied > 0 {
        println!(
            "Migrated embedding model to shared location ({copied} file(s) copied).\n  \
             From: {}\n  \
             To:   {}\n  \
             You can remove the old directory with: rm -rf {}",
            legacy_dir.display(),
            global_dir.display(),
            legacy_dir.display(),
        );
    }
}

/// Mean-pool the last hidden state over non-masked positions.
///
/// `slice` is the flat row-major data of a `[1, seq_len, hidden_dim]` tensor.
fn mean_pool(slice: &[f32], shape: &[usize], attention_mask: &[i64]) -> Vec<f32> {
    let (seq_len, hidden_dim) = (shape[1], shape[2]);
    let mut pooled = vec![0.0f32; hidden_dim];
    let mut count = 0.0f32;

    for t in 0..seq_len {
        if attention_mask.get(t).copied().unwrap_or(0) == 0 {
            continue;
        }
        count += 1.0;
        let offset = t * hidden_dim;
        for (d, p) in pooled.iter_mut().enumerate() {
            *p += slice[offset + d];
        }
    }

    if count > 0.0 {
        for v in &mut pooled {
            *v /= count;
        }
    }
    pooled
}

/// L2-normalise a vector in place and return it.
///
/// Uses fused multiply-add for improved numerical stability.
fn l2_normalize(mut v: Vec<f32>) -> Vec<f32> {
    let norm: f32 = v
        .iter()
        .fold(0.0_f32, |acc: f32, &x| x.mul_add(x, acc))
        .sqrt();
    if norm > 1e-9 {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}

/// Build the text to embed for a node: name + qualified name + docstring + signature.
pub fn node_embed_text(
    name: &str,
    qualified_name: &str,
    docstring: Option<&str>,
    signature: Option<&str>,
) -> String {
    let mut parts = vec![name.to_string()];
    if qualified_name != name {
        parts.push(qualified_name.to_string());
    }
    if let Some(doc) = docstring {
        parts.push(doc.to_string());
    }
    if let Some(sig) = signature {
        parts.push(sig.to_string());
    }
    parts.join(" | ")
}

/// Store an embedding vector for a node in the database.
///
/// # Arguments
///
/// * `conn` - Database connection
/// * `node_id` - ID of the node
/// * `embedding` - The embedding vector
/// * `model_name` - Name of the model used to generate the embedding
pub fn store_embedding(
    conn: &Connection,
    node_id: &str,
    embedding: &[f32],
    model_name: &str,
) -> io::Result<()> {
    // Convert f32 slice to bytes
    let embedding_bytes: Vec<u8> = embedding.iter().flat_map(|&f| f.to_le_bytes()).collect();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| io::Error::other(format!("Failed to get system time: {}", e)))?
        .as_millis() as i64;

    conn.execute(
        "INSERT OR REPLACE INTO vectors (node_id, embedding, model, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![node_id, embedding_bytes, model_name, now],
    )
    .map_err(|e| io::Error::other(format!("Failed to store embedding: {}", e)))?;

    Ok(())
}

/// Load an embedding vector from the database.
///
/// # Arguments
///
/// * `conn` - Database connection
/// * `node_id` - ID of the node
///
/// # Returns
///
/// The embedding vector or None if not found.
pub fn load_embedding(conn: &Connection, node_id: &str) -> io::Result<Option<Vec<f32>>> {
    let mut stmt = conn
        .prepare("SELECT embedding FROM vectors WHERE node_id = ?1")
        .map_err(|e| io::Error::other(format!("Failed to prepare query: {}", e)))?;

    let mut rows = stmt
        .query(params![node_id])
        .map_err(|e| io::Error::other(format!("Failed to query: {}", e)))?;

    match rows.next().map_err(io::Error::other)? {
        Some(row) => {
            let bytes: Vec<u8> = row.get(0).map_err(io::Error::other)?;

            // Convert bytes back to f32 slice
            let embedding: Vec<f32> = bytes
                .chunks_exact(4)
                .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect();
            Ok(Some(embedding))
        }
        None => Ok(None),
    }
}

/// Calculate cosine similarity between two vectors.
///
/// Uses fused multiply-add for improved numerical stability.
///
/// # Arguments
///
/// * `a` - First vector
/// * `b` - Second vector
///
/// # Returns
///
/// Cosine similarity in range [-1, 1], where 1 means identical direction.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot = a
        .iter()
        .zip(b.iter())
        .fold(0.0_f32, |acc: f32, (&x, &y)| x.mul_add(y, acc));
    let norm_a = a
        .iter()
        .fold(0.0_f32, |acc: f32, &x| x.mul_add(x, acc))
        .sqrt();
    let norm_b = b
        .iter()
        .fold(0.0_f32, |acc: f32, &y| y.mul_add(y, acc))
        .sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot / (norm_a * norm_b)
}

/// Search for nodes similar to the query embedding.
///
/// # Arguments
///
/// * `conn` - Database connection
/// * `query_embedding` - The query embedding vector
/// * `model` - Model name to restrict results to (matches the `vectors.model`
///   column stored by [`store_embedding`]); prevents comparing a query
///   embedding against rows written by a different, previously-configured
///   model
/// * `limit` - Maximum number of results to return
/// * `min_similarity` - Minimum cosine similarity threshold (0.0 to 1.0)
///
/// # Returns
///
/// A vector of SearchResult ordered by similarity (highest first).
pub fn search_similar(
    conn: &Connection,
    query_embedding: &[f32],
    model: &str,
    limit: usize,
    min_similarity: f32,
) -> io::Result<Vec<SearchResult>> {
    let mut stmt = conn
        .prepare(
            "SELECT v.node_id, v.embedding,
                         n.id, n.kind, n.name, n.qualified_name, n.file_path, n.language,
                         n.start_line, n.end_line, n.start_column, n.end_column,
                         n.docstring, n.signature, n.visibility,
                         n.is_exported, n.is_async, n.is_static, n.is_abstract,
                         n.decorators, n.type_parameters
                  FROM vectors v
                  JOIN nodes n ON v.node_id = n.id
                  WHERE v.model = ?1",
        )
        .map_err(|e| io::Error::other(format!("Failed to prepare query: {}", e)))?;

    let rows = stmt
        .query_map(params![model], |row| {
            let embedding_bytes: Vec<u8> = row.get(1)?;

            // Convert bytes to f32 vector
            let embedding: Vec<f32> = embedding_bytes
                .chunks_exact(4)
                .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect();

            let similarity = cosine_similarity(query_embedding, &embedding);

            // Parse node from row (offset by 2 since we have node_id and embedding first)
            use crate::types::{Language, Node, NodeKind};

            let node = Node {
                id: row.get(2)?,
                kind: serde_json::from_str(&format!("\"{}\"", row.get::<_, String>(3)?))
                    .unwrap_or(NodeKind::Function),
                name: row.get(4)?,
                qualified_name: row.get(5)?,
                file_path: row.get(6)?,
                language: serde_json::from_str(&format!("\"{}\"", row.get::<_, String>(7)?))
                    .unwrap_or(Language::TypeScript),
                start_line: row.get(8)?,
                end_line: row.get(9)?,
                start_column: row.get(10)?,
                end_column: row.get(11)?,
                docstring: row.get(12)?,
                signature: row.get(13)?,
                visibility: row
                    .get::<_, Option<String>>(14)?
                    .and_then(|s| serde_json::from_str(&format!("\"{}\"", s)).ok()),
                is_exported: row.get(15)?,
                is_async: row.get(16)?,
                is_static: row.get(17)?,
                is_abstract: row.get(18)?,
                decorators: row
                    .get::<_, Option<String>>(19)?
                    .and_then(|s| serde_json::from_str(&s).ok()),
                type_parameters: row
                    .get::<_, Option<String>>(20)?
                    .and_then(|s| serde_json::from_str(&s).ok()),
                updated_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?
                    .as_millis() as i64,
            };

            Ok((similarity, node))
        })
        .map_err(|e| io::Error::other(format!("Failed to execute query: {}", e)))?;

    let mut results: Vec<_> = rows
        .filter_map(|r| r.ok())
        .filter(|(sim, _)| *sim >= min_similarity)
        .collect();

    // Sort by similarity (highest first)
    results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    // Take top N and convert to SearchResult
    Ok(results
        .into_iter()
        .take(limit)
        .map(|(similarity, node)| SearchResult {
            node,
            score: similarity,
            highlights: None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Property-based tests ──────────────────────────────────────────────────

    #[cfg(test)]
    mod props {
        use super::*;
        use proptest::prelude::*;

        // Generate non-empty vectors of bounded f32 values (avoids norm overflow).
        fn finite_vec(max_len: usize) -> impl Strategy<Value = Vec<f32>> {
            prop::collection::vec(-1000.0f32..=1000.0f32, 1..=max_len)
        }

        proptest! {
            /// Cosine similarity is symmetric: sim(a, b) == sim(b, a)
            #[test]
            fn prop_cosine_symmetry(a in finite_vec(16), b in finite_vec(16)) {
                if a.len() == b.len() {
                    let ab = cosine_similarity(&a, &b);
                    let ba = cosine_similarity(&b, &a);
                    prop_assert!((ab - ba).abs() < 1e-5, "symmetry violated: {} vs {}", ab, ba);
                }
            }

            /// Result is always in [-1, 1] for same-length vectors.
            #[test]
            fn prop_cosine_range(a in finite_vec(16), b in finite_vec(16)) {
                if a.len() == b.len() {
                    let sim = cosine_similarity(&a, &b);
                    prop_assert!(
                        (-1.0 - 1e-5..=1.0 + 1e-5).contains(&sim),
                        "cosine_similarity out of [-1, 1]: {}",
                        sim
                    );
                }
            }

            /// A non-zero vector has self-similarity of 1.0.
            #[test]
            fn prop_cosine_self_similarity(a in finite_vec(16)) {
                let all_zero = a.iter().all(|&x| x == 0.0);
                if !all_zero {
                    let sim = cosine_similarity(&a, &a);
                    prop_assert!(
                        (sim - 1.0).abs() < 1e-5,
                        "self-similarity should be 1.0 but got {}",
                        sim
                    );
                }
            }

            /// Mismatched lengths always return 0.0.
            #[test]
            fn prop_cosine_different_lengths(
                a in finite_vec(8),
                extra in finite_vec(8),
            ) {
                let mut b = a.clone();
                b.extend_from_slice(&extra);
                // b is strictly longer than a
                let sim = cosine_similarity(&a, &b);
                prop_assert_eq!(sim, 0.0, "different-length vectors should return 0.0");
            }

            /// Scaling a vector does not change its cosine similarity with another.
            #[test]
            fn prop_cosine_scale_invariant(
                a in finite_vec(8),
                b in finite_vec(8),
                scale in 0.1f32..100.0f32,
            ) {
                if a.len() == b.len() {
                    let sim_orig = cosine_similarity(&a, &b);
                    let scaled: Vec<f32> = a.iter().map(|&x| x * scale).collect();
                    let sim_scaled = cosine_similarity(&scaled, &b);
                    prop_assert!(
                        (sim_orig - sim_scaled).abs() < 1e-4,
                        "scale invariance violated: {} vs {}",
                        sim_orig,
                        sim_scaled
                    );
                }
            }
        }
    }

    // ── Unit tests ────────────────────────────────────────────────────────────

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 0.0001);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 0.0).abs() < 0.0001);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![-1.0, -2.0, -3.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - (-1.0)).abs() < 0.0001);
    }

    #[test]
    fn test_cosine_similarity_different_lengths() {
        let a = vec![1.0, 2.0];
        let b = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0);
    }

    #[test]
    fn test_cosine_similarity_empty() {
        let a: Vec<f32> = vec![];
        let b: Vec<f32> = vec![];
        let sim = cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0);
    }

    // ── Model registry ───────────────────────────────────────────────────────

    #[test]
    fn model_spec_finds_known_models() {
        assert!(model_spec(DEFAULT_MODEL).is_ok());
        assert!(model_spec("jina-embeddings-v2-base-code").is_ok());
    }

    #[test]
    fn model_spec_errors_on_unknown_model_with_known_list() {
        let err = model_spec("not-a-real-model").unwrap_err().to_string();
        assert!(err.contains("not-a-real-model"));
        assert!(err.contains(DEFAULT_MODEL));
        assert!(err.contains("jina-embeddings-v2-base-code"));
    }

    #[test]
    fn resolve_model_dir_uses_configured_model_by_default() {
        let cfg = VectorsConfig {
            model: "jina-embeddings-v2-base-code".to_string(),
            model_dir: None,
            ..VectorsConfig::default()
        };
        let (name, dir) = resolve_model_dir(&cfg).unwrap();
        assert_eq!(name, "jina-embeddings-v2-base-code");
        assert!(dir.ends_with("jina-embeddings-v2-base-code"));
    }

    #[test]
    fn resolve_model_dir_respects_model_dir_override() {
        let cfg = VectorsConfig {
            model: DEFAULT_MODEL.to_string(),
            model_dir: Some("/tmp/custom-model-dir".to_string()),
            ..VectorsConfig::default()
        };
        let (name, dir) = resolve_model_dir(&cfg).unwrap();
        assert_eq!(name, DEFAULT_MODEL);
        assert_eq!(dir, PathBuf::from("/tmp/custom-model-dir"));
    }

    #[test]
    fn resolve_model_dir_errors_on_unknown_model() {
        let cfg = VectorsConfig {
            model: "not-a-real-model".to_string(),
            ..VectorsConfig::default()
        };
        assert!(resolve_model_dir(&cfg).is_err());
    }

    #[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
    #[test]
    fn download_model_rejects_variant_from_a_different_model_family() {
        let dir = std::env::temp_dir().join("coraline-test-download-model-mismatch");
        // A nomic-only filename passed for the jina model must be rejected
        // before any network request is attempted (no files should appear).
        let err = download_model(
            "jina-embeddings-v2-base-code",
            &dir,
            "model_int8.onnx",
            true,
            true,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("model_int8.onnx"));
        assert!(err.contains("jina-embeddings-v2-base-code"));
        assert!(!dir.exists());
    }
}
