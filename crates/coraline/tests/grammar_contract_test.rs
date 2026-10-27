#![deny(unsafe_code)]
// Test setup legitimately unwraps: a missing grammar or an unparseable
// fixture must fail the test loudly rather than silently skip a contract.
#![expect(clippy::expect_used, reason = "test setup unwraps are intentional")]

//! Guard test for the language ↔ grammar contract.
//!
//! Every node kind and field name that `extraction.rs` relies on is declared
//! in [`coraline::grammar`]. This test asserts that each declared string
//! actually exists in the tree-sitter grammar it claims to describe.
//!
//! Without this test, a grammar rename (or a typo like Kotlin's
//! `interface_declaration`, which does not exist) silently disables an entire
//! language: no call edges, no import edges, no inheritance edges — and the
//! only symptom is that `coraline callers` returns nothing.
//!
//! See issue #109.

use coraline::grammar::{self, CalleeSource};
use coraline::types::Language;
use tree_sitter::Language as TsLanguage;

/// Map a Coraline language to the tree-sitter grammar that backs it.
///
/// Only languages with a contract are checked; the contract itself is the
/// list of languages the extractor supports.
fn grammar_for(language: Language) -> Option<TsLanguage> {
    Some(match language {
        Language::Rust => TsLanguage::new(tree_sitter_rust::LANGUAGE),
        Language::JavaScript | Language::Jsx => TsLanguage::new(tree_sitter_javascript::LANGUAGE),
        Language::TypeScript => TsLanguage::new(tree_sitter_typescript::LANGUAGE_TYPESCRIPT),
        Language::Tsx => TsLanguage::new(tree_sitter_typescript::LANGUAGE_TSX),
        Language::Python => TsLanguage::new(tree_sitter_python::LANGUAGE),
        Language::Go => TsLanguage::new(tree_sitter_go::LANGUAGE),
        Language::Java => TsLanguage::new(tree_sitter_java::LANGUAGE),
        Language::C => TsLanguage::new(tree_sitter_c::LANGUAGE),
        Language::Cpp => TsLanguage::new(tree_sitter_cpp::LANGUAGE),
        Language::CSharp => TsLanguage::new(tree_sitter_c_sharp::LANGUAGE),
        Language::Php => TsLanguage::new(tree_sitter_php::LANGUAGE_PHP),
        Language::Ruby => TsLanguage::new(tree_sitter_ruby::LANGUAGE),
        Language::Swift => TsLanguage::new(tree_sitter_swift::LANGUAGE),
        Language::Kotlin => TsLanguage::new(tree_sitter_kotlin_ng::LANGUAGE),
        Language::Blazor => TsLanguage::new(tree_sitter_blazor::LANGUAGE),
        Language::Perl => TsLanguage::new(ts_parser_perl::LANGUAGE),
        Language::Fortran => TsLanguage::new(tree_sitter_fortran::LANGUAGE),
        Language::Haskell => TsLanguage::new(tree_sitter_haskell::LANGUAGE),
        Language::Julia => TsLanguage::new(tree_sitter_julia::LANGUAGE),
        Language::Lua => TsLanguage::new(tree_sitter_lua::LANGUAGE),
        Language::Matlab => TsLanguage::new(tree_sitter_matlab::LANGUAGE),
        Language::R => TsLanguage::new(tree_sitter_r::LANGUAGE),
        Language::Scala => TsLanguage::new(tree_sitter_scala::LANGUAGE),
        Language::Zig => TsLanguage::new(tree_sitter_zig::LANGUAGE),
        Language::Dart => TsLanguage::new(tree_sitter_dart::LANGUAGE),
        Language::Groovy => TsLanguage::new(tree_sitter_groovy::LANGUAGE),
        Language::Powershell => TsLanguage::new(tree_sitter_powershell::LANGUAGE),
        Language::Nix => TsLanguage::new(tree_sitter_nix::LANGUAGE),
        Language::Erlang => TsLanguage::new(tree_sitter_erlang::LANGUAGE),
        Language::Elm => TsLanguage::new(tree_sitter_elm::LANGUAGE),
        Language::Bash => TsLanguage::new(tree_sitter_bash::LANGUAGE),
        Language::Elixir => TsLanguage::new(tree_sitter_elixir::LANGUAGE),
        Language::Toml => TsLanguage::new(tree_sitter_toml_ng::LANGUAGE),
        Language::Yaml => TsLanguage::new(tree_sitter_yaml::LANGUAGE),
        _ => return None,
    })
}

