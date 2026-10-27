# Security Policy

## Supported Versions

Security updates are provided for the latest released version. The
project is in active development, so upgrading promptly is recommended.

| Version | Supported          |
| ------- | ------------------ |
| 0.15.x  | :white_check_mark: |
| 0.14.x  | :white_check_mark: |
| < 0.14  | :x:                |
`main` carries unreleased work and is not a supported target — use the
latest release from [crates.io](https://crates.io/crates/coraline) or
[GitHub Releases](https://github.com/greysquirr3l/coraline/releases).

## Reporting a Vulnerability

We take the security of Coraline seriously. If you discover a security vulnerability, please report it responsibly.

### How to Report

**Please DO NOT open a public GitHub issue for security vulnerabilities.**

Instead, report security issues via email to:

**<s0ma@protonmail.com>**

### What to Include

When reporting a vulnerability, please include:

- **Description** - A clear description of the vulnerability
- **Impact** - What an attacker could achieve by exploiting this vulnerability
- **Steps to Reproduce** - Detailed steps to reproduce the issue
- **Affected Versions** - Which versions of Coraline are affected
- **Mitigations** - Any workarounds or mitigations you've identified (if applicable)
- **Proof of Concept** - If possible, include a minimal proof of concept

### What to Expect

- **Acknowledgment** - We'll acknowledge receipt of your report within 48 hours
- **Updates** - We'll keep you informed about our progress investigating and addressing the issue
- **Credit** - If you'd like, we'll credit you for the discovery in our security advisory and changelog
- **Timeline** - We aim to address critical vulnerabilities within 7 days, and other vulnerabilities within 30 days

### Security Best Practices

When using Coraline, we recommend:

1. **Keep Coraline up to date** - Always use the latest version for security patches
2. **Review MCP permissions** - Understand what permissions Coraline requests when used as an MCP server
3. **Protect your database** - Ensure your `.coraline/` directory has appropriate file permissions
4. **Sanitize inputs** - When integrating Coraline into custom applications, validate and sanitize all inputs
5. **Monitor dependencies** - We use Dependabot and security scanning; you should too for projects that depend on Coraline

### Security Features

Coraline is built with security in mind:

- ✅ **No unsafe code in the default build** - The workspace sets
  `unsafe_code = "deny"` and every crate opts in via
  `[lints] workspace = true`, so `unsafe` is a compile error in the
  library, the binary and the grammar crate alike. Two deliberate
  exceptions exist, both narrow and both documented at the call site:
  - `src/vec_ext.rs` registers the `sqlite-vec` extension through
    `sqlite3_auto_extension`, which is `unsafe`. The module is
    feature-gated behind `vec-ext` and the allowance is scoped to that
    one FFI registration.
  - `crates/tree-sitter-blazor/bindings/rust/lib.rs` calls
    `LanguageFn::from_raw`, the standard tree-sitter binding pattern.
- ✅ **Dependency scanning** - Automated security audits via GitHub Actions and `cargo-deny`
- ✅ **Fuzz testing** - `cargo-fuzz` libFuzzer targets for the MCP JSON-RPC surface and the SQLite/FTS5 query layer, exercised nightly and on every PR via ClusterFuzzLite
- ✅ **Code scanning** - CodeQL analysis (weekly) and `cargo-audit` dependency checks (daily)
- ✅ **Local processing** - Your source code is never uploaded. The one
  outbound request Coraline makes is downloading the embedding model
  from HuggingFace, and only when you ask for it (`coraline model
  download`, or the prompt during `coraline init`). The update checker
  in `src/update.rs` queries the crates.io API for the latest version.
- ✅ **Dependency review** - Every direct dependency is vetted by hand;
  `cargo deny` enforces the licence, advisory and duplicate-version
  policy, and `cargo audit` runs daily
- ⚠️ **Input validation** - Arguments are parsed and validated at the
  CLI and MCP boundaries. Note that three file tools accept paths outside
  the project root: `coraline_read_file`, `coraline_list_dir` and
  `coraline_get_file_nodes` resolve absolute paths as given, and a
  relative path may contain `..`. The MCP server runs with the
  privileges of the user who started it, so it can read any file that
  user can. Run it against a project you are willing to expose to the
  assistant.

### Disclosure Policy

We follow a **coordinated disclosure** policy:

1. Security vulnerabilities are kept confidential until a fix is available
2. We'll work with you to understand and address the issue
3. Once a fix is released, we'll publish a security advisory
4. We'll credit researchers who report vulnerabilities (unless they prefer to remain anonymous)

## Thank You

We appreciate the security research community's efforts to help keep Coraline and its users safe. Thank you for responsibly disclosing any issues you find.
