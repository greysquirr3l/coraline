#![allow(clippy::expect_used)]

//! Integration tests for the extraction contracts of Fortran, Haskell,
//! Julia, Lua, MATLAB and R.
//!
//! A contract is only worth anything if it turns a parse tree into graph
//! nodes, so each test here indexes a fixture and asserts the graph the
//! contract is supposed to produce: the declarations, the call edges
//! between them, and the language-specific shapes — a Fortran `use`, a
//! Julia `using`, a Haskell `deriving` clause — that the contract
//! encodes.

use std::path::Path;

use coraline::{config, db, extraction};
use rusqlite::Connection;
use tempfile::TempDir;

/// Create an empty project database.
fn setup_test_db() -> (TempDir, String) {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let project_root = temp_dir
        .path()
        .to_str()
        .expect("Failed to convert path to string")
        .to_string();

    db::initialize_database(temp_dir.path()).expect("Failed to initialize database");

    (temp_dir, project_root)
}

/// Index a fixture directory into a fresh project and open its database.
fn index_fixture(fixture: &str) -> (TempDir, Connection) {
    let (temp, project_root) = setup_test_db();
    let project_path = Path::new(&project_root);

    let fixture_src = Path::new("tests/fixtures").join(fixture);
    let fixture_dst = project_path.join("src");
    std::fs::create_dir_all(&fixture_dst).expect("Failed to create fixture directory");

    for entry in std::fs::read_dir(&fixture_src).expect("Failed to read fixture directory") {
        let entry = entry.expect("Failed to read directory entry");
        let dest = fixture_dst.join(entry.file_name());
        std::fs::copy(entry.path(), dest).expect("Failed to copy fixture file");
    }

    let cfg = config::create_default_config(project_path);
    let result =
        extraction::index_all(project_path, &cfg, false, None).expect("Failed to index project");

    assert!(result.files_indexed > 0, "Should index at least one file");
    assert!(result.nodes_created > 0, "Should create at least one node");

    let conn = db::open_database(project_path).expect("Failed to open database");
    (temp, conn)
}

/// The names of every node of a kind.
fn nodes_of_kind(conn: &Connection, kind: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM nodes WHERE kind = ?1")
        .expect("Failed to prepare the node query");
    stmt.query_map([kind], |row| row.get::<_, String>(0))
        .expect("Failed to query the nodes")
        .filter_map(Result::ok)
        .collect()
}

/// The kind of the first edge between two named nodes.
fn edge_kind(conn: &Connection, source: &str, target: &str) -> Option<String> {
    let mut stmt = conn
        .prepare(
            "SELECT e.kind FROM edges e
             JOIN nodes s ON s.id = e.source
             JOIN nodes t ON t.id = e.target
             WHERE s.name = ?1 AND t.name = ?2",
        )
        .expect("Failed to prepare the edge query");
    stmt.query_map([source, target], |row| row.get::<_, String>(0))
        .expect("Failed to query the edges")
        .find_map(Result::ok)
}

/// Assert that a node of `kind` named `name` exists.
#[track_caller]
fn assert_node(conn: &Connection, kind: &str, name: &str) {
    let nodes = nodes_of_kind(conn, kind);
    assert!(
        nodes.iter().any(|n| n == name),
        "Should find a '{kind}' node named '{name}', got {nodes:?}"
    );
}

/// Assert that an edge of `kind` runs from `source` to `target`.
#[track_caller]
fn assert_edge(conn: &Connection, kind: &str, source: &str, target: &str) {
    assert_eq!(
        edge_kind(conn, source, target).as_deref(),
        Some(kind),
        "Expected a '{kind}' edge from '{source}' to '{target}'"
    );
}

/// The module path an import node names.
///
/// The node's own name is the last segment of the path (`mutable`), and
/// the path itself is the tail of its qualified name.
fn import_module_path(conn: &Connection, name: &str) -> Option<String> {
    let mut stmt = conn
        .prepare(
            "SELECT qualified_name FROM nodes
             WHERE kind = 'import' AND name = ?1
             ORDER BY id
             LIMIT 1",
        )
        .expect("Failed to prepare the import query");
    stmt.query_map([name], |row| row.get::<_, String>(0))
        .expect("Failed to query the imports")
        .find_map(Result::ok)
        .map(|qualified| {
            qualified
                .rsplit("::")
                .next()
                .unwrap_or_default()
                .to_string()
        })
}

