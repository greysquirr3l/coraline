# Security Policy

## Supported Versions

Coraline is currently in active development. Security updates will be provided for the latest version on the `main` branch.

| Version | Supported          |
| ------- | ------------------ |
| main    | :white_check_mark: |
| < 0.1.0 | :x:                |

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

- ✅ **No unsafe code** - `#![forbid(unsafe_code)]` throughout the codebase
- ✅ **Dependency scanning** - Automated security audits via GitHub Actions and `cargo-deny`
- ✅ **Local processing** - All data stays on your machine; no external API calls
- ✅ **Minimal dependencies** - We carefully vet all dependencies
- ✅ **Input validation** - All user inputs are validated before processing

### Disclosure Policy

We follow a **coordinated disclosure** policy:

1. Security vulnerabilities are kept confidential until a fix is available
2. We'll work with you to understand and address the issue
3. Once a fix is released, we'll publish a security advisory
4. We'll credit researchers who report vulnerabilities (unless they prefer to remain anonymous)

## Thank You

We appreciate the security research community's efforts to help keep Coraline and its users safe. Thank you for responsibly disclosing any issues you find.
