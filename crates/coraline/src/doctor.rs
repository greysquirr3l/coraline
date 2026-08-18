#![forbid(unsafe_code)]

//! Diagnostic checks for the Coraline installation.
//!
//! [`run_all`] builds a [`Report`] of [`Probe`]s covering config, database,
//! git hooks, and model presence. With `deep = true`, three additional slow
//! probes are added: ONNX model load, inference, and embedding-coverage count.
//!
//! The probe contract is intentionally narrow:
//!
//! - `name` — a short stable identifier (used as a JSON key and a human label).
//! - `ok` — pass/fail.
//! - `detail` — a one-line human-readable description; the user can read this
//!   without leaving the terminal.
//! - `fix` — when `ok` is false, a remediation hint that points at the
//!   next command the user should run.
//!
//! Ordering is stable so the output is deterministic.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config;
use crate::db;
use crate::sync::GitHooksManager;
use crate::vectors;

/// One diagnostic check.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Probe {
    /// Short stable identifier (used as JSON key and human label).
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
    /// Optional remediation hint. Skipped from JSON when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

/// Result of a `doctor` run.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Report {
    pub probes: Vec<Probe>,
    /// Shell exit code: 0 if all probes passed, 1 otherwise.
    pub exit_code: i32,
}

impl Report {
    /// Build a `Report` from probes, deriving `exit_code` from probe results.
    pub fn from_probes(probes: Vec<Probe>) -> Self {
        let exit_code = i32::from(!probes.iter().all(|p| p.ok));
        Self { probes, exit_code }
    }
}

/// Run all probes. With `deep = true`, include the slow model probes.
pub fn run_all(project_root: &Path, deep: bool) -> Report {
    let mut probes = vec![
        check_config(project_root),
        check_db(project_root),
        check_hooks(project_root),
        check_model_presence(project_root),
    ];
    if deep {
        #[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
        {
            probes.push(check_model_load(project_root));
            probes.push(check_model_inference(project_root));
            probes.push(check_embed_coverage(project_root));
        }
    }
    Report::from_probes(probes)
}

// ─── Cheap probes ────────────────────────────────────────────────────────────

/// Check that the active `config.toml` is present and readable.
pub fn check_config(project_root: &Path) -> Probe {
    let cfg_path = config::toml_config_path(project_root);
    match std::fs::read_to_string(&cfg_path) {
        Ok(content) => Probe {
            name: "config",
            ok: true,
            detail: format!("{} ({} bytes)", cfg_path.display(), content.len()),
            fix: None,
        },
        Err(e) => Probe {
            name: "config",
            ok: false,
            detail: format!("{} not readable: {}", cfg_path.display(), e),
            fix: Some("Run `coraline init` to create config.toml.".to_string()),
        },
    }
}

/// Check that the `SQLite` database can be opened and has stats.
pub fn check_db(project_root: &Path) -> Probe {
    let db_path = db::database_path(project_root);
    match db::open_database(project_root) {
        Ok(conn) => {
            let detail = match db::get_db_stats(&conn) {
                Ok(s) => format!(
                    "{} ({} nodes, {} edges, {} files)",
                    db_path.display(),
                    s.node_count,
                    s.edge_count,
                    s.file_count
                ),
                Err(e) => format!("{} (open ok, stats failed: {e})", db_path.display()),
            };
            Probe {
                name: "database",
                ok: true,
                detail,
                fix: None,
            }
        }
        Err(e) => Probe {
            name: "database",
            ok: false,
            detail: format!("{} not openable: {}", db_path.display(), e),
            fix: Some("Run `coraline init` to create the database.".to_string()),
        },
    }
}

/// Check that the git post-commit hook is installed (or n/a for non-git repos).
pub fn check_hooks(project_root: &Path) -> Probe {
    let hooks = GitHooksManager::new(project_root);
    let is_git = hooks.is_git_repository();
    let installed = hooks.is_hook_installed();
    let (ok, detail) = match (is_git, installed) {
        (false, _) => (true, "not a git repository".to_string()),
        (true, true) => (true, "installed".to_string()),
        (true, false) => (false, "not installed".to_string()),
    };
    let fix = if ok {
        None
    } else {
        Some("Run `coraline hooks install`.".to_string())
    };
    Probe {
        name: "git hooks",
        ok,
        detail,
        fix,
    }
}

