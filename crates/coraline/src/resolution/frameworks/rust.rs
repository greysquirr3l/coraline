#![forbid(unsafe_code)]

//! Rust module-path resolution.
//!
//! Handles `crate::`, `super::`, and `self::` qualified references by
//! mapping them to source files relative to `src/` or the calling file.

use std::path::{Path, PathBuf};

use super::{FrameworkResolver, ResolveContext};

pub struct RustResolver;

impl FrameworkResolver for RustResolver {
    fn name(&self) -> &'static str {
        "rust"
    }

    fn detect(&self, project_root: &Path) -> bool {
        project_root.join("Cargo.toml").exists()
    }

    fn resolve_to_paths(&self, ctx: &ResolveContext<'_>) -> Vec<PathBuf> {
        let name = ctx.reference_name;

        // `crate::foo::bar::Sym` → src/foo/bar.rs or src/foo/bar/mod.rs
        if let Some(path_part) = name.strip_prefix("crate::") {
            return resolve_crate_path(ctx.project_root, path_part);
        }

        // `super::foo::Sym` → parent directory of from_file
        if let Some(path_part) = name.strip_prefix("super::") {
            return resolve_relative_path(ctx.from_file, path_part, 1);
        }

        // `self::foo::Sym`
        if let Some(path_part) = name.strip_prefix("self::") {
            return resolve_relative_path(ctx.from_file, path_part, 0);
        }

        // Plain module name → look for adjacent module file
        if is_valid_module_name(name) {
            return resolve_adjacent_module(ctx.from_file, name);
        }

        Vec::new()
    }
}

fn resolve_crate_path(project_root: &Path, path_part: &str) -> Vec<PathBuf> {
    let segments: Vec<&str> = path_part.split("::").collect();
    if segments.is_empty() {
        return Vec::new();
    }
    // Drop the final segment (symbol name) to get the module path
    let module_segs = if segments.len() > 1 {
        &segments[..segments.len() - 1]
    } else {
        &segments[..]
    };

    let rel: PathBuf = module_segs.iter().collect();
    let src = project_root.join("src");

    let mut out = Vec::new();
    let rs = src.join(&rel).with_extension("rs");
    if rs.exists() {
        out.push(rs);
    }
    let mod_rs = src.join(&rel).join("mod.rs");
    if mod_rs.exists() {
        out.push(mod_rs);
    }
    out
}

fn resolve_relative_path(from_file: &str, path_part: &str, up_levels: usize) -> Vec<PathBuf> {
    let from = Path::new(from_file);
    let mut base = from.parent().unwrap_or(Path::new(""));
    for _ in 0..up_levels {
        base = base.parent().unwrap_or(base);
    }

    let segments: Vec<&str> = path_part.split("::").collect();
    if segments.is_empty() {
        return Vec::new();
    }
    let module_segs = if segments.len() > 1 {
        &segments[..segments.len() - 1]
    } else {
        &segments[..]
    };

    let rel: PathBuf = module_segs.iter().collect();
    let mut out = Vec::new();

    let rs = base.join(&rel).with_extension("rs");
    if rs.exists() {
        out.push(rs);
    }
    let mod_rs = base.join(&rel).join("mod.rs");
    if mod_rs.exists() {
        out.push(mod_rs);
    }
    out
}

fn resolve_adjacent_module(from_file: &str, module_name: &str) -> Vec<PathBuf> {
    let Some(parent) = Path::new(from_file).parent() else {
        return Vec::new();
    };
    let mut out = Vec::new();

    let rs = parent.join(format!("{module_name}.rs"));
    if rs.exists() {
        out.push(rs);
    }
    let mod_rs = parent.join(module_name).join("mod.rs");
    if mod_rs.exists() {
        out.push(mod_rs);
    }
    out
}

fn is_valid_module_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains("::")
        && !name.contains('.')
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}
