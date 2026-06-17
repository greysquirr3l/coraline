# Git Hooks for Coraline Development

This directory contains git hooks to maintain code quality and enforce development standards.

## Installation

```bash
./scripts/install-hooks.sh
```

This will install three git hooks:
- `pre-commit` - Runs on every commit
- `commit-msg` - Validates commit messages
- `pre-push` - Runs before pushing to remote

## Hooks

### pre-commit

**Runs automatically before each commit**

Checks performed:
1. **Code formatting** - Runs `cargo fmt --all` and auto-stages changes
2. **Secret detection** - Scans for leaked secrets/credentials with `gitleaks`
3. **Security audit** - Checks for known vulnerabilities with `cargo audit`
4. **License/dependency check** - Validates licenses and dependencies with `cargo deny` (if deny.toml exists)

**Optional dependencies:**
```bash
brew install gitleaks              # macOS
cargo install cargo-audit
cargo install cargo-deny
```

### commit-msg

**Runs automatically when writing commit messages**

Enforces [Conventional Commits](https://www.conventionalcommits.org/) format:

```
<type>[(scope)][!]: <description>

[optional body]

[optional footer]
```

**Valid types:**
- `feat` - New feature
- `fix` - Bug fix
- `docs` - Documentation changes
- `style` - Code style changes (formatting, etc.)
- `refactor` - Code refactoring
- `test` - Test changes
- `chore` - Maintenance tasks
- `ci` - CI/CD changes
- `perf` - Performance improvements
- `build` - Build system changes

**Examples:**
```bash
feat: add timeout configuration to MCP server
fix(mcp): resolve dead code warning for timeout_ms
docs: update README with OpenCode integration
feat(tools)!: add batch query tools for token savings
```

**Also checks:**
- Subject line length (warns if > 72 characters)
- Blank line between subject and body (required if body exists)

### pre-push

**Runs automatically before pushing to remote**

Comprehensive quality checks:
1. **Tests** - Runs `cargo test --all-features`
2. **Linting** - Runs `cargo clippy --all-targets --all-features -- -D warnings`
3. **Release build** - Ensures `cargo build --release --all-features` succeeds
4. **Documentation** - Builds Rust docs with `cargo doc --no-deps --all-features`
5. **mdbook** - Builds mdbook if `book/book.toml` exists

**Note:** All cargo commands use `--all-features` to include embeddings support.

## Bypassing Hooks

**Not recommended**, but you can bypass hooks in emergencies:

```bash
git commit --no-verify    # Skip pre-commit and commit-msg
git push --no-verify      # Skip pre-push
```

## Uninstalling

To remove the hooks:

```bash
rm .git/hooks/pre-commit
rm .git/hooks/commit-msg
rm .git/hooks/pre-push
```

## Development Workflow

With hooks installed, your typical workflow becomes:

```bash
# Make changes
vim src/some_file.rs

# Stage changes
git add src/some_file.rs

# Commit (pre-commit runs automatically)
# - Formats code
# - Checks for secrets
# - Audits dependencies
git commit -m "feat: add new feature"

# Push (pre-push runs automatically)
# - Runs tests
# - Runs clippy
# - Builds release
# - Builds docs
git push
```

All quality checks happen automatically - no need to remember to run them manually!

## Troubleshooting

**Hook fails with "command not found":**
- Install the optional dependency (gitleaks, cargo-audit, cargo-deny, mdbook)
- Or the hook will skip that check and continue

**Commit message validation fails:**
- Ensure your message follows conventional commits format
- Use one of the valid types: feat, fix, docs, etc.
- Include a colon and space after the type: `feat: description`

**Pre-push takes too long:**
- Consider using `git push --no-verify` for WIP branches
- Re-enable verification before merging to main

**Formatting conflicts:**
- The pre-commit hook auto-formats and stages changes
- If you see unexpected formatting, check your editor settings
- Run `cargo fmt --all` manually to see what will change
