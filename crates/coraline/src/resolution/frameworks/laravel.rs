#![forbid(unsafe_code)]

//! Laravel / PHP reference resolution.
//!
//! Handles fully-qualified PHP class names (`App\Models\User`),
//! dot-notation view names (`'admin.dashboard'`), and known Laravel
//! facades (`Auth`, `DB`, `Cache`, …).

use std::path::{Path, PathBuf};

use super::{FrameworkResolver, ResolveContext};

pub struct LaravelResolver;

impl FrameworkResolver for LaravelResolver {
    fn name(&self) -> &'static str {
        "laravel"
    }

    fn detect(&self, project_root: &Path) -> bool {
        if project_root.join("artisan").exists() {
            return true;
        }
        let composer = project_root.join("composer.json");
        if composer.exists()
            && let Ok(content) = std::fs::read_to_string(&composer)
        {
            return content.contains("laravel/framework");
        }
        false
    }

    fn resolve_to_paths(&self, ctx: &ResolveContext<'_>) -> Vec<PathBuf> {
        let name = ctx.reference_name;

        // Fully-qualified class: App\Models\User → app/Models/User.php
        if name.contains('\\') {
            return resolve_fqn(ctx.project_root, name);
        }

        // Dot-notation view: 'admin.dashboard' → resources/views/admin/dashboard.blade.php
        if looks_like_view_name(name) {
            return resolve_view(ctx.project_root, name);
        }

        // Known Laravel facade → check app/Facades/ first
        if is_laravel_facade(name) {
            return resolve_facade(ctx.project_root, name);
        }

        Vec::new()
    }
}

/// PSR-4: `App\Models\User` → `app/Models/User.php`
fn resolve_fqn(project_root: &Path, fqn: &str) -> Vec<PathBuf> {
    let parts: Vec<&str> = fqn.split('\\').collect();
    if parts.len() < 2 {
        return Vec::new();
    }

    let base_dir = match parts.first().copied().unwrap_or_default() {
        "App" => project_root.join("app"),
        "Tests" => project_root.join("tests"),
        "Database" => project_root.join("database"),
        other => project_root.join(other.to_lowercase()),
    };

    let mut path = base_dir;
    for part in parts.get(1..parts.len() - 1).unwrap_or(&[]) {
        path = path.join(part);
    }
    let Some(&last) = parts.last() else {
        return Vec::new();
    };
    let file = path.join(format!("{last}.php"));

    if file.exists() {
        vec![file]
    } else {
        Vec::new()
    }
}

/// `'admin.dashboard'` → `resources/views/admin/dashboard.blade.php`
fn resolve_view(project_root: &Path, view_name: &str) -> Vec<PathBuf> {
    let name = view_name.trim_matches(|c| c == '\'' || c == '"');
    let views_dir = project_root.join("resources").join("views");
    if !views_dir.exists() {
        return Vec::new();
    }

    let parts: Vec<&str> = name.split('.').collect();
    let mut path = views_dir;
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            path = path.join(format!("{part}.blade.php"));
        } else {
            path = path.join(part);
        }
    }

    if path.exists() {
        vec![path]
    } else {
        Vec::new()
    }
}

fn resolve_facade(project_root: &Path, facade_name: &str) -> Vec<PathBuf> {
    let candidate = project_root
        .join("app")
        .join("Facades")
        .join(format!("{facade_name}.php"));
    if candidate.exists() {
        vec![candidate]
    } else {
        Vec::new()
    }
}

fn looks_like_view_name(name: &str) -> bool {
    let cleaned = name.trim_matches(|c| c == '\'' || c == '"');
    cleaned.contains('.')
        && cleaned
            .chars()
            .all(|c| c.is_alphanumeric() || c == '.' || c == '_' || c == '-')
        && cleaned.chars().next().is_some_and(char::is_alphanumeric)
        // Avoid matching version strings or semver (e.g. "1.0.0")
        && cleaned.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
}

const LARAVEL_FACADES: &[&str] = &[
    "Auth",
    "Cache",
    "Config",
    "Cookie",
    "Crypt",
    "DB",
    "Event",
    "File",
    "Gate",
    "Hash",
    "Http",
    "Log",
    "Mail",
    "Queue",
    "Redirect",
    "Request",
    "Response",
    "Route",
    "Schema",
    "Session",
    "Storage",
    "URL",
    "Validator",
    "View",
];

fn is_laravel_facade(name: &str) -> bool {
    LARAVEL_FACADES.contains(&name)
}
