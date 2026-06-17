#!/usr/bin/env bash
# Install git hooks for coraline development

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GIT_DIR="$(git rev-parse --git-dir)"

echo "Installing git hooks for coraline..."

# Install pre-commit hook
cp "$SCRIPT_DIR/pre-commit" "$GIT_DIR/hooks/pre-commit"
chmod +x "$GIT_DIR/hooks/pre-commit"
echo "  ✓ pre-commit"

# Install commit-msg hook
cp "$SCRIPT_DIR/commit-msg" "$GIT_DIR/hooks/commit-msg"
chmod +x "$GIT_DIR/hooks/commit-msg"
echo "  ✓ commit-msg"

# Install pre-push hook
cp "$SCRIPT_DIR/pre-push" "$GIT_DIR/hooks/pre-push"
chmod +x "$GIT_DIR/hooks/pre-push"
echo "  ✓ pre-push"

echo ""
echo "Git hooks installed successfully!"
echo ""
echo "pre-commit (runs on every commit):"
echo "  • Format code with cargo fmt"
echo "  • Detect secrets with gitleaks"
echo "  • Check vulnerabilities with cargo audit"
echo "  • Verify licenses/dependencies with cargo deny"
echo ""
echo "commit-msg (validates commit messages):"
echo "  • Enforce conventional commits format (feat:, fix:, etc.)"
echo "  • Check subject line length (max 72 chars)"
echo "  • Ensure proper formatting (blank line between subject/body)"
echo ""
echo "pre-push (runs before push to remote):"
echo "  • Run all tests with cargo test --all-features"
echo "  • Run lints with cargo clippy --all-features"
echo "  • Verify release build"
echo "  • Build documentation"
echo "  • Build mdbook (if present)"
echo ""
echo "To bypass hooks (not recommended):"
echo "  git commit --no-verify"
echo "  git push --no-verify"
echo ""
echo "Optional dependencies for full functionality:"
echo "  brew install gitleaks              # Secret detection"
echo "  cargo install cargo-audit          # Security auditing"
echo "  cargo install cargo-deny           # License/dependency checks"
echo "  cargo install mdbook               # Documentation building"
