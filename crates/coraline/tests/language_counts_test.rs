#![allow(clippy::expect_used)]

//! Pins the language counts, and checks the documentation against them.
//!
//! Coraline supports a language in three stages, and the stages are easy
//! to confuse:
//!
//! 1. **Known** — the language has a variant in the `Language` enum.
//! 2. **Has a grammar** — a tree-sitter grammar crate is a dependency, so
//!    a file in the language can be parsed.
//! 3. **Indexed** — the language has an extraction contract, so the
//!    parser's output becomes graph nodes.
//!
//! Only stage 3 produces a usable index. A language at stage 2 parses
//! perfectly and still yields nothing but a `file` node, because
//! `map_kind` has no contract to map the grammar's node kinds against.
//! That gap is issue #128.
//!
//! The counts here are duplicated in the README, in
//! `docs/book/src/languages.md`, and in the table the book page renders.
//! This test fails when they disagree, so a language being added cannot
//! silently make the prose wrong again.

use coraline::config::is_language_supported;
use coraline::grammar::contract_for;
use coraline::types::Language;

/// Every language Coraline knows about, minus the `Unknown` catch-all.
///
/// `Unknown` is not a language: it is what an unrecognised extension
/// maps to, so it has no grammar and no contract.
const KNOWN: &[Language] = &[
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
    Language::Blazor,
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
    Language::Markdown,
    Language::Matlab,
    Language::Nix,
    Language::Perl,
    Language::Powershell,
    Language::R,
    Language::Scala,
    Language::Toml,
    Language::Yaml,
    Language::Zig,
];

/// Languages with no tree-sitter grammar, parsed by hand instead.
///
/// `Liquid` is the only one: it has no grammar crate, and the extractor
/// handles it with a bespoke parser.
const NO_GRAMMAR: &[Language] = &[Language::Liquid];

/// The count of languages Coraline knows about.
const KNOWN_COUNT: usize = 37;

/// The count of languages that ship a tree-sitter grammar.
const WITH_GRAMMAR_COUNT: usize = 36;

/// The count of languages that are indexed end to end.
const INDEXED_COUNT: usize = 37;

/// The count of languages that have a grammar but no extraction contract.
const PENDING_COUNT: usize = 0;

#[test]
fn known_languages_are_enumerated() {
    assert_eq!(
        KNOWN.len(),
        KNOWN_COUNT,
        "the known-language list must match the documented count"
    );
}

/// The default include patterns and `is_language_supported` must agree.
///
/// If a language is indexed, its extensions are in the default patterns;
/// if it is not, they are not. A disagreement means a language is
/// either silently skipped or silently produces an empty file node.
#[test]
fn default_include_patterns_agree_with_is_language_supported() {
    for language in KNOWN {
        assert_eq!(
            is_included_by_default(*language),
            is_language_supported(language),
            "{language:?}: the default include patterns and is_language_supported disagree"
        );
    }
}

/// Every default include pattern must name an extension the detector
/// recognises.
///
/// A pattern that names an extension the detector does not know is the
/// quietest failure there is: the file *is* scanned, found to be an
/// unknown language, and dropped. It contributes a `file` node and
/// nothing else, and nothing reports an error.
///
/// This is not hypothetical — `**/*.psm1` was in the default patterns
/// while the detector only knew `ps1`, so every PowerShell module file
/// was silently skipped.
///
/// The check runs in both directions: a pattern naming an unknown
/// extension, and a known extension with no pattern to match it.
#[test]
fn include_patterns_agree_with_the_extension_table() {
    let patterns = coraline::config::default_include_patterns();

    for pattern in &patterns {
        let Some(extension) = pattern.strip_prefix("**/*.") else {
            continue;
        };
        assert!(
            coraline::config::EXTENSION_LANGUAGES
                .iter()
                .any(|(ext, _)| ext.eq_ignore_ascii_case(extension)),
            "{pattern}: the default include patterns name an extension the detector does not recognise"
        );
    }

    for (extension, language) in coraline::config::EXTENSION_LANGUAGES {
        if !is_language_supported(language) {
            continue;
        }
        assert!(
            patterns.iter().any(|p| p
                .strip_prefix("**/*.")
                .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))),
            "{extension}: the extension table maps it to {language:?}, which is indexed, but no default include pattern matches it"
        );
    }
}
/// A language with a contract must be indexed.
///
/// This is the other direction of
/// `every_indexed_language_has_a_contract_or_is_markup`, and it is the
/// one that was missing when this issue was found. The two lists — the
/// contracts and `is_language_supported` — were checked in one
/// direction only, so they could drift apart, and did: a language
/// gained a contract but was never added to `is_language_supported`, so
/// its files were never scanned and the contract was dead code.
///
/// A contract for a language that is not indexed is not harmless. It
/// says the language *should* produce a graph, and the graph builder
/// is the only thing that knows it.
#[test]
fn every_contracted_language_is_indexed() {
    for contract in coraline::grammar::CONTRACTS {
        assert!(
            is_language_supported(&contract.language),
            "{:?}: it has an extraction contract but is not in is_language_supported, so its files are never scanned",
            contract.language
        );
    }
}

