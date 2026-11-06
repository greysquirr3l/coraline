//! Recovery of declaration documentation and signatures.
//!
//! Two node fields depend on the *text* of a declaration rather than on its
//! shape:
//!
//! * `Node::docstring` — the documentation attached to a declaration.
//! * `Node::signature` — the declaration's header, i.e. the source text
//!   from the start of the declaration up to the start of its body.
//!
//! Both are driven entirely by the per-language [`LanguageContract`] in
//! [`crate::grammar`], so the grammar-specific knowledge (which node kinds
//! carry a comment, which field holds the body, which markers a comment
//! uses) stays declarative and is guarded by
//! `tests/grammar_contract_test.rs`.
//!
//! The two MCP tools that depend on these fields —
//! `coraline_search_by_docstring` and `coraline_search_by_signature` —
//! were dead until this module filled them in.

use tree_sitter::Node;

use crate::grammar::{CommentSpec, DocSpec, SignatureSpec, contract_for};
use crate::types::Language;

/// The documentation attached to a declaration, if the language has any.
///
/// The lookup follows the language's [`DocSpec`]:
///
/// * [`DocSpec::None`] — the language has no documentation concept.
/// * [`DocSpec::PrecedingComment`] — the documentation is the run of
///   comment nodes that immediately precedes the declaration.
/// * [`DocSpec::BodyFirstString`] — the documentation is the first string
///   literal in the declaration's body (Python's docstring).
pub fn docstring_for(node: &Node, source: &str, language: Language) -> Option<String> {
    let contract = contract_for(language)?;
    match contract.doc {
        DocSpec::None => None,
        DocSpec::BodyFirstString { body_field } => body_first_string(node, source, body_field),
        DocSpec::PrecedingComment { comment, wrappers } => {
            preceding_comment(node, source, &comment, wrappers)
        }
    }
}

/// The signature of a declaration: its header, without its body.
///
/// For Rust `pub async fn plan<T>(x: u32) -> Result<T, E> { … }` the
/// signature is `pub async fn plan<T>(x: u32) -> Result<T, E>`. The text is
/// whitespace-normalised so that a multi-line header stays on one line and
/// remains searchable.
pub fn signature_for(node: &Node, source: &str, language: Language) -> Option<String> {
    let contract = contract_for(language)?;
    let text = signature_text(node, source, &contract.signature)?;
    let normalised = normalise_whitespace(&text);
    (!normalised.is_empty()).then_some(normalised)
}

/// The documentation of the scope a file declares — Rust's `//!` module
/// docs.
///
/// Rust's `//!` comments document the module they appear in rather than the
/// item that follows them, so they belong to the file node rather than to
/// any declaration inside it. The lookup takes the run of comments at the
/// very top of the file that the language's `scope_doc` spec accepts.
pub fn scope_docstring_for(root: &Node, source: &str, language: Language) -> Option<String> {
    let contract = contract_for(language)?;
    let spec = contract.scope_doc?;

    let mut parts = Vec::new();
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        if !spec.kinds.contains(&child.kind()) {
            break;
        }
        if !has_marker_field(&child, spec.marker_field) {
            break;
        }
        match comment_text(&child, source, &spec) {
            Some(text) => parts.push(text),
            None => break,
        }
    }

    join_parts(&parts)
}

/// The documentation of the comment run that immediately precedes a
/// declaration.
///
/// The declaration may be wrapped — TypeScript's `export function add(…)`
/// parses as an `export_statement` around the `function_declaration` — in
/// which case the comment precedes the *wrapper*. The run stops at the
/// first sibling that is not a documentation comment, so an unrelated
/// comment further up the file is never mistaken for the declaration's
/// documentation.
fn preceding_comment(
    node: &Node,
    source: &str,
    spec: &CommentSpec,
    wrappers: &[&str],
) -> Option<String> {
    let anchor = wrapper_anchor(node, wrappers);

    let mut parts = Vec::new();
    let mut sibling = anchor.prev_named_sibling();
    // The row the item before the candidate starts on, used to spot a
    // blank line between two comments.
    let mut previous_start_row = anchor.start_position().row;
    while let Some(candidate) = sibling {
        if !spec.kinds.contains(&candidate.kind()) {
            break;
        }
        if !has_marker_field(&candidate, spec.marker_field) {
            break;
        }
        // A blank line between two comments means they are not one
        // documentation block: the run stops at the blank line. Ruby's
        // `# frozen_string_literal: true` magic comment sits above the
        // file's real documentation, and this is what keeps it out.
        if !parts.is_empty() && candidate.end_position().row + 1 < previous_start_row {
            break;
        }
        match comment_text(&candidate, source, spec) {
            Some(text) => parts.push(text),
            None => break,
        }
        previous_start_row = candidate.start_position().row;
        sibling = candidate.prev_named_sibling();
    }

    // The walk ran backwards through the comments, so put them back in
    // source order before joining them.
    parts.reverse();
    join_parts(&parts)
}

/// The node whose preceding siblings hold the declaration's documentation.
///
/// For a wrapped declaration that is the wrapper; otherwise it is the
/// declaration itself.
fn wrapper_anchor<'a>(node: &Node<'a>, wrappers: &[&str]) -> Node<'a> {
    if wrappers.is_empty() {
        return *node;
    }
    node.parent()
        .filter(|parent| wrappers.contains(&parent.kind()))
        .unwrap_or(*node)
}