/// Return `true` when `kind` is a node kind the grammar knows about.
fn has_node_kind(ts: &TsLanguage, kind: &str) -> bool {
    ts.id_for_node_kind(kind, true) != 0
}

/// Return `true` when `field` is a field name the grammar knows about.
fn has_field(ts: &TsLanguage, field: &str) -> bool {
    ts.field_id_for_name(field).is_some()
}

#[test]
fn every_declared_node_kind_exists_in_its_grammar() {
    for contract in grammar::CONTRACTS {
        let ts = grammar_for(contract.language)
            .expect("every contracted language has a grammar wired up in this test's match arm");

        for (kind, node_kind, _) in contract.kinds {
            assert!(
                has_node_kind(&ts, kind),
                "{:?}: node kind {:?} (mapped to {:?}) does not exist in the grammar",
                contract.language,
                kind,
                node_kind
            );
        }

        for kind in contract.import_kinds {
            assert!(
                has_node_kind(&ts, kind),
                "{:?}: import node kind {:?} does not exist in the grammar",
                contract.language,
                kind
            );
        }

        for kind in contract.instantiation_kinds {
            assert!(
                has_node_kind(&ts, kind),
                "{:?}: instantiation node kind {:?} does not exist in the grammar",
                contract.language,
                kind
            );
        }
    }
}

#[test]
fn every_declared_call_kind_exists_in_its_grammar() {
    for contract in grammar::CONTRACTS {
        let ts = grammar_for(contract.language)
            .expect("every contracted language has a grammar wired up in this test's match arm");

        for (kind, _) in contract.call_kinds {
            assert!(
                has_node_kind(&ts, kind),
                "{:?}: call node kind {:?} does not exist in the grammar",
                contract.language,
                kind
            );
        }
    }
}

/// Source snippets that contain one call site of a given node kind.
///
/// The callee field has to exist *on the call node*, not merely somewhere
/// in the grammar. A field can exist in a grammar without existing on a
/// particular node kind — C#'s `using_directive` is the classic example,
/// where the grammar has a `name` field (used by `method_declaration`)
/// but the import node stores its path in a `qualified_name` child. A
/// grammar-wide check cannot tell the two apart.
///
/// The snippets are qualified where the contract declares a receiver, so
/// that the receiver field is present as well.
const CALL_SITES: &[(Language, &str, &str)] = &[
    (Language::Rust, "call_expression", "fn main() { helper(); }"),
    (
        Language::Rust,
        "macro_invocation",
        "fn main() { println!(\"x\"); }",
    ),
    (Language::TypeScript, "call_expression", "helper();"),
    (Language::TypeScript, "new_expression", "new Helper();"),
    (Language::JavaScript, "call_expression", "helper();"),
    (Language::JavaScript, "new_expression", "new Helper();"),
    (Language::Python, "call", "helper()"),
    (Language::Go, "call_expression", "helper()"),
    (Language::Java, "method_invocation", "receiver.helper();"),
    (
        Language::Java,
        "object_creation_expression",
        "new Helper();",
    ),
    (Language::C, "call_expression", "helper();"),
    (Language::Cpp, "call_expression", "helper();"),
    (Language::CSharp, "invocation_expression", "helper();"),
    (
        Language::CSharp,
        "object_creation_expression",
        "new Helper();",
    ),
    (Language::Php, "function_call_expression", "<?php helper();"),
    (
        Language::Php,
        "member_call_expression",
        "<?php $receiver->helper();",
    ),
    (
        Language::Php,
        "scoped_call_expression",
        "<?php Helper::helper();",
    ),
    (
        Language::Php,
        "nullsafe_member_call_expression",
        "<?php $receiver?->helper();",
    ),
    (Language::Ruby, "call", "receiver.helper()"),
    (Language::Perl, "function_call_expression", "helper();"),
    (
        Language::Perl,
        "method_call_expression",
        "$receiver->helper();",
    ),
    (Language::Fortran, "subroutine_call", "call report(1)"),
    (Language::Haskell, "apply", "area (head shapes)"),
    (Language::Lua, "function_call", "helper(1)"),
    (Language::Matlab, "function_call", "helper(1);"),
    (Language::R, "call", "area(radius)"),
    (Language::Scala, "call_expression", "helper()"),
    (Language::Zig, "call_expression", "helper()"),
    (
        Language::Dart,
        "call_expression",
        "double total() { return helper(); }",
    ),
    (Language::Groovy, "method_invocation", "receiver.helper()"),
    (Language::Erlang, "call", "helper(1)"),
    (Language::Erlang, "remote", "lists:map(fun area/1, Shapes)"),
    (
        Language::Elm,
        "function_call_expr",
        "module M exposing (helper)\n\nhelper radius =\n    area radius\n",
    ),
    (Language::Bash, "command", "helper"),
    (Language::Nix, "apply_expression", "helper radius"),
    (Language::Powershell, "command", "Get-Area -Radius $r"),
    (Language::Elixir, "call", "Enum.map(shapes, &Circle.area/1)"),
];

