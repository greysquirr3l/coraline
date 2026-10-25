#![deny(unsafe_code)]
#![allow(clippy::match_same_arms)]

//! Declarative description of how each language's tree-sitter grammar maps
//! onto Coraline's node kinds.
//!
//! The extraction code used to hard-code tree-sitter node kinds and field
//! names in a series of `match language { … }` blocks. Several of those
//! strings never existed in the corresponding grammar (Kotlin's
//! `interface_declaration`, Swift's `function_call_expression`, Ruby's
//! `method_call`, …), so whole languages silently produced zero call,
//! import and inheritance edges.
//!
//! This module makes the mapping data-driven. Every node kind and field name
//! used by the extractor is declared here, and
//! `tests/grammar_contract_test.rs` asserts that each declared string really
//! exists in the grammar it claims to describe. That turns "the grammar
//! changed and we silently stopped indexing" into a failing test.
//!
//! The contract is deliberately *declarative*: it describes *which* grammar
//! constructs matter, not how to walk them. Language-specific traversal
//! details stay in `extraction.rs`, but they may only reference strings that
//! appear in this module.

use crate::types::{Language, NodeKind};

/// How the name of a declaration is recovered from its grammar node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameSource {
    /// The node has a `name` field holding the identifier.
    NameField,
    /// The identifier is a direct child node of this kind.
    ChildKind(&'static str),
    /// The identifier is reached through a chain of named fields.
    FieldPath(&'static [&'static str]),
    /// The declaration is anonymous (no name to recover).
    None,
}

/// How the callee of a call expression is recovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalleeSource {
    /// A field on the call node holds the callee expression.
    ///
    /// `receiver` names the field on the *call node* that holds the
    /// receiver, for grammars that split the two (Java's `method_invocation`
    /// has `name` and `object`; PHP's `member_call_expression` has `name`
    /// and `object`).
    Field {
        callee: &'static str,
        receiver: Option<&'static str>,
    },
    /// The callee is the first named child of the call node, and the callee
    /// *name* is the last identifier inside it. Used by languages whose
    /// grammars expose `call_expression` without a `function` field
    /// (Kotlin, Swift).
    FirstChildLastIdentifier,
}

/// How a supertype is recovered from a declaration's heritage clause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeritageSource {
    /// A field on the declaration node holds the supertype expression
    /// (Rust's `impl_item.type`, Python's `class_definition.superclasses`).
    Field(&'static str),
    /// The supertype names live in named children of this kind; each child
    /// carries the type in the given field
    /// (TypeScript's `extends_clause.value`).
    ChildField(&'static str, &'static str),
    /// The supertype names live in named children of this kind; each child
    /// *is* the type name
    /// (C#'s `base_list`, Kotlin's `delegation_specifier`).
    ChildIsType(&'static str),
    /// The supertype names live in named children of this kind; each child
    /// holds the type in a nested node of the given kind
    /// (Swift's `inheritance_specifier.inherits_from` → `user_type`).
    ChildNested(&'static str, &'static str),
    /// The supertype names live in named children of a *container* node of
    /// this kind; each of those holds the type in a nested node of the given
    /// kind (Kotlin's `delegation_specifiers` → `delegation_specifier` →
    /// `constructor_invocation` → `user_type`).
    ChildInContainer(&'static str, &'static str, &'static str),
}

/// A single language's mapping from grammar constructs to graph concepts.
#[derive(Debug, Clone, Copy)]
pub struct LanguageContract {
    /// The language this contract describes.
    pub language: Language,
    /// Grammar node kinds that become graph nodes, and the graph kind each
    /// maps to.
    ///
    /// The second element of the tuple is `true` when the node introduces a
    /// scope (children belong to it) and `false` for leaf-ish declarations.
    pub kinds: &'static [(&'static str, NodeKind, bool)],
    /// Grammar node kinds that represent a call site.
    pub call_kinds: &'static [(&'static str, CalleeSource)],
    /// Grammar node kinds that represent an import.
    pub import_kinds: &'static [&'static str],
    /// Grammar node kinds that represent a type instantiation
    /// (`new Foo()`, `Foo()`, …).
    pub instantiation_kinds: &'static [&'static str],
    /// How supertypes are recovered from a declaration node.
    ///
    /// `None` means the language has no heritage clause (or it is not
    /// modelled yet).
    pub heritage: Option<&'static [HeritageRule]>,
}