/// The qualifier recorded with an unresolved reference.
fn unresolved_qualifier(conn: &Connection, name: &str) -> Option<String> {
    let mut stmt = conn
        .prepare(
            "SELECT qualifier FROM unresolved_refs
             WHERE reference_name = ?1
             ORDER BY id
             LIMIT 1",
        )
        .expect("Failed to prepare the unresolved query");
    stmt.query_map([name], |row| row.get::<_, Option<String>>(0))
        .expect("Failed to query the unresolved references")
        .find_map(Result::ok)
        .flatten()
}

#[test]
fn test_extract_fortran_module() {
    let (_temp, conn) = index_fixture("fortran-module");

    assert_node(&conn, "module", "geometry");
    assert_node(&conn, "function", "area");
    assert_node(&conn, "function", "report");
    // `type, extends(shape_base) :: circle` is a derived type.
    assert_node(&conn, "struct", "circle");
    assert_node(&conn, "struct", "shape_base");
    // `real(real64) :: radius` — the component of the derived type.
    assert_node(&conn, "field", "radius");
    // `use iso_fortran_env, only: real64` is an import.
    assert_node(&conn, "import", "iso_fortran_env");
    // `public :: area` is the module's export list.
    assert_node(&conn, "export", "area");

    // A Fortran type extension is an inheritance edge.
    assert_edge(&conn, "extends", "circle", "shape_base");
    // `call report(a)` is a call edge.
    assert_edge(&conn, "calls", "area", "report");
}

#[test]
fn test_extract_haskell_module() {
    let (_temp, conn) = index_fixture("haskell-module");

    assert_node(&conn, "module", "Geometry");
    assert_node(&conn, "function", "area");
    assert_node(&conn, "function", "total");
    // `data Shape = …` is a data type, `class Describable a` a type class.
    assert_node(&conn, "struct", "Shape");
    assert_node(&conn, "interface", "Describable");
    // `import Data.List (sortBy)` is an import.
    assert_node(&conn, "import", "Data.List");

    // `deriving (Describable)` — the data type implements the class.
    assert_edge(&conn, "implements", "Shape", "Describable");
    // `area (head shapes)` — an application is a call edge.
    assert_edge(&conn, "calls", "total", "area");
    // `Data.List.sortBy compare shapes` — a qualified name is the callee
    // of the call, with the module recorded as its qualifier.
    assert_eq!(
        unresolved_qualifier(&conn, "sortBy").as_deref(),
        Some("Data.List"),
        "a qualified Haskell call should record the module it came from"
    );
}

#[test]
fn test_extract_julia_module() {
    let (_temp, conn) = index_fixture("julia-module");

    assert_node(&conn, "module", "Geometry");
    assert_node(&conn, "function", "area");
    assert_node(&conn, "function", "total");
    // `scaled(factor, shape) = factor * area(shape)` — the short form of a
    // function definition.
    assert_node(&conn, "function", "scaled");
    // `abstract type Shape end` is an interface, `struct Circle` a struct.
    assert_node(&conn, "interface", "Shape");
    assert_node(&conn, "struct", "Circle");
    // `using Printf` and `import Base: show` are imports.
    assert_node(&conn, "import", "Printf");
    assert_node(&conn, "import", "Base");
    // `export area, Shape` is the module's export list.
    assert_node(&conn, "export", "area");
    assert_node(&conn, "export", "Shape");

    // `struct Circle <: Shape` — the struct extends the abstract type.
    assert_edge(&conn, "extends", "Circle", "Shape");
    // `sum(area.(shapes))` — a broadcast call is a call edge.
    assert_edge(&conn, "calls", "total", "area");
    // The signature of a function definition is not a call site, so a
    // Julia function does not call itself.
    assert_eq!(
        edge_kind(&conn, "area", "area"),
        None,
        "a Julia function declaration must not produce a self-call edge"
    );
}

#[test]
fn test_extract_lua_module() {
    let (_temp, conn) = index_fixture("lua-module");

    // `function M.area(radius)` — the function's own name is the last
    // segment of the qualified name.
    assert_node(&conn, "function", "area");
    assert_node(&conn, "function", "describe");
    // `local M = {}` — a local binding is a variable.
    assert_node(&conn, "variable", "M");

    // `M.area(radius)` — a qualified call is a call edge.
    assert_edge(&conn, "calls", "describe", "area");
}

