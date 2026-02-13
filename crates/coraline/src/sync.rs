#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};

const POST_COMMIT_HOOK: &str = "post-commit";
const CODEGRAPH_MARKER: &str = "# Coraline auto-sync hook";

fn post_commit_script() -> String {
    let script = r#"#!/bin/sh
# Coraline auto-sync hook
# This hook keeps the graph in sync after each commit.
# To remove: coraline hooks remove

(
  if [ ! -d ".codegraph" ]; then
	exit 0
  fi

    if command -v coraline >/dev/null 2>&1; then
	coraline sync --quiet 2>/dev/null &
    elif command -v cargo >/dev/null 2>&1 && [ -f "Cargo.toml" ]; then
	cargo run -q -p coraline --bin coraline -- sync --quiet 2>/dev/null &
  fi
) &

exit 0
"#;

    script.to_string()
}

#[derive(Debug)]
pub struct HookInstallResult {
    pub success: bool,
    pub hook_path: PathBuf,
    pub message: String,
    pub previous_hook_backed_up: bool,
    pub backup_path: Option<PathBuf>,
}

#[derive(Debug)]
pub struct HookRemoveResult {
    pub success: bool,
    pub message: String,
    pub restored_from_backup: bool,
}

#[derive(Debug)]
pub struct GitHooksManager {
    git_dir: PathBuf,
    hooks_dir: PathBuf,
}

impl GitHooksManager {
    pub fn new(project_root: &Path) -> Self {
        let git_dir = project_root.join(".git");
        let hooks_dir = git_dir.join("hooks");
        Self { git_dir, hooks_dir }
    }

    pub fn is_git_repository(&self) -> bool {
        self.git_dir.is_dir()
    }

    pub fn is_hook_installed(&self) -> bool {
        let hook_path = self.hooks_dir.join(POST_COMMIT_HOOK);
        let content = fs::read_to_string(&hook_path).unwrap_or_default();
        content.contains(CODEGRAPH_MARKER)
    }

    pub fn install_hook(&self) -> HookInstallResult {
        let hook_path = self.hooks_dir.join(POST_COMMIT_HOOK);

        if !self.is_git_repository() {
            return HookInstallResult {
                success: false,
                hook_path,
                message: "Not a git repository. Run git init first.".to_string(),
                previous_hook_backed_up: false,
                backup_path: None,
            };
        }

        if let Err(err) = fs::create_dir_all(&self.hooks_dir) {
            return HookInstallResult {
                success: false,
                hook_path,
                message: format!("Failed to create hooks directory: {err}"),
                previous_hook_backed_up: false,
                backup_path: None,
            };
        }

        let mut previous_hook_backed_up = false;
        let mut backup_path = None;

        if hook_path.exists() {
            let existing = fs::read_to_string(&hook_path).unwrap_or_default();
            if !existing.contains(CODEGRAPH_MARKER) {
                let backup = hook_path.with_extension("coraline-backup");
                if let Err(err) = fs::copy(&hook_path, &backup) {
                    return HookInstallResult {
                        success: false,
                        hook_path,
                        message: format!("Failed to backup existing hook: {err}"),
                        previous_hook_backed_up: false,
                        backup_path: None,
                    };
                }
                previous_hook_backed_up = true;
                backup_path = Some(backup);
            }
        }

        if let Err(err) = fs::write(&hook_path, post_commit_script()) {
            return HookInstallResult {
                success: false,
                hook_path,
                message: format!("Failed to write hook: {err}"),
                previous_hook_backed_up,
                backup_path,
            };
        }

        if let Err(err) = make_executable(&hook_path) {
            return HookInstallResult {
                success: false,
                hook_path,
                message: format!("Failed to set hook permissions: {err}"),
                previous_hook_backed_up,
                backup_path,
            };
        }

        HookInstallResult {
            success: true,
            hook_path,
            message: "Post-commit hook installed.".to_string(),
            previous_hook_backed_up,
            backup_path,
        }
    }

    pub fn remove_hook(&self) -> HookRemoveResult {
        let hook_path = self.hooks_dir.join(POST_COMMIT_HOOK);
        let backup_path = hook_path.with_extension("coraline-backup");

        if !hook_path.exists() {
            return HookRemoveResult {
                success: true,
                message: "No post-commit hook found.".to_string(),
                restored_from_backup: false,
            };
        }

        let content = fs::read_to_string(&hook_path).unwrap_or_default();
        if !content.contains(CODEGRAPH_MARKER) {
            return HookRemoveResult {
                success: false,
                message: "Post-commit hook was not installed by Coraline.".to_string(),
                restored_from_backup: false,
            };
        }

        if let Err(err) = fs::remove_file(&hook_path) {
            return HookRemoveResult {
                success: false,
                message: format!("Failed to remove hook: {err}"),
                restored_from_backup: false,
            };
        }

        if backup_path.exists() {
            if let Err(err) = fs::rename(&backup_path, &hook_path) {
                return HookRemoveResult {
                    success: true,
                    message: format!("Hook removed. Failed to restore backup: {err}"),
                    restored_from_backup: false,
                };
            }
            return HookRemoveResult {
                success: true,
                message: "Hook removed. Previous hook restored.".to_string(),
                restored_from_backup: true,
            };
        }

        HookRemoveResult {
            success: true,
            message: "Hook removed.".to_string(),
            restored_from_backup: false,
        }
    }
}

fn make_executable(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)
    }

    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}