/// One rule for recovering supertypes from a declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeritageRule {
    /// Where the supertype names come from.
    pub source: HeritageSource,
    /// Whether the recovered supertypes are base classes or interfaces.
    pub kind: HeritageKind,
}

/// Whether a recovered supertype expresses *extension* (inheritance from a
/// class) or *implementation* (conformance to an interface/trait/protocol).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeritageKind {
    /// The supertype is a base class.
    Extends,
    /// The supertype is an interface/trait/protocol.
    Implements,
}

/// Look up the contract for `language`, if one is declared.
pub fn contract_for(language: Language) -> Option<&'static LanguageContract> {
    CONTRACTS.iter().find(|c| c.language == language)
}

/// Map a grammar node kind to a graph node kind for `language`.
///
/// Returns `(node_kind, is_container)`. `node_kind` is `None` when the
/// grammar kind does not correspond to a graph node.
pub fn map_kind(language: Language, kind: &str) -> (Option<NodeKind>, bool) {
    let Some(contract) = contract_for(language) else {
        return (None, false);
    };
    contract
        .kinds
        .iter()
        .find(|(grammar_kind, _, _)| *grammar_kind == kind)
        .map_or((None, false), |(_, node_kind, container)| {
            (Some(*node_kind), *container)
        })
}

/// Return the callee-extraction strategy for a call node of `language`.
pub fn callee_source(language: Language, kind: &str) -> Option<CalleeSource> {
    let contract = contract_for(language)?;
    contract
        .call_kinds
        .iter()
        .find(|(grammar_kind, _)| *grammar_kind == kind)
        .map(|(_, source)| *source)
}

/// Return `true` when `kind` is a call site in `language`.
pub fn is_call_kind(language: Language, kind: &str) -> bool {
    callee_source(language, kind).is_some()
}

/// Return `true` when `kind` is an import statement in `language`.
pub fn is_import_kind(language: Language, kind: &str) -> bool {
    contract_for(language).is_some_and(|c| c.import_kinds.contains(&kind))
}

/// Return `true` when `kind` is a type instantiation in `language`.
pub fn is_instantiation_kind(language: Language, kind: &str) -> bool {
    contract_for(language).is_some_and(|c| c.instantiation_kinds.contains(&kind))
}