#[test]
fn test_extract_matlab_class() {
    let (_temp, conn) = index_fixture("matlab-module");

    assert_node(&conn, "class", "Circle");
    assert_node(&conn, "class", "Shape");
    assert_node(&conn, "function", "area");
    // `properties … Radius … end` — a class property.
    assert_node(&conn, "property", "Radius");
    // `enumeration … ROUND … end` — an enumeration member.
    assert_node(&conn, "enum_member", "ROUND");

    // `classdef Circle < Shape` — the class extends its supertype.
    assert_edge(&conn, "extends", "Circle", "Shape");
}

#[test]
fn test_extract_scala_module() {
    let (_temp, conn) = index_fixture("scala-module");

    // `class Circle(val radius: Double) extends Shape`.
    assert_node(&conn, "class", "Circle");
    // `trait Shape`.
    assert_node(&conn, "interface", "Shape");
    // `object Geometry` — a singleton, like Kotlin's `object`.
    assert_node(&conn, "class", "Geometry");
    // `def area(): Double = …`.
    assert_node(&conn, "function", "area");
    assert_node(&conn, "function", "total");
    // `val MAX = 10`.
    assert_node(&conn, "constant", "MAX");
    // `import scala.collection.mutable` — the whole dotted path is the
    // module the import names.
    assert_node(&conn, "import", "mutable");
    assert_eq!(
        import_module_path(&conn, "mutable").as_deref(),
        Some("scala.collection.mutable"),
        "a Scala import should name the whole dotted path"
    );

    // `class Circle … extends Shape` — the class extends its supertype.
    assert_edge(&conn, "extends", "Circle", "Shape");
}

#[test]
fn test_extract_zig_module() {
    let (_temp, conn) = index_fixture("zig-module");

    // `pub const Circle = struct { … }` — the struct is named by the
    // constant it is bound to.
    assert_node(&conn, "struct", "Circle");
    // `pub fn area(self: Circle) f64 { … }`.
    assert_node(&conn, "function", "area");
    assert_node(&conn, "function", "total");
    // `radius: f64` — a field of the struct.
    assert_node(&conn, "field", "radius");
    // `const std = @import("std");` — a binding.
    assert_node(&conn, "variable", "std");

    // `Circle.area(shape)` — a call edge.
    assert_edge(&conn, "calls", "total", "area");
}

#[test]
fn test_extract_dart_module() {
    let (_temp, conn) = index_fixture("dart-module");

    // `class Circle extends Shape`.
    assert_node(&conn, "class", "Circle");
    assert_node(&conn, "class", "Shape");
    // `double area() => …` — the name is in the signature.
    assert_node(&conn, "function", "area");
    // A top-level function.
    assert_node(&conn, "function", "total");
    // `final double radius;` — a field.
    assert_node(&conn, "field", "radius");
    // `import 'dart:math';`.
    assert_node(&conn, "import", "dart:math");

    // `class Circle extends Shape` — the class extends its supertype.
    assert_edge(&conn, "extends", "Circle", "Shape");
}

#[test]
fn test_extract_groovy_class() {
    let (_temp, conn) = index_fixture("groovy-module");

    // `class Circle extends Shape`.
    assert_node(&conn, "class", "Circle");
    // `interface Shape`.
    assert_node(&conn, "interface", "Shape");
    // `double area() { … }` — a method of the class.
    assert_node(&conn, "function", "area");
    assert_node(&conn, "function", "describe");
    // `double radius` — a field of the class.
    assert_node(&conn, "field", "radius");
    // `import java.util.List` — the last segment names the import.
    assert_node(&conn, "import", "List");
    // `package geometry`.
    assert_node(&conn, "module", "geometry");

    // `class Circle extends Shape` — the class extends its supertype.
    assert_edge(&conn, "extends", "Circle", "Shape");
    // `area()` — a method call.
    assert_edge(&conn, "calls", "describe", "area");
}