#[test]
fn call_nodes_expose_the_declared_callee_field() {
    for contract in grammar::CONTRACTS {
        for (kind, source) in contract.call_kinds {
            let CalleeSource::Field { callee, receiver } = source else {
                continue;
            };

            let snippet = CALL_SITES
                .iter()
                .find(|(language, node_kind, _)| {
                    *language == contract.language && *node_kind == *kind
                })
                .map_or_else(
                    || {
                        unreachable!(
                            "{:?}: no source snippet declared for call node kind {:?}",
                            contract.language, kind
                        )
                    },
                    |(_, _, snippet)| *snippet,
                );

            with_parsed_tree(contract.language, snippet, |tree| {
                let node = find_node(&tree.root_node(), kind).unwrap_or_else(|| {
                    unreachable!(
                        "{:?}: {snippet:?} does not contain a {kind:?} node",
                        contract.language
                    )
                });

                assert!(
                    node.child_by_field_name(callee).is_some(),
                    "{:?}: the {kind:?} node does not expose the callee field {callee:?}",
                    contract.language
                );

                if let Some(receiver) = receiver {
                    assert!(
                        node.child_by_field_name(receiver).is_some(),
                        "{:?}: the {kind:?} node does not expose the receiver field {receiver:?}",
                        contract.language
                    );
                }
            });
        }
    }
}

/// Every language Coraline claims to support needs a contract.
///
/// A language without a contract is not "not supported yet" — it is a
/// language that is indexed and produces an empty file: no nodes, no
/// edges, and no symptom beyond `coraline callers` returning nothing.
/// TSX, JSX and Blazor all fell into that hole when the extractor moved
/// to the contract table.
#[test]
fn every_supported_language_has_a_contract() {
    /// Languages Coraline indexes without extracting declarations.
    ///
    /// Markdown and Liquid are markup: they have no declarations to
    /// extract, so they have nothing to declare.
    const WITHOUT_DECLARATIONS: &[Language] = &[Language::Markdown, Language::Liquid];

    for language in ALL_LANGUAGES {
        if !coraline::config::is_language_supported(language) {
            continue;
        }
        if WITHOUT_DECLARATIONS.contains(language) {
            continue;
        }
        assert!(
            grammar::contract_for(*language).is_some(),
            "{language:?} is a supported language but has no grammar contract, so it is indexed as an empty file"
        );
    }
}

/// Every language Coraline knows about.
const ALL_LANGUAGES: &[Language] = &[
    Language::TypeScript,
    Language::JavaScript,
    Language::Tsx,
    Language::Jsx,
    Language::Python,
    Language::Go,
    Language::Rust,
    Language::Java,
    Language::C,
    Language::Cpp,
    Language::CSharp,
    Language::Php,
    Language::Ruby,
    Language::Swift,
    Language::Kotlin,
    Language::Liquid,
    Language::Markdown,
    Language::Blazor,
    Language::Perl,
    Language::Bash,
    Language::Dart,
    Language::Elixir,
    Language::Elm,
    Language::Erlang,
    Language::Fortran,
    Language::Groovy,
    Language::Haskell,
    Language::Julia,
    Language::Lua,
    Language::Matlab,
    Language::Nix,
    Language::Powershell,
    Language::R,
    Language::Scala,
    Language::Toml,
    Language::Yaml,
    Language::Zig,
];

/// Parse `source` with `language`'s grammar and hand the tree to `f`.
///
/// The tree has to outlive any node borrowed from it, so it is passed to a
/// closure rather than returned alongside the node.
fn with_parsed_tree<R>(
    language: Language,
    source: &str,
    f: impl FnOnce(&tree_sitter::Tree) -> R,
) -> R {
    let ts = grammar_for(language).expect("grammar wired up for the tested language");
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&ts).expect("set language");

    let tree = parser.parse(source, None).expect("parse source");
    f(&tree)
}

