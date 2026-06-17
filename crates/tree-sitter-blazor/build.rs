use std::path::Path;

#[cfg(target_os = "macos")]
fn configure_macos_archiver() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    if std::env::var_os("AR").is_some() {
        return;
    }

    let out_dir =
        std::env::var_os("OUT_DIR").map_or_else(|| PathBuf::from("target"), PathBuf::from);
    let wrapper = out_dir.join("bsd-ar-wrapper.sh");
    let script = r#"#!/usr/bin/env bash
set -euo pipefail
args=()
for arg in "$@"; do
  if [[ "$arg" =~ ^[A-Za-z-]+$ ]]; then
    args+=("${arg//D/}")
  else
    args+=("$arg")
  fi
done
exec /usr/bin/ar "${args[@]}"
"#;

    let _ = fs::write(&wrapper, script);
    let _ = fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755));
    std::env::set_var("AR", wrapper);
}

fn main() {
    #[cfg(target_os = "macos")]
    configure_macos_archiver();

    let mut build = cc::Build::new();
    build.file("src/parser.c").include("src");

    let scanner_path = Path::new("src/scanner.c");
    if scanner_path.exists() {
        build.file(scanner_path);
        println!("cargo:rerun-if-changed=src/scanner.c");
    }

    build.compile("tree-sitter-blazor");

    println!("cargo:rerun-if-changed=grammar.js");
    println!("cargo:rerun-if-changed=src/parser.c");
    println!("cargo:rerun-if-changed=src/node-types.json");
}
