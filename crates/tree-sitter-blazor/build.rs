use std::path::Path;

fn main() {
    // Previously this build script wrote a wrapper around `/usr/bin/ar`
    // that stripped GNU-style `-D` (deterministic mode) flags so
    // `cc-rs` could invoke macOS's BSD `ar`. The regex-based filter
    // (`${arg//D/}` applied to any `[A-Za-z-]+` arg) was too broad and
    // also stripped legitimate BSD-ar flags like `-p`, breaking
    // `cargo lint`. Modern `cc-rs` (>=1.x) handles BSD ar correctly via
    // the `AR` env var, so we just use the system archiver as-is.
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