/// Find the first node with kind `node_kind`, searching the whole tree.
fn find_node<'a>(root: &tree_sitter::Node<'a>, node_kind: &str) -> Option<tree_sitter::Node<'a>> {
    let mut cursor = root.walk();
    if let Some(node) = root
        .named_children(&mut cursor)
        .find(|n| n.kind() == node_kind)
    {
        return Some(node);
    }

    let mut stack = vec![*root];
    while let Some(node) = stack.pop() {
        let mut inner = node.walk();
        for child in node.named_children(&mut inner) {
            if child.kind() == node_kind {
                return Some(child);
            }
            stack.push(child);
        }
    }
    None
}

/// Assert that the import node of `language` exposes its path via `field`.
///
/// A field can exist in a grammar without existing on a *particular* node
/// kind. C#'s `using_directive` is the classic example: the grammar has a
/// `name` field (used by `method_declaration`), but the import node stores
/// its path in a `qualified_name` *child*. Checking the field against the
/// grammar as a whole would miss that, so the contract test parses real
/// source and inspects the actual node.
#[test]
fn import_nodes_expose_the_declared_path_field() {
    let cases: &[(Language, &str, &str, &str)] = &[
        (
            Language::Rust,
            "use std::collections::HashMap;",
            "use_declaration",
            "argument",
        ),
        (
            Language::JavaScript,
            "import fs from 'node:fs';",
            "import_statement",
            "source",
        ),
        (
            Language::TypeScript,
            "import fs from 'node:fs';",
            "import_statement",
            "source",
        ),
        (
            Language::Python,
            "from os import path",
            "import_from_statement",
            "module_name",
        ),
        (Language::C, "#include <stdio.h>", "preproc_include", "path"),
        (
            Language::Cpp,
            "#include <stdio.h>",
            "preproc_include",
            "path",
        ),
    ];

    for (language, source, node_kind, field) in cases {
        with_parsed_tree(*language, source, |tree| {
            let node = find_node(&tree.root_node(), node_kind)
                .expect("the fixture source contains the expected node kind");
            assert!(
                node.child_by_field_name(field).is_some(),
                "{language:?}: {node_kind} node does not expose its path via the {field:?} field"
            );
        });
    }
}

/// C# stores a `using` path in a `qualified_name` child, not in a field.
///
/// The grammar *does* have a `name` field (used by declarations), so a
/// grammar-wide check would wrongly pass. This pins the node-level shape so
/// a future refactor does not silently break C# imports.
#[test]
fn csharp_using_directive_has_no_name_field() {
    with_parsed_tree(Language::CSharp, "using System.Text;\n", |tree| {
        let node = find_node(&tree.root_node(), "using_directive")
            .expect("using_directive node in the parsed source");

        assert!(
            node.child_by_field_name("name").is_none(),
            "C# using_directive unexpectedly gained a `name` field; update the extractor's \
             import_module_path arm for CSharp"
        );

        let mut cursor = node.walk();
        assert!(
            node.named_children(&mut cursor)
                .any(|n| n.kind() == "qualified_name" || n.kind() == "identifier"),
            "C# using_directive no longer has a qualified_name or identifier child"
        );
    });
}

/// Java stores an import's path in a `scoped_identifier` child, not in a
/// field. This test pins that behaviour so a future refactor does not
/// silently break Java imports.
#[test]
fn java_import_path_lives_in_a_scoped_identifier_child() {
    let ts = grammar_for(Language::Java).expect("java grammar");
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&ts).expect("set language");

    let source = "package com.a;
import com.b.Other;
";
    let tree = parser.parse(source, None).expect("parse");
    let root = tree.root_node();

    let mut cursor = root.walk();
    let import = root
        .named_children(&mut cursor)
        .find(|n| n.kind() == "import_declaration")
        .expect("import_declaration node");

    // The old code looked for a `name` field on the import node; Java has
    // none, so the path must come from the child.
    assert!(
        import.child_by_field_name("name").is_none(),
        "java import_declaration unexpectedly gained a `name` field; update the extractor"
    );

    let mut cursor = import.walk();
    let has_scoped = import
        .named_children(&mut cursor)
        .any(|n| n.kind() == "scoped_identifier");
    assert!(
        has_scoped,
        "java import_declaration no longer has a scoped_identifier child"
    );
}

