#![forbid(unsafe_code)]

//! React / Next.js reference resolution.
//!
//! Handles relative imports (`./Foo`, `../Bar`), path-aliased imports
//! (`@/components/Button`, `~/utils`), and bare `PascalCase` component
//! names by probing common component directories for matching files.

use std::path::{Path, PathBuf};

use super::{FrameworkResolver, ResolveContext};

const EXTENSIONS: &[&str] = &["tsx", "ts", "jsx", "js"];

pub struct ReactResolver;

impl FrameworkResolver for ReactResolver {
    fn name(&self) -> &'static str {
        "react"
    }

    fn detect(&self, project_root: &Path) -> bool {
        let pkg = project_root.join("package.json");
        if pkg.exists()
            && let Ok(content) = std::fs::read_to_string(&pkg)
            && (content.contains("\"react\"") || content.contains("\"next\""))
        {
            return true;
        }
        project_root.join("next.config.js").exists()
            || project_root.join("next.config.ts").exists()
            || project_root.join("next.config.mjs").exists()
    }

    fn resolve_to_paths(&self, ctx: &ResolveContext<'_>) -> Vec<PathBuf> {
        let name = ctx.reference_name;

        // Relative import: ./Foo, ../utils/helpers
        if name.starts_with("./") || name.starts_with("../") {
            return resolve_relative(ctx.from_file, name);
        }

        // Path aliases: @/ or ~/
        if let Some(rest) = name.strip_prefix("@/").or_else(|| name.strip_prefix("~/")) {
            return resolve_aliased(ctx.project_root, rest);
        }

        // Bare PascalCase → search component directories
        if is_pascal_case(name) {
            return resolve_component(ctx.project_root, name);
        }

        Vec::new()
    }
}

fn resolve_relative(from_file: &str, import_path: &str) -> Vec<PathBuf> {
    let base = Path::new(from_file)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let joined = base.join(import_path);
    probe_with_extensions(&joined)
}

fn resolve_aliased(project_root: &Path, rest: &str) -> Vec<PathBuf> {
    let roots = [
        project_root.join("src"),
        project_root.join("app"),
        project_root.to_path_buf(),
    ];
    let mut out = Vec::new();
    for root in &roots {
        let mut found = probe_with_extensions(&root.join(rest));
        out.append(&mut found);
    }
    out
}

fn resolve_component(project_root: &Path, name: &str) -> Vec<PathBuf> {
    let search_dirs = [
        project_root.join("src").join("components"),
        project_root.join("src").join("app"),
        project_root.join("components"),
        project_root.join("app"),
    ];
    let mut out = Vec::new();
    for dir in &search_dirs {
        if !dir.exists() {
            continue;
        }
        for ext in EXTENSIONS {
            let p = dir.join(format!("{name}.{ext}"));
            if p.exists() {
                out.push(p);
            }
            let index = dir.join(name).join(format!("index.{ext}"));
            if index.exists() {
                out.push(index);
            }
        }
    }
    out
}

/// Given a base path (no extension), probe all known extensions and index
/// files, returning every variant that exists on disk.
fn probe_with_extensions(base: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();

    // Already has a recognised extension
    if base.extension().is_some() && base.exists() {
        out.push(base.to_path_buf());
        return out;
    }

    for ext in EXTENSIONS {
        let p = base.with_extension(ext);
        if p.exists() {
            out.push(p);
        }
    }
    for ext in EXTENSIONS {
        let p = base.join(format!("index.{ext}"));
        if p.exists() {
            out.push(p);
        }
    }
    out
}

fn is_pascal_case(name: &str) -> bool {
    !name.is_empty()
        && !name.contains("::")
        && !name.contains('.')
        && !name.contains('/')
        && name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && name.chars().all(|c| c.is_alphanumeric() || c == '_')
}
