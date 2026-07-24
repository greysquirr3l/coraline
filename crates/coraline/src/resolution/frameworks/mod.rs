#![forbid(unsafe_code)]

//! Framework-specific reference resolution plugins.
//!
//! Each resolver handles one framework's naming conventions and returns
//! candidate file paths that likely contain the referenced symbol.  The
//! caller is responsible for looking up nodes in those paths via the DB.

use std::path::{Path, PathBuf};

pub mod blazor;
pub mod laravel;
pub mod react;
pub mod rust;

/// Context passed to a framework resolver when attempting to resolve a reference.
pub struct ResolveContext<'a> {
    /// Absolute path of the file that contains the reference.
    pub from_file: &'a str,
    /// The name/text of the reference being resolved.
    pub reference_name: &'a str,
    /// Root directory of the project (absolute).
    pub project_root: &'a Path,
}

/// Trait that framework-specific resolvers implement.
///
/// Implementations are stateless; all state needed for detection and
/// resolution is derived at call time from the project root and context.
pub trait FrameworkResolver: Send + Sync {
    /// Short identifier for this resolver (used in log messages).
    fn name(&self) -> &'static str;

    /// Return `true` if this resolver applies to the project at `project_root`.
    fn detect(&self, project_root: &Path) -> bool;

    /// Given a reference context, return absolute paths of files that likely
    /// define the referenced symbol.  Returns an empty `Vec` when this
    /// resolver cannot help with the given reference.
    fn resolve_to_paths(&self, ctx: &ResolveContext<'_>) -> Vec<PathBuf>;
}

/// Build the list of all framework resolvers in evaluation order.
pub fn default_resolvers() -> Vec<Box<dyn FrameworkResolver>> {
    vec![
        Box::new(rust::RustResolver),
        Box::new(react::ReactResolver),
        Box::new(blazor::BlazorResolver),
        Box::new(laravel::LaravelResolver),
    ]
}

/// Return absolute path hints from the first applicable resolver that
/// produces results.  Returns an empty `Vec` if no resolver helps.
pub fn framework_path_hints(
    project_root: &Path,
    from_file: &str,
    reference_name: &str,
) -> Vec<PathBuf> {
    let ctx = ResolveContext {
        from_file,
        reference_name,
        project_root,
    };
    for resolver in default_resolvers() {
        if resolver.detect(project_root) {
            let paths = resolver.resolve_to_paths(&ctx);
            if !paths.is_empty() {
                tracing::debug!(
                    resolver = resolver.name(),
                    reference = reference_name,
                    candidates = paths.len(),
                    "framework resolver produced path hints"
                );
                return paths;
            }
        }
    }
    Vec::new()
}