#[test]
fn every_declared_import_field_exists_in_its_grammar() {
    // The fields used to reach an import's module path, per language.
    // These are the names that used to be wrong (e.g. `CSharp` used
    // `qualified_name`, `Kotlin` used `type`, `Swift` used `module_name`).
    let import_fields: &[(Language, &str)] = &[
        (Language::Rust, "argument"),
        (Language::JavaScript, "source"),
        (Language::TypeScript, "source"),
        (Language::Tsx, "source"),
        (Language::Jsx, "source"),
        (Language::Python, "module_name"),
        (Language::Java, "name"),
        (Language::C, "path"),
        (Language::Cpp, "path"),
        (Language::Php, "name"),
    ];

    for (language, field) in import_fields {
        let ts = grammar_for(*language).expect("grammar wired up for the tested language");
        assert!(
            has_field(&ts, field),
            "{language:?}: import field {field:?} does not exist in the grammar"
        );
    }
}

#[test]
fn every_declared_name_field_exists_in_its_grammar() {
    // Fields used to recover a declaration's name.
    let name_fields: &[(Language, &str)] = &[
        (Language::Rust, "name"),
        (Language::JavaScript, "name"),
        (Language::TypeScript, "name"),
        (Language::Python, "name"),
        (Language::Go, "name"),
        (Language::Java, "name"),
        (Language::C, "declarator"),
        (Language::Cpp, "declarator"),
        (Language::CSharp, "name"),
        (Language::Php, "name"),
        (Language::Ruby, "name"),
        (Language::Swift, "name"),
        (Language::Kotlin, "name"),
    ];

    for (language, field) in name_fields {
        let ts = grammar_for(*language).expect("grammar wired up for the tested language");
        assert!(
            has_field(&ts, field),
            "{language:?}: name field {field:?} does not exist in the grammar"
        );
    }
}

#[test]
fn every_language_with_a_contract_is_supported() {
    // A contract that exists but whose language is not in
    // `is_language_supported` would be dead code — exactly the bug the issue
    // described for 17 languages.
    for contract in grammar::CONTRACTS {
        assert!(
            coraline::config::is_language_supported(&contract.language),
            "{:?} has a grammar contract but is not a supported language",
            contract.language
        );
    }
}

#[test]
fn kotlin_has_no_interface_declaration_node_kind() {
    // Regression guard for the specific mistake the issue called out:
    // Kotlin's `interface` is an anonymous keyword token, not a node kind.
    // The contract must map `class_declaration` and discriminate the
    // interface case from the node's children instead.
    let ts = grammar_for(Language::Kotlin).expect("kotlin grammar");
    assert!(
        !has_node_kind(&ts, "interface_declaration"),
        "tree-sitter-kotlin-ng unexpectedly grew an `interface_declaration` node kind; \
         the Kotlin contract in coraline::grammar should be updated"
    );
    assert!(has_node_kind(&ts, "class_declaration"));
}

#[test]
fn swift_uses_class_declaration_for_structs_and_enums() {
    // Swift's `struct`/`enum`/`class`/`extension` all arrive as
    // `class_declaration`, discriminated by the `declaration_kind` field.
    let ts = grammar_for(Language::Swift).expect("swift grammar");
    assert!(has_node_kind(&ts, "class_declaration"));
    assert!(has_field(&ts, "declaration_kind"));
    assert!(
        !has_node_kind(&ts, "struct_declaration"),
        "tree-sitter-swift unexpectedly grew a `struct_declaration` node kind; \
         the Swift contract in coraline::grammar should be updated"
    );
}

#[test]
fn ruby_uses_call_not_method_call() {
    // Ruby call sites are `call` nodes with a `method` field. The old code
    // looked for `method_call`, which does not exist.
    let ts = grammar_for(Language::Ruby).expect("ruby grammar");
    assert!(has_node_kind(&ts, "call"));
    assert!(has_field(&ts, "method"));
    assert!(!has_node_kind(&ts, "method_call"));
}

#[test]
fn java_uses_name_field_for_method_invocation() {
    // Java's `method_invocation` exposes the method name via `name`; the old
    // code looked for `method`.
    let ts = grammar_for(Language::Java).expect("java grammar");
    assert!(has_node_kind(&ts, "method_invocation"));
    assert!(has_field(&ts, "name"));
    assert!(has_field(&ts, "object"));
}

#[test]
fn go_imports_live_in_import_spec() {
    // Go's `import_declaration` has no fields at all; the path is on the
    // nested `import_spec`.
    let ts = grammar_for(Language::Go).expect("go grammar");
    assert!(has_node_kind(&ts, "import_declaration"));
    assert!(has_node_kind(&ts, "import_spec"));
    assert!(has_field(&ts, "path"));
}
