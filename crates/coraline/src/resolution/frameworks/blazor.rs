#![forbid(unsafe_code)]

//! Blazor / ASP.NET Core component resolution.
//!
//! Resolves PascalCase component names (e.g. `Counter`, `UserList`) to
//! their `.razor` or code-behind `.cs` files, and resolves dot-qualified
//! .NET type names (e.g. `MyApp.Pages.Counter`) to their file path.

use std::path::{Path, PathBuf};

use super::{FrameworkResolver, ResolveContext};

pub struct BlazorResolver;

impl FrameworkResolver for BlazorResolver {
    fn name(&self) -> &'static str {
        "blazor"
    }

    fn detect(&self, project_root: &Path) -> bool {
        // A .csproj with Blazor/AspNetCore content, or any .razor file present
        if let Ok(entries) = std::fs::read_dir(project_root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("csproj") {
                    if let Ok(content) = std::fs::read_to_string(&path)
                        && (content.contains("Blazor") || content.contains("AspNetCore"))
                    {
                        return true;
                    }
                    // Plain .csproj — check for .razor files nearby
                    return dir_has_ext(project_root, "razor", 3);
                }
            }
        }
        false
    }

    fn resolve_to_paths(&self, ctx: &ResolveContext<'_>) -> Vec<PathBuf> {
        let name = ctx.reference_name;

        // Dot-qualified .NET type: MyApp.Pages.Counter → Pages/Counter.razor
        if name.contains('.') {
            return resolve_dotnet_type(ctx.project_root, name);
        }

        // PascalCase bare name → search for matching .razor file
        if is_component_name(name) {
            return find_razor_component(ctx.project_root, name);
        }

        Vec::new()
    }
}

fn find_razor_component(project_root: &Path, name: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    find_file(project_root, &format!("{name}.razor"), &mut out, 8);
    // Also check for code-behind
    if out.is_empty() {
        find_file(project_root, &format!("{name}.cs"), &mut out, 8);
    }
    out
}

fn resolve_dotnet_type(project_root: &Path, qualified_name: &str) -> Vec<PathBuf> {
    let parts: Vec<&str> = qualified_name.split('.').collect();
    if parts.len() < 2 {
        return Vec::new();
    }
    let Some(&type_name) = parts.last() else {
        return Vec::new();
    };
    let ns_parts = parts.get(..parts.len() - 1).unwrap_or(&[]);
    let rel_dir: PathBuf = ns_parts.iter().collect();

    let mut out = Vec::new();
    for base in [
        project_root.to_path_buf(),
        project_root.join("Pages"),
        project_root.join("Components"),
        project_root.join("Shared"),
    ] {
        let razor = base.join(&rel_dir).join(format!("{type_name}.razor"));
        if razor.exists() {
            out.push(razor);
        }
        let cs = base.join(&rel_dir).join(format!("{type_name}.cs"));
        if cs.exists() {
            out.push(cs);
        }
    }
    out
}

fn find_file(dir: &Path, filename: &str, out: &mut Vec<PathBuf>, max_depth: usize) {
    if max_depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let stem = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches!(stem, "bin" | "obj" | "node_modules" | ".git" | "target") {
                continue;
            }
            find_file(&path, filename, out, max_depth - 1);
        } else if path.file_name().and_then(|n| n.to_str()) == Some(filename) {
            out.push(path);
        }
    }
}

fn dir_has_ext(dir: &Path, ext: &str, max_depth: usize) -> bool {
    if max_depth == 0 {
        return false;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if dir_has_ext(&path, ext, max_depth - 1) {
                return true;
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            return true;
        }
    }
    false
}

fn is_component_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('.')
        && !name.contains("::")
        && name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && name.chars().all(|c| c.is_alphanumeric() || c == '_')
}