/// The bare documentation text of a comment node.
fn comment_text(node: &Node, source: &str, spec: &CommentSpec) -> Option<String> {
    let raw = match spec.text_field {
        // The grammar already separated the text from the marker.
        Some(field) => {
            let text_node = node.child_by_field_name(field)?;
            source.get(text_node.byte_range())?.to_string()
        }
        None => source.get(node.byte_range())?.to_string(),
    };

    // A `#!` comment is an interpreter directive — a shell shebang — and
    // never documentation.
    if raw.trim_start().starts_with("#!") {
        return None;
    }

    let cleaned = match spec.text_field {
        Some(_) => raw.trim().to_string(),
        None => strip_comment_markers(&raw, spec.openers, spec.closers),
    };

    (!cleaned.is_empty()).then_some(cleaned)
}

/// Whether a comment node carries the marker field the contract requires.
///
/// Rust's grammar uses the `outer` field to tell `///` (documentation for
/// the item that follows) from `//!` (documentation for the enclosing
/// module), and the `inner` field for the reverse.
fn has_marker_field(node: &Node, marker_field: Option<&str>) -> bool {
    marker_field.is_none_or(|field| node.child_by_field_name(field).is_some())
}

/// Strip the comment markers from a raw comment's text.
///
/// The opener is removed from the front and the closer from the end, both
/// matched longest-first. A block comment whose every non-empty line starts
/// with `*` — the `JSDoc` and Haddock shape — also loses those stars, so
/// `/**\n * Adds two numbers.\n */` reads as `Adds two numbers.`.
fn strip_comment_markers(raw: &str, openers: &[&str], closers: &[&str]) -> String {
    let mut text = raw.trim();

    let mut opened = false;
    for opener in openers {
        if let Some(rest) = text.strip_prefix(opener) {
            text = rest;
            opened = true;
            break;
        }
    }

    for closer in closers {
        if let Some(rest) = text.strip_suffix(closer) {
            text = rest;
            break;
        }
    }

    if opened && text.contains('\n') && every_line_starts_with_star(text) {
        let stripped = text
            .lines()
            .map(|line| {
                let trimmed = line.trim_start();
                trimmed
                    .strip_prefix('*')
                    .unwrap_or(trimmed)
                    .trim_start()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");
        return stripped.trim().to_string();
    }

    text.trim().to_string()
}

/// Whether every non-empty line of `text` starts with `*`.
fn every_line_starts_with_star(text: &str) -> bool {
    let mut saw_line = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !trimmed.starts_with('*') {
            return false;
        }
        saw_line = true;
    }
    saw_line
}

/// The documentation of a declaration that carries it as the first string
/// literal of its body — Python's docstring convention.
fn body_first_string(node: &Node, source: &str, body_field: &str) -> Option<String> {
    let body = node.child_by_field_name(body_field)?;

    let mut cursor = body.walk();
    for statement in body.named_children(&mut cursor) {
        // A comment may precede the docstring without being a statement.
        if statement.kind() == "string" {
            return string_literal_text(&statement, source);
        }
        if statement.kind() != "expression_statement" {
            continue;
        }
        let mut inner = statement.walk();
        for child in statement.named_children(&mut inner) {
            if child.kind() == "string" {
                return string_literal_text(&child, source);
            }
        }
        return None;
    }

    None
}

/// The text of a string literal, without its quotes.
fn string_literal_text(node: &Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "string_content" {
            let text = source.get(child.byte_range())?;
            return Some(text.trim().to_string());
        }
    }
    None
}

/// The signature text of a declaration, before whitespace normalisation.
fn signature_text(node: &Node, source: &str, spec: &SignatureSpec) -> Option<String> {
    // Some grammars put the header in a node of its own — Fortran's
    // `subroutine` wraps its header in a `subroutine_statement`.
    if !spec.header_kinds.is_empty() {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if spec.header_kinds.contains(&child.kind()) {
                return source.get(child.byte_range()).map(str::to_string);
            }
        }
    }

    let end = signature_body(node, spec).map_or_else(|| node.end_byte(), |body| body.start_byte());
    source.get(node.byte_range().start..end).map(str::to_string)
}

/// The node that starts a declaration's body, and therefore ends its
/// signature.
fn signature_body<'a>(node: &Node<'a>, spec: &SignatureSpec) -> Option<Node<'a>> {
    let body = spec
        .body_path
        .and_then(|path| resolve_field_path(node, path));
    if let Some(body) = body {
        return Some(body);
    }

    if !spec.body_kinds.is_empty() {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if spec.body_kinds.contains(&child.kind()) {
                return Some(child);
            }
        }
    }

    None
}

/// Resolve a dotted field path such as `clause.body` against a node.
///
/// A path exists because some grammars hide the body behind an
/// intermediate node: Erlang's `fun_decl` keeps its clause in a `clause`
/// field, and the clause keeps its body in a `body` field.
fn resolve_field_path<'a>(node: &Node<'a>, path: &str) -> Option<Node<'a>> {
    let mut current = *node;
    for segment in path.split('.') {
        current = current.child_by_field_name(segment)?;
    }
    Some(current)
}

/// Collapse every run of whitespace into a single space.
fn normalise_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Join the parts of a documentation block into one string.
fn join_parts(parts: &[String]) -> Option<String> {
    if parts.is_empty() {
        return None;
    }
    let joined = parts.join("\n");
    (!joined.trim().is_empty()).then_some(joined)
}