#[test]
fn grammar_backed_languages_are_counted() {
    let with_grammar = KNOWN
        .iter()
        .filter(|language| !NO_GRAMMAR.contains(language))
        .count();
    assert_eq!(
        with_grammar, WITH_GRAMMAR_COUNT,
        "the number of languages with a tree-sitter grammar changed"
    );
}

#[test]
fn indexed_languages_are_counted() {
    let indexed = KNOWN
        .iter()
        .filter(|language| is_language_supported(language))
        .count();
    assert_eq!(
        indexed, INDEXED_COUNT,
        "the number of indexed languages changed"
    );
}

#[test]
fn pending_languages_are_counted() {
    let pending = KNOWN
        .iter()
        .filter(|language| !is_language_supported(language))
        .count();
    assert_eq!(
        pending, PENDING_COUNT,
        "the number of languages with a grammar but no contract changed"
    );
}

/// Every indexed language either has a contract, or is markup.
///
/// Liquid and Markdown are indexed without one: they have no
/// declarations to extract.
#[test]
fn every_indexed_language_has_a_contract_or_is_markup() {
    const MARKUP: &[Language] = &[Language::Markdown, Language::Liquid];

    for language in KNOWN.iter().filter(|l| is_language_supported(l)) {
        if MARKUP.contains(language) {
            continue;
        }
        assert!(
            contract_for(*language).is_some(),
            "{language:?} is indexed but has no extraction contract"
        );
    }
}

/// The contract count is deliberately lower than the indexed count.
///
/// `contract_for` maps TSX onto the TypeScript contract and JSX onto
/// the JavaScript one, so 33 declared contracts cover 35 languages.
/// Together with the two markup languages that makes 37 indexed.
#[test]
fn contracts_cover_the_indexed_languages_with_declarations() {
    let declared = coraline::grammar::CONTRACTS.len();
    let mapped = 2; // TSX -> TypeScript, JSX -> JavaScript
    let markup = 2; // Liquid, Markdown

    assert_eq!(
        declared + mapped + markup,
        INDEXED_COUNT,
        "the contract, mapping and markup counts must add up to the indexed count"
    );
}

/// The language table in the book must list every known language, and
/// must agree with the code about which ones are indexed.
#[test]
fn book_language_table_matches_the_code() {
    let book =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/book/src/languages.md");
    let text = std::fs::read_to_string(&book).expect("read the language page");

    let mut rows = 0_usize;
    let mut indexed = 0_usize;
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("| ") else {
            continue;
        };
        // Skip the header and the separator row.
        if rest.starts_with("Language |") || rest.starts_with("---") {
            continue;
        }
        let Some((name, _)) = rest.split_once(" | ") else {
            continue;
        };
        rows += 1;

        let is_indexed = rest.contains("✅");
        if is_indexed {
            indexed += 1;
        }

        let Some(language) = book_table_language(name) else {
            unreachable!("the book table lists an unknown language: {name}");
        };
        assert!(
            is_indexed == is_language_supported(&language),
            "the book table marks {name} as indexed={is_indexed}, but the code says {}",
            is_language_supported(&language)
        );
    }

    assert_eq!(
        rows, KNOWN_COUNT,
        "the book table must list every known language"
    );
    assert_eq!(
        indexed, INDEXED_COUNT,
        "the book table must mark the same languages as indexed as the code does"
    );
}