/// Check that at least one model variant from `MODEL_PREFERENCE_ORDER` is
/// present on disk.
pub fn check_model_presence(project_root: &Path) -> Probe {
    let model_dir = resolve_status_model_dir(project_root);
    match compute_model_state(&model_dir) {
        ModelState::Present { name, size_bytes } => {
            let size_mb = size_bytes / 1_000_000;
            Probe {
                name: "model file",
                ok: true,
                detail: format!("{name} ({size_mb} MB)"),
                fix: None,
            }
        }
        ModelState::Absent => Probe {
            name: "model file",
            ok: false,
            detail: "no model file in MODEL_PREFERENCE_ORDER".to_string(),
            fix: Some("Run `coraline model download`.".to_string()),
        },
    }
}

// ─── Deep probes (cfg-gated) ──────────────────────────────────────────────────

/// Try to instantiate the ONNX `VectorManager`. This is the slowest cheap
/// check (~340 ms cold) because it loads the model into memory.
#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
pub fn check_model_load(project_root: &Path) -> Probe {
    let start = std::time::Instant::now();
    match vectors::VectorManager::from_project(project_root) {
        Ok(_) => Probe {
            name: "model loads",
            ok: true,
            detail: format!("loaded in {:?}", start.elapsed()),
            fix: None,
        },
        Err(e) => Probe {
            name: "model loads",
            ok: false,
            detail: format!("load failed: {e}"),
            fix: Some("Run `coraline model download`.".to_string()),
        },
    }
}

/// Run a single sample embedding through the model to verify the inference
/// path works end-to-end.
#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
pub fn check_model_inference(project_root: &Path) -> Probe {
    let mut vm = match vectors::VectorManager::from_project(project_root) {
        Ok(vm) => vm,
        Err(e) => {
            return Probe {
                name: "inference",
                ok: false,
                detail: format!("model not loaded: {e}"),
                fix: None,
            };
        }
    };
    let sample = "fn add(a: i32, b: i32) -> i32 { a + b }";
    match vm.embed(sample) {
        Ok(embedding) if !embedding.is_empty() => Probe {
            name: "inference",
            ok: true,
            detail: format!("{}d embedding returned", embedding.len()),
            fix: None,
        },
        Ok(_) => Probe {
            name: "inference",
            ok: false,
            detail: "empty embedding returned".to_string(),
            fix: None,
        },
        Err(e) => Probe {
            name: "inference",
            ok: false,
            detail: format!("embed failed: {e}"),
            fix: None,
        },
    }
}

/// Compare node count to embedded-node count (rows in the `vectors` table).
#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
pub fn check_embed_coverage(project_root: &Path) -> Probe {
    let conn = match db::open_database(project_root) {
        Ok(c) => c,
        Err(e) => {
            return Probe {
                name: "embed coverage",
                ok: false,
                detail: format!("db open failed: {e}"),
                fix: None,
            };
        }
    };
    let total = match db::get_all_nodes(&conn) {
        Ok(nodes) => nodes.len(),
        Err(e) => {
            return Probe {
                name: "embed coverage",
                ok: false,
                detail: format!("node query failed: {e}"),
                fix: None,
            };
        }
    };
    let unembedded = match db::get_unembedded_nodes(&conn) {
        Ok(nodes) => nodes.len(),
        Err(e) => {
            return Probe {
                name: "embed coverage",
                ok: false,
                detail: format!("unembedded-node query failed: {e}"),
                fix: None,
            };
        }
    };
    let embedded = total.saturating_sub(unembedded);
    let ok = total == 0 || embedded == total;
    Probe {
        name: "embed coverage",
        ok,
        detail: format!("{embedded}/{total} nodes embedded"),
        fix: if ok {
            None
        } else {
            Some("Run `coraline embed` to fill vectors.".to_string())
        },
    }
}

// ─── Model state helpers (shared with `coraline status`) ─────────────────────

/// Embedding-model state, as surfaced by `coraline status`.
#[derive(Debug, PartialEq, Eq)]
pub enum ModelState {
    /// At least one variant in `MODEL_PREFERENCE_ORDER` is present.
    Present { name: String, size_bytes: u64 },
    /// No model file exists in the directory.
    Absent,
}

/// Pure lookup of which (if any) model variant is present on disk.
pub fn compute_model_state(model_dir: &Path) -> ModelState {
    for name in vectors::MODEL_PREFERENCE_ORDER {
        let p = model_dir.join(name);
        if let Ok(meta) = std::fs::metadata(&p) {
            return ModelState::Present {
                name: (*name).to_string(),
                size_bytes: meta.len(),
            };
        }
    }
    ModelState::Absent
}