#[test]
fn test_extract_powershell_functions() {
    let (_temp, conn) = index_fixture("powershell-module");

    // `function Get-Area { … }` — the name is the `function_name` child.
    assert_node(&conn, "function", "Get-Area");
    assert_node(&conn, "function", "Get-Total");

    // `Get-Area -Radius $shape.Radius` — a command is a call.
    assert_edge(&conn, "calls", "Get-Total", "Get-Area");
}

#[test]
fn test_extract_nix_bindings() {
    let (_temp, conn) = index_fixture("nix-module");

    // `radius = 2.0;` — a binding to a value.
    assert_node(&conn, "variable", "radius");
    // `area = r: 3.14159 * r * r;` — a binding to a function.
    assert_node(&conn, "function", "area");
    // `inherit radius;`.
    assert_node(&conn, "variable", "radius");

    // `value = area radius;` — an application is a call edge. The call
    // is not inside a function, so the file is its source.
    assert_edge(&conn, "calls", "geometry.nix", "area");
}

#[test]
fn test_extract_erlang_module() {
    let (_temp, conn) = index_fixture("erlang-module");

    // `-module(geometry).` — the module is a module attribute.
    assert_node(&conn, "module", "geometry");
    // `-record(circle, {radius = 0.0}).`
    assert_node(&conn, "struct", "circle");
    // `area(#circle{radius = Radius}) -> …`.
    assert_node(&conn, "function", "area");
    assert_node(&conn, "function", "total");
}

#[test]
fn test_extract_elm_module() {
    let (_temp, conn) = index_fixture("elm-module");

    // `module Geometry exposing (area, total)`.
    assert_node(&conn, "module", "Geometry");
    // `area radius = …`.
    assert_node(&conn, "function", "area");
    assert_node(&conn, "function", "total");
    // `import Html exposing (text)`.
    assert_node(&conn, "import", "Html");
}

#[test]
fn test_extract_bash_functions() {
    let (_temp, conn) = index_fixture("bash-module");

    // `area() { … }`.
    assert_node(&conn, "function", "area");
    assert_node(&conn, "function", "total");
    // `RADIUS=2.0`.
    assert_node(&conn, "variable", "RADIUS");
    // `source ./helpers.sh` — a source command is an import.
    assert_node(&conn, "import", "helpers");

    // `area` — a command is a call.
    assert_edge(&conn, "calls", "total", "area");
}

#[test]
fn test_extract_elixir_module() {
    let (_temp, conn) = index_fixture("elixir-module");

    // `defmodule Geometry do` — every Elixir construct is a call whose
    // target names it.
    assert_node(&conn, "module", "Geometry");
    assert_node(&conn, "module", "Circle");
    // `def area(%Circle{radius: radius}) do`.
    assert_node(&conn, "function", "area");
    assert_node(&conn, "function", "total");
    // `defstruct [:radius]`.
    assert_node(&conn, "struct", "Circle");
    // `alias Other.Module, as: Other`.
    assert_node(&conn, "import", "Other.Module");

    // `describe(shapes)` — a call edge.
    assert_edge(&conn, "calls", "total", "describe");
}

#[test]
fn test_extract_toml_tables() {
    let (_temp, conn) = index_fixture("toml-module");

    // `[server]` — a table.
    assert_node(&conn, "struct", "server");
    assert_node(&conn, "struct", "logging");
    // `host = "localhost"` — a key.
    assert_node(&conn, "field", "host");
    assert_node(&conn, "field", "port");
    assert_node(&conn, "field", "level");
}

#[test]
fn test_extract_yaml_mappings() {
    let (_temp, conn) = index_fixture("yaml-module");

    // `server:` — a mapping.
    assert_node(&conn, "struct", "server");
    assert_node(&conn, "struct", "logging");
    // `host: localhost` — a key.
    assert_node(&conn, "field", "host");
    assert_node(&conn, "field", "port");
    assert_node(&conn, "field", "level");
}

#[test]
fn test_extract_r_functions() {
    let (_temp, conn) = index_fixture("r-module");

    // `area <- function(radius) …` — the name of the function is the
    // left-hand side of the assignment it is bound to.
    assert_node(&conn, "function", "area");
    assert_node(&conn, "function", "describe");
    assert_node(&conn, "function", "total");

    // `sprintf("area %.2f", area(radius))` — a call is a call edge.
    assert_edge(&conn, "calls", "describe", "area");
}