/// Map a name from the book table to a `Language` variant.
///
/// The table uses prose names (`C++`, `C#`, `TypeScript`); the enum uses
/// Rust identifiers (`Cpp`, `CSharp`).
///
/// Returns `None` for a name the table should not contain, so the
/// caller can report it rather than panicking.
const fn book_table_language(name: &str) -> Option<Language> {
    match name.as_bytes() {
        b"TypeScript" => Some(Language::TypeScript),
        b"TSX" => Some(Language::Tsx),
        b"JavaScript" => Some(Language::JavaScript),
        b"JSX" => Some(Language::Jsx),
        b"Python" => Some(Language::Python),
        b"Go" => Some(Language::Go),
        b"Rust" => Some(Language::Rust),
        b"Java" => Some(Language::Java),
        b"C" => Some(Language::C),
        b"C++" => Some(Language::Cpp),
        b"C#" => Some(Language::CSharp),
        b"PHP" => Some(Language::Php),
        b"Ruby" => Some(Language::Ruby),
        b"Swift" => Some(Language::Swift),
        b"Kotlin" => Some(Language::Kotlin),
        b"Liquid" => Some(Language::Liquid),
        b"Blazor" => Some(Language::Blazor),
        b"Markdown" => Some(Language::Markdown),
        b"Perl" => Some(Language::Perl),
        b"Bash" => Some(Language::Bash),
        b"Dart" => Some(Language::Dart),
        b"Elixir" => Some(Language::Elixir),
        b"Elm" => Some(Language::Elm),
        b"Erlang" => Some(Language::Erlang),
        b"Fortran" => Some(Language::Fortran),
        b"Groovy" => Some(Language::Groovy),
        b"Haskell" => Some(Language::Haskell),
        b"Julia" => Some(Language::Julia),
        b"Lua" => Some(Language::Lua),
        b"MATLAB" => Some(Language::Matlab),
        b"Nix" => Some(Language::Nix),
        b"PowerShell" => Some(Language::Powershell),
        b"R" => Some(Language::R),
        b"Scala" => Some(Language::Scala),
        b"TOML" => Some(Language::Toml),
        b"YAML" => Some(Language::Yaml),
        b"Zig" => Some(Language::Zig),
        _ => None,
    }
}

/// Whether a language is picked up by the default include patterns.
///
/// This is the signal that a language is *indexed*, not that it has a
/// grammar: a grammar-backed language that is not indexed is still
/// excluded from the default patterns, which is exactly the
/// stage-2-but-not-stage-3 case.
fn is_included_by_default(language: Language) -> bool {
    let patterns = coraline::config::default_include_patterns();
    let extensions = known_extensions(language);
    !extensions.is_empty()
        && extensions
            .iter()
            .all(|ext| patterns.iter().any(|p| p.ends_with(&format!("*.{ext}"))))
}

/// The file extensions the book table lists for a language.
const fn known_extensions(language: Language) -> &'static [&'static str] {
    match language {
        Language::TypeScript => &["ts"],
        Language::Tsx => &["tsx"],
        Language::JavaScript => &["js"],
        Language::Jsx => &["jsx"],
        Language::Python => &["py"],
        Language::Go => &["go"],
        Language::Rust => &["rs"],
        Language::Java => &["java"],
        Language::C => &["c", "h"],
        Language::Cpp => &["cpp", "cc", "cxx", "hpp"],
        Language::CSharp => &["cs"],
        Language::Php => &["php"],
        Language::Ruby => &["rb"],
        Language::Swift => &["swift"],
        Language::Kotlin => &["kt", "kts"],
        Language::Liquid => &["liquid"],
        Language::Blazor => &["razor", "cshtml"],
        Language::Markdown => &["md", "markdown"],
        Language::Perl => &["pl", "pm"],
        Language::Bash => &["sh", "bash"],
        Language::Dart => &["dart"],
        Language::Elixir => &["ex", "exs"],
        Language::Elm => &["elm"],
        Language::Erlang => &["erl", "hrl"],
        Language::Fortran => &["f", "for", "f90", "f95", "f03", "f08"],
        Language::Groovy => &["groovy", "gradle"],
        Language::Haskell => &["hs"],
        Language::Julia => &["jl"],
        Language::Lua => &["lua"],
        Language::Matlab => &["m"],
        Language::Nix => &["nix"],
        Language::Powershell => &["ps1", "psm1"],
        Language::R => &["r", "R"],
        Language::Scala => &["scala", "sc"],
        Language::Toml => &["toml"],
        Language::Yaml => &["yml", "yaml"],
        Language::Zig => &["zig"],
        Language::Unknown => &[],
    }
}
