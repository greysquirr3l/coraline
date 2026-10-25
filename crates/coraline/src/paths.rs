//! Canonical project-relative path handling.
//!
//! A project path is stored in the database, matched against include and
//! exclude globs, and compared with import paths by the resolver. Those
//! three operations have to agree on one spelling, or the edges come out
//! wrong — a path stored as `src\com\example\Main.kt` matches no
//! `**/*.kt` glob, and no `com/example` import hint either.
//!
//! The canonical form is the relative path with `/` separators, on every
//! platform. Windows paths are the reason this module exists: `Path`
//! renders them with `\`, which silently breaks globs, package matching
//! and every SQL query that names a file.

use std::path::Path;

/// Render `path` as a project-relative path with `/` separators.
///
/// The path is split into its components and rejoined with `/`, so the
/// result is the same on every platform. A `/` inside a single file name
/// is not possible on any supported platform, and a `\` inside a file name
/// (legal on Unix, not on Windows) survives as part of that component.
pub fn canonical(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Replace Windows path separators with `/`.
///
/// Used when a path arrives as a string rather than as a `Path` — an
/// import path read out of source, or a path that came out of the
/// database before the canonical form was introduced.
pub fn normalise_separators(path: &str) -> String {
    if path.contains('\\') {
        path.replace('\\', "/")
    } else {
        path.to_string()
    }
}

/// The directory part of a canonical path, without a trailing separator.
///
/// `src/com/example/Main.kt` gives `src/com/example`; a path with no
/// directory gives an empty string.
pub fn parent(path: &str) -> &str {
    path.rfind('/').map_or("", |index| &path[..index])
}

/// The last segment of a canonical path.
pub fn file_name(path: &str) -> &str {
    path.rfind('/').map_or(path, |index| &path[index + 1..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_uses_forward_slashes() {
        assert_eq!(
            canonical(Path::new("src/com/example/Main.kt")),
            "src/com/example/Main.kt"
        );
    }

    #[test]
    fn canonical_of_a_bare_file_name_is_unchanged() {
        assert_eq!(canonical(Path::new("main.rs")), "main.rs");
    }

    #[test]
    fn normalise_separators_converts_windows_paths() {
        assert_eq!(
            normalise_separators("src\\com\\example\\Main.kt"),
            "src/com/example/Main.kt"
        );
    }

    #[test]
    fn normalise_separators_leaves_unix_paths_alone() {
        assert_eq!(
            normalise_separators("src/com/example/Main.kt"),
            "src/com/example/Main.kt"
        );
    }

    #[test]
    fn parent_and_file_name_split_a_canonical_path() {
        assert_eq!(parent("src/com/example/Main.kt"), "src/com/example");
        assert_eq!(file_name("src/com/example/Main.kt"), "Main.kt");
        assert_eq!(parent("main.rs"), "");
        assert_eq!(file_name("main.rs"), "main.rs");
    }
}