/// All contracts, in declaration order.
pub static CONTRACTS: &[LanguageContract] = &[
    LanguageContract {
        language: Language::Rust,
        kinds: &[
            ("function_item", NodeKind::Function, false),
            ("struct_item", NodeKind::Struct, true),
            ("enum_item", NodeKind::Enum, true),
            ("trait_item", NodeKind::Trait, true),
            ("impl_item", NodeKind::Class, true),
            ("use_declaration", NodeKind::Import, false),
            ("mod_item", NodeKind::Module, true),
            ("extern_crate_declaration", NodeKind::Export, false),
            ("const_item", NodeKind::Constant, false),
            ("static_item", NodeKind::Constant, false),
            ("type_item", NodeKind::TypeAlias, false),
            ("field_declaration", NodeKind::Field, false),
            ("macro_definition", NodeKind::Function, false),
        ],
        call_kinds: &[
            (
                "call_expression",
                CalleeSource::Field {
                    callee: "function",
                    receiver: None,
                },
            ),
            (
                "macro_invocation",
                CalleeSource::Field {
                    callee: "macro",
                    receiver: None,
                },
            ),
        ],
        import_kinds: &["use_declaration"],
        instantiation_kinds: &[],
        heritage: Some(&[HeritageRule {
            source: HeritageSource::Field("type"),
            kind: HeritageKind::Implements,
        }]),
    },
    LanguageContract {
        language: Language::TypeScript,
        kinds: &[
            ("function_declaration", NodeKind::Function, false),
            ("arrow_function", NodeKind::Function, false),
            ("function_expression", NodeKind::Function, false),
            ("generator_function_declaration", NodeKind::Function, false),
            ("class_declaration", NodeKind::Class, true),
            ("abstract_class_declaration", NodeKind::Class, true),
            ("method_definition", NodeKind::Method, false),
            ("interface_declaration", NodeKind::Interface, true),
            ("type_alias_declaration", NodeKind::TypeAlias, false),
            ("enum_declaration", NodeKind::Enum, true),
            ("import_statement", NodeKind::Import, false),
            ("export_statement", NodeKind::Export, false),
            ("variable_declarator", NodeKind::Variable, false),
            ("public_field_definition", NodeKind::Field, false),
            ("property_signature", NodeKind::Property, false),
        ],
        call_kinds: &[
            (
                "call_expression",
                CalleeSource::Field {
                    callee: "function",
                    receiver: None,
                },
            ),
            (
                "new_expression",
                CalleeSource::Field {
                    callee: "constructor",
                    receiver: None,
                },
            ),
        ],
        import_kinds: &["import_statement"],
        instantiation_kinds: &["new_expression"],
        heritage: Some(&[
            HeritageRule {
                source: HeritageSource::ChildField("extends_clause", "value"),
                kind: HeritageKind::Extends,
            },
            HeritageRule {
                source: HeritageSource::ChildIsType("implements_clause"),
                kind: HeritageKind::Implements,
            },
        ]),
    },
    LanguageContract {
        language: Language::JavaScript,
        kinds: &[
            ("function_declaration", NodeKind::Function, false),
            ("arrow_function", NodeKind::Function, false),
            ("function_expression", NodeKind::Function, false),
            ("generator_function_declaration", NodeKind::Function, false),
            ("class_declaration", NodeKind::Class, true),
            ("method_definition", NodeKind::Method, false),
            ("import_statement", NodeKind::Import, false),
            ("export_statement", NodeKind::Export, false),
            ("variable_declarator", NodeKind::Variable, false),
            ("field_definition", NodeKind::Field, false),
        ],
        call_kinds: &[
            (
                "call_expression",
                CalleeSource::Field {
                    callee: "function",
                    receiver: None,
                },
            ),
            (
                "new_expression",
                CalleeSource::Field {
                    callee: "constructor",
                    receiver: None,
                },
            ),
        ],
        import_kinds: &["import_statement"],
        instantiation_kinds: &["new_expression"],
        heritage: Some(&[HeritageRule {
            source: HeritageSource::ChildField("extends_clause", "value"),
            kind: HeritageKind::Extends,
        }]),
    },
    LanguageContract {
        language: Language::Python,
        kinds: &[
            ("function_definition", NodeKind::Function, false),
            ("class_definition", NodeKind::Class, true),
            ("decorated_definition", NodeKind::Function, false),
            ("import_statement", NodeKind::Import, false),
            ("import_from_statement", NodeKind::Import, false),
            ("assignment", NodeKind::Variable, false),
            ("augmented_assignment", NodeKind::Variable, false),
        ],
        call_kinds: &[(
            "call",
            CalleeSource::Field {
                callee: "function",
                receiver: None,
            },
        )],
        import_kinds: &["import_statement", "import_from_statement"],
        instantiation_kinds: &[],
        heritage: Some(&[HeritageRule {
            source: HeritageSource::Field("superclasses"),
            kind: HeritageKind::Implements,
        }]),
    },
    LanguageContract {
        language: Language::Go,
        kinds: &[
            ("function_declaration", NodeKind::Function, false),
            ("method_declaration", NodeKind::Method, false),
            ("type_declaration", NodeKind::Class, true),
            ("type_spec", NodeKind::Struct, true),
            ("const_declaration", NodeKind::Constant, false),
            ("var_declaration", NodeKind::Variable, false),
            ("import_declaration", NodeKind::Import, false),
        ],
        call_kinds: &[(
            "call_expression",
            CalleeSource::Field {
                callee: "function",
                receiver: None,
            },
        )],
        import_kinds: &["import_declaration"],
        instantiation_kinds: &[],
        heritage: None,
    },
    LanguageContract {
        language: Language::Java,
        kinds: &[
            ("class_declaration", NodeKind::Class, true),
            ("interface_declaration", NodeKind::Interface, true),
            ("enum_declaration", NodeKind::Enum, true),
            ("record_declaration", NodeKind::Class, true),
            ("method_declaration", NodeKind::Method, false),
            ("constructor_declaration", NodeKind::Method, false),
            ("field_declaration", NodeKind::Field, false),
            ("import_declaration", NodeKind::Import, false),
        ],
        call_kinds: &[
            (
                "method_invocation",
                CalleeSource::Field {
                    callee: "name",
                    receiver: Some("object"),
                },
            ),
            (
                "object_creation_expression",
                CalleeSource::Field {
                    callee: "type",
                    receiver: None,
                },
            ),
        ],
        import_kinds: &["import_declaration"],
        instantiation_kinds: &["object_creation_expression"],
        heritage: Some(&[
            HeritageRule {
                source: HeritageSource::Field("superclass"),
                kind: HeritageKind::Extends,
            },
            HeritageRule {
                source: HeritageSource::Field("interfaces"),
                kind: HeritageKind::Implements,
            },
        ]),
    },
    LanguageContract {
        language: Language::C,
        kinds: &[
            ("function_definition", NodeKind::Function, false),
            ("struct_specifier", NodeKind::Struct, true),
            ("union_specifier", NodeKind::Struct, true),
            ("enum_specifier", NodeKind::Enum, true),
            ("type_definition", NodeKind::TypeAlias, false),
            ("declaration", NodeKind::Variable, false),
            ("preproc_include", NodeKind::Import, false),
            ("preproc_def", NodeKind::Constant, false),
        ],
        call_kinds: &[(
            "call_expression",
            CalleeSource::Field {
                callee: "function",
                receiver: None,
            },
        )],
        import_kinds: &["preproc_include"],
        instantiation_kinds: &[],
        heritage: None,
    },
    LanguageContract {
        language: Language::Cpp,
        kinds: &[
            ("function_definition", NodeKind::Function, false),
            ("class_specifier", NodeKind::Class, true),
            ("struct_specifier", NodeKind::Struct, true),
            ("union_specifier", NodeKind::Struct, true),
            ("enum_specifier", NodeKind::Enum, true),
            ("type_definition", NodeKind::TypeAlias, false),
            ("declaration", NodeKind::Variable, false),
            ("namespace_definition", NodeKind::Namespace, true),
            ("preproc_include", NodeKind::Import, false),
            ("preproc_def", NodeKind::Constant, false),
            ("field_declaration", NodeKind::Field, false),
        ],
        call_kinds: &[(
            "call_expression",
            CalleeSource::Field {
                callee: "function",
                receiver: None,
            },
        )],
        import_kinds: &["preproc_include"],
        instantiation_kinds: &[],
        heritage: Some(&[HeritageRule {
            source: HeritageSource::ChildField("base_class_clause", "type"),
            kind: HeritageKind::Extends,
        }]),
    },
    LanguageContract {
        language: Language::CSharp,
        kinds: &[
            ("class_declaration", NodeKind::Class, true),
            ("interface_declaration", NodeKind::Interface, true),
            ("struct_declaration", NodeKind::Struct, true),
            ("record_declaration", NodeKind::Class, true),
            ("enum_declaration", NodeKind::Enum, true),
            ("method_declaration", NodeKind::Method, false),
            ("constructor_declaration", NodeKind::Method, false),
            ("property_declaration", NodeKind::Property, false),
            ("field_declaration", NodeKind::Field, false),
            ("using_directive", NodeKind::Import, false),
            ("namespace_declaration", NodeKind::Namespace, true),
            (
                "file_scoped_namespace_declaration",
                NodeKind::Namespace,
                true,
            ),
        ],
        call_kinds: &[
            (
                "invocation_expression",
                CalleeSource::Field {
                    callee: "function",
                    receiver: None,
                },
            ),
            (
                "object_creation_expression",
                CalleeSource::Field {
                    callee: "type",
                    receiver: None,
                },
            ),
        ],
        import_kinds: &["using_directive"],
        instantiation_kinds: &["object_creation_expression"],
        heritage: Some(&[HeritageRule {
            source: HeritageSource::ChildIsType("base_list"),
            kind: HeritageKind::Implements,
        }]),
    },
    LanguageContract {
        language: Language::Php,
        kinds: &[
            ("class_declaration", NodeKind::Class, true),
            ("interface_declaration", NodeKind::Interface, true),
            ("trait_declaration", NodeKind::Trait, true),
            ("enum_declaration", NodeKind::Enum, true),
            ("method_declaration", NodeKind::Method, false),
            ("function_definition", NodeKind::Function, false),
            ("namespace_definition", NodeKind::Namespace, true),
            ("namespace_use_declaration", NodeKind::Import, false),
        ],
        call_kinds: &[
            (
                "function_call_expression",
                CalleeSource::Field {
                    callee: "function",
                    receiver: None,
                },
            ),
            (
                "member_call_expression",
                CalleeSource::Field {
                    callee: "name",
                    receiver: Some("object"),
                },
            ),
            (
                "scoped_call_expression",
                CalleeSource::Field {
                    callee: "name",
                    receiver: Some("scope"),
                },
            ),
            (
                "nullsafe_member_call_expression",
                CalleeSource::Field {
                    callee: "name",
                    receiver: Some("object"),
                },
            ),
            (
                "object_creation_expression",
                CalleeSource::FirstChildLastIdentifier,
            ),
        ],
        import_kinds: &["namespace_use_declaration"],
        instantiation_kinds: &["object_creation_expression"],
        heritage: Some(&[HeritageRule {
            source: HeritageSource::ChildIsType("base_clause"),
            kind: HeritageKind::Implements,
        }]),
    },
    LanguageContract {
        language: Language::Ruby,
        kinds: &[
            ("method", NodeKind::Method, false),
            ("singleton_method", NodeKind::Method, false),
            ("class", NodeKind::Class, true),
            ("module", NodeKind::Namespace, true),
            ("assignment", NodeKind::Variable, false),
        ],
        call_kinds: &[(
            "call",
            CalleeSource::Field {
                callee: "method",
                receiver: Some("receiver"),
            },
        )],
        import_kinds: &[],
        instantiation_kinds: &[],
        heritage: Some(&[HeritageRule {
            source: HeritageSource::Field("superclass"),
            kind: HeritageKind::Extends,
        }]),
    },
    LanguageContract {
        language: Language::Swift,
        kinds: &[
            ("function_declaration", NodeKind::Function, false),
            ("init_declaration", NodeKind::Method, false),
            ("deinit_declaration", NodeKind::Method, false),
            ("protocol_function_declaration", NodeKind::Method, false),
            ("class_declaration", NodeKind::Class, true),
            ("protocol_declaration", NodeKind::Protocol, true),
            ("property_declaration", NodeKind::Property, false),
            ("import_declaration", NodeKind::Import, false),
            ("typealias_declaration", NodeKind::TypeAlias, false),
        ],
        call_kinds: &[("call_expression", CalleeSource::FirstChildLastIdentifier)],
        import_kinds: &["import_declaration"],
        instantiation_kinds: &[],
        heritage: Some(&[HeritageRule {
            source: HeritageSource::ChildNested("inheritance_specifier", "user_type"),
            kind: HeritageKind::Implements,
        }]),
    },
    LanguageContract {
        language: Language::Kotlin,
        kinds: &[
            ("function_declaration", NodeKind::Function, false),
            ("secondary_constructor", NodeKind::Method, false),
            ("property_declaration", NodeKind::Property, false),
            ("class_declaration", NodeKind::Class, true),
            ("object_declaration", NodeKind::Class, true),
            ("companion_object", NodeKind::Class, true),
            ("enum_entry", NodeKind::EnumMember, false),
            ("type_alias", NodeKind::TypeAlias, false),
            ("import", NodeKind::Import, false),
            ("package_header", NodeKind::Module, false),
        ],
        call_kinds: &[("call_expression", CalleeSource::FirstChildLastIdentifier)],
        import_kinds: &["import"],
        instantiation_kinds: &[],
        heritage: Some(&[HeritageRule {
            source: HeritageSource::ChildInContainer(
                "delegation_specifiers",
                "delegation_specifier",
                "user_type",
            ),
            kind: HeritageKind::Implements,
        }]),
    },
];
