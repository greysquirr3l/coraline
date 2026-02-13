use std::path::Path;

fn main() {
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
