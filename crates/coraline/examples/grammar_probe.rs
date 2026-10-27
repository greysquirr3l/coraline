//! Dump a grammar's node kinds and fields, and print the parse tree of a
//! sample file.
//!
//! The extraction contracts in `coraline::grammar` name node kinds and
//! fields that have to exist in the grammar they describe. The names have
//! to be discovered empirically — a grammar rename or a typo silently
//! disables a language — so this tool prints what the grammar actually
//! has:
//!
//! ```text
//! cargo run --example grammar_probe -- fortran           # node kinds and fields
//! cargo run --example grammar_probe -- fortran file.f90   # the parse tree
//! ```
#![allow(clippy::print_stdout)]

use std::process::ExitCode;

use tree_sitter::{Language as TsLanguage, Parser};

/// The grammar backing each language the extractor knows about.
fn grammar_for(name: &str) -> Option<TsLanguage> {
    let language = match name {
        "bash" => tree_sitter::Language::new(tree_sitter_bash::LANGUAGE),
        "c" => tree_sitter::Language::new(tree_sitter_c::LANGUAGE),
        "cpp" => tree_sitter::Language::new(tree_sitter_cpp::LANGUAGE),
        "csharp" => tree_sitter::Language::new(tree_sitter_c_sharp::LANGUAGE),
        "dart" => tree_sitter::Language::new(tree_sitter_dart::LANGUAGE),
        "elixir" => tree_sitter::Language::new(tree_sitter_elixir::LANGUAGE),
        "elm" => tree_sitter::Language::new(tree_sitter_elm::LANGUAGE),
        "erlang" => tree_sitter::Language::new(tree_sitter_erlang::LANGUAGE),
        "fortran" => tree_sitter::Language::new(tree_sitter_fortran::LANGUAGE),
        "go" => tree_sitter::Language::new(tree_sitter_go::LANGUAGE),
        "groovy" => tree_sitter::Language::new(tree_sitter_groovy::LANGUAGE),
        "haskell" => tree_sitter::Language::new(tree_sitter_haskell::LANGUAGE),
        "java" => tree_sitter::Language::new(tree_sitter_java::LANGUAGE),
        "javascript" => tree_sitter::Language::new(tree_sitter_javascript::LANGUAGE),
        "julia" => tree_sitter::Language::new(tree_sitter_julia::LANGUAGE),
        "kotlin" => tree_sitter::Language::new(tree_sitter_kotlin_ng::LANGUAGE),
        "lua" => tree_sitter::Language::new(tree_sitter_lua::LANGUAGE),
        "matlab" => tree_sitter::Language::new(tree_sitter_matlab::LANGUAGE),
        "nix" => tree_sitter::Language::new(tree_sitter_nix::LANGUAGE),
        "perl" => tree_sitter::Language::new(ts_parser_perl::LANGUAGE),
        "php" => tree_sitter::Language::new(tree_sitter_php::LANGUAGE_PHP),
        "powershell" => tree_sitter::Language::new(tree_sitter_powershell::LANGUAGE),
        "python" => tree_sitter::Language::new(tree_sitter_python::LANGUAGE),
        "r" => tree_sitter::Language::new(tree_sitter_r::LANGUAGE),
        "ruby" => tree_sitter::Language::new(tree_sitter_ruby::LANGUAGE),
        "rust" => tree_sitter::Language::new(tree_sitter_rust::LANGUAGE),
        "scala" => tree_sitter::Language::new(tree_sitter_scala::LANGUAGE),
        "swift" => tree_sitter::Language::new(tree_sitter_swift::LANGUAGE),
        "toml" => tree_sitter::Language::new(tree_sitter_toml_ng::LANGUAGE),
        "typescript" => tree_sitter::Language::new(tree_sitter_typescript::LANGUAGE_TYPESCRIPT),
        "yaml" => tree_sitter::Language::new(tree_sitter_yaml::LANGUAGE),
        "zig" => tree_sitter::Language::new(tree_sitter_zig::LANGUAGE),
        _ => return None,
    };
    Some(language)
}

/// Print the node kind, the field it sits in and a snippet of its text.
fn print_tree(node: &tree_sitter::Node, source: &str, depth: usize, field: Option<&str>) {
    let indent = "  ".repeat(depth);
    let field = field.map_or(String::new(), |f| format!("{f}: "));
    let text = node.utf8_text(source.as_bytes()).unwrap_or_default();
    // A newline would break the indentation of the tree, so it is shown as
    // a marker instead.
    let snippet: String = text
        .chars()
        .map(|c| if c == '\n' { '⏎' } else { c })
        .take(48)
        .collect();
    println!("{indent}{field}{}  |{snippet}|", node.kind());

    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    for (index, child) in children.iter().enumerate() {
        let field = u32::try_from(index).ok().and_then(|index| node.field_name_for_child(index));
        print_tree(child, source, depth + 1, field);
    }
}

/// Print every named node kind the grammar knows.
fn print_node_kinds(language: &TsLanguage) {
    println!("=== node kinds ===");
    for id in 0..language.node_kind_count() {
        let Ok(id) = u16::try_from(id) else {
            continue;
        };
        if let Some(kind) = language.node_kind_for_id(id)
            && language.node_kind_is_named(id)
        {
            println!("  {kind}");
        }
    }
}

/// Print every field name the grammar knows.
fn print_fields(language: &TsLanguage) {
    println!("=== fields ===");
    for id in 0..language.field_count() {
        let Ok(id) = u16::try_from(id) else {
            continue;
        };
        if let Some(name) = language.field_name_for_id(id) {
            println!("  {name}");
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let Some(name) = args.get(1) else {
        eprintln!("usage: grammar_probe <language> [file]");
        return ExitCode::from(2);
    };

    let Some(language) = grammar_for(name) else {
        eprintln!("unknown grammar: {name}");
        return ExitCode::from(2);
    };

    let Some(file) = args.get(2) else {
        print_node_kinds(&language);
        print_fields(&language);
        return ExitCode::SUCCESS;
    };

    let Ok(source) = std::fs::read_to_string(file) else {
        eprintln!("could not read {file}");
        return ExitCode::FAILURE;
    };

    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        eprintln!("could not load the {name} grammar");
        return ExitCode::FAILURE;
    }
    let Some(tree) = parser.parse(&source, None) else {
        eprintln!("could not parse {file}");
        return ExitCode::FAILURE;
    };

    print_tree(&tree.root_node(), &source, 0, None);
    ExitCode::SUCCESS
}