/// Resolve the embedding-model directory for the given project.
///
/// Honours `vectors.model_dir` in `config.toml` when set; falls back to the
/// global default (`~/.config/coraline/models/nomic-embed-text-v1.5/`).
pub fn resolve_status_model_dir(project_root: &Path) -> PathBuf {
    let cfg = config::load_toml_config(project_root).unwrap_or_default();
    cfg.vectors
        .model_dir
        .map_or_else(|| vectors::default_model_dir(project_root), PathBuf::from)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn empty_temp() -> Result<TempDir, Box<dyn std::error::Error>> {
        Ok(tempfile::TempDir::new()?)
    }

    #[test]
    fn config_missing_when_no_init() -> TestResult {
        let root = empty_temp()?;
        let probe = check_config(root.path());
        assert!(!probe.ok);
        let Some(fix) = probe.fix.as_ref() else {
            return Err("expected fix on missing config".into());
        };
        assert!(fix.contains("init"));
        Ok(())
    }

    #[test]
    fn config_ok_when_present() -> TestResult {
        let root = empty_temp()?;
        fs::create_dir_all(root.path().join(".coraline"))?;
        let cfg_path = root.path().join(".coraline/config.toml");
        fs::write(&cfg_path, "[vectors]\n")?;
        let probe = check_config(root.path());
        if !probe.ok {
            return Err(format!("probe not ok: {probe:?}").into());
        }
        assert!(probe.detail.contains("bytes"));
        Ok(())
    }

    #[test]
    fn db_missing_when_no_init() -> TestResult {
        let root = empty_temp()?;
        let probe = check_db(root.path());
        assert!(!probe.ok);
        assert!(probe.fix.is_some());
        Ok(())
    }

    #[test]
    fn db_ok_when_initialized() -> TestResult {
        let root = empty_temp()?;
        db::initialize_database(root.path())?;
        let probe = check_db(root.path());
        assert!(probe.ok);
        assert!(probe.detail.contains("nodes"));
        Ok(())
    }

    #[test]
    fn hooks_no_git_is_ok() -> TestResult {
        let root = empty_temp()?;
        let probe = check_hooks(root.path());
        assert!(probe.ok);
        assert!(probe.detail.contains("not a git"));
        Ok(())
    }

    #[test]
    fn model_presence_absent_when_no_files() -> TestResult {
        let root = empty_temp()?;
        // Pin the model dir to the temp project so the global default can't
        // leak a model file across tests.
        let coraline_dir = root.path().join(".coraline");
        fs::create_dir_all(&coraline_dir)?;
        let models_dir = root.path().join("models");
        let config = format!("[vectors]\nmodel_dir = \"{}\"\n", models_dir.display());
        fs::write(coraline_dir.join("config.toml"), config)?;
        let probe = check_model_presence(root.path());
        assert!(!probe.ok);
        let Some(fix) = probe.fix.as_ref() else {
            return Err("expected fix on missing model".into());
        };
        assert!(fix.contains("model download"));
        Ok(())
    }

    #[test]
    fn model_presence_ok_when_file_exists() -> TestResult {
        let root = empty_temp()?;
        let coraline_dir = root.path().join(".coraline");
        fs::create_dir_all(&coraline_dir)?;
        let models_dir = root.path().join("models");
        fs::create_dir_all(&models_dir)?;
        fs::write(models_dir.join("model_int8.onnx"), vec![0u8; 1_000_000])?;
        let config = format!("[vectors]\nmodel_dir = \"{}\"\n", models_dir.display());
        fs::write(coraline_dir.join("config.toml"), config)?;
        let probe = check_model_presence(root.path());
        assert_eq!(probe.name, "model file");
        assert!(probe.ok);
        Ok(())
    }

    #[test]
    fn model_state_absent_when_empty_dir() -> TestResult {
        let root = empty_temp()?;
        let state = compute_model_state(root.path());
        assert_eq!(state, ModelState::Absent);
        Ok(())
    }

    #[test]
    fn model_state_present_picks_first_preferred() -> TestResult {
        let root = empty_temp()?;
        fs::write(root.path().join("model_int8.onnx"), vec![0u8; 42])?;
        let state = compute_model_state(root.path());
        assert_eq!(
            state,
            ModelState::Present {
                name: "model_int8.onnx".to_string(),
                size_bytes: 42,
            }
        );
        Ok(())
    }

    #[test]
    fn resolve_status_returns_a_non_empty_path() -> TestResult {
        // We can't assert a specific default model here because `load_toml_config`
        // merges in the user's global `~/.config/coraline/config.toml`, which
        // may pin `model_dir` to anything. The XDG-based `global_config_dir()`
        // is also not cross-platform (see config.rs). The override path is
        // covered separately; here we just verify the function returns a path.
        let root = empty_temp()?;
        let resolved = resolve_status_model_dir(root.path());
        assert!(resolved.file_name().is_some());
        Ok(())
    }

    #[test]
    fn resolve_status_override() -> TestResult {
        let root = empty_temp()?;
        let coraline_dir = root.path().join(".coraline");
        fs::create_dir_all(&coraline_dir)?;
        let custom = root.path().join("custom-models");
        fs::create_dir_all(&custom)?;
        let config = format!("[vectors]\nmodel_dir = \"{}\"\n", custom.display());
        fs::write(coraline_dir.join("config.toml"), config)?;
        let resolved = resolve_status_model_dir(root.path());
        assert_eq!(resolved, custom);
        Ok(())
    }

    #[test]
    fn report_exit_code_all_ok() {
        let probes = vec![
            Probe {
                name: "a",
                ok: true,
                detail: "ok".to_string(),
                fix: None,
            },
            Probe {
                name: "b",
                ok: true,
                detail: "ok".to_string(),
                fix: None,
            },
        ];
        let report = Report::from_probes(probes);
        assert_eq!(report.exit_code, 0);
    }

    #[test]
    fn report_exit_code_on_failure() {
        let probes = vec![
            Probe {
                name: "a",
                ok: true,
                detail: "ok".to_string(),
                fix: None,
            },
            Probe {
                name: "b",
                ok: false,
                detail: "fail".to_string(),
                fix: Some("fix".to_string()),
            },
        ];
        let report = Report::from_probes(probes);
        assert_eq!(report.exit_code, 1);
    }

    #[test]
    fn probe_json_omits_fix_when_none() -> TestResult {
        let probe = Probe {
            name: "test",
            ok: true,
            detail: "ok".to_string(),
            fix: None,
        };
        let json = serde_json::to_string(&probe)?;
        assert!(!json.contains("fix"));
        Ok(())
    }

    #[test]
    fn probe_json_includes_fix_when_some() -> TestResult {
        let probe = Probe {
            name: "test",
            ok: false,
            detail: "fail".to_string(),
            fix: Some("coraline init".to_string()),
        };
        let json = serde_json::to_string(&probe)?;
        assert!(json.contains("\"fix\":\"coraline init\""));
        Ok(())
    }

    #[test]
    fn run_all_cheap_probes_in_stable_order() -> TestResult {
        let root = empty_temp()?;
        let report = run_all(root.path(), false);
        let names: Vec<&str> = report.probes.iter().map(|p| p.name).collect();
        assert_eq!(names, vec!["config", "database", "git hooks", "model file"]);
        Ok(())
    }

    /// Deep probes require a real ONNX model on disk. Skipped by default
    /// since a CI test machine may not have one and the `ort` crate
    /// can panic on a missing/invalid model. Run with
    /// `cargo test -- --ignored` to verify locally.
    #[test]
    #[ignore = "requires a valid ONNX model on disk; ort may panic on missing model"]
    fn run_all_deep_includes_three_more_in_stable_order() -> TestResult {
        let root = empty_temp()?;
        let report = run_all(root.path(), true);
        let names: Vec<&str> = report.probes.iter().map(|p| p.name).collect();
        assert_eq!(
            names,
            vec![
                "config",
                "database",
                "git hooks",
                "model file",
                "model loads",
                "inference",
                "embed coverage",
            ]
        );
        Ok(())
    }

    #[test]
    fn report_json_serializes_structurally() -> TestResult {
        let report = Report::from_probes(vec![Probe {
            name: "test",
            ok: true,
            detail: "ok".to_string(),
            fix: None,
        }]);
        let json = serde_json::to_string_pretty(&report)?;
        assert!(json.contains("\"probes\""));
        assert!(json.contains("\"exit_code\": 0"));
        Ok(())
    }
}
