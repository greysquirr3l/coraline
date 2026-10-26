#!/usr/bin/env bash
#
# Tests for dependabot-semver.sh.
#
# Run directly:
#   .github/scripts/dependabot-semver-test.sh
#
# The CI workflow runs this on every push and pull request so the
# auto-merge fallback cannot silently rot.

set -uo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
readonly SCRIPT_DIR
readonly CLASSIFIER="$SCRIPT_DIR/dependabot-semver.sh"

if [[ ! -x $CLASSIFIER ]]; then
  printf 'dependabot-semver-test: classifier not found or not executable: %s\n' "$CLASSIFIER" >&2
  exit 1
fi

tests=0
failures=0

assert_level() {
  local name=$1 expected=$2 actual
  actual=$("$CLASSIFIER")
  tests=$((tests + 1))
  if [[ $actual == "$expected" ]]; then
    printf 'ok   %s\n' "$name"
  else
    printf 'FAIL %s\n       expected: %q\n       actual:   %q\n' "$name" "$expected" "$actual"
    failures=$((failures + 1))
  fi
}

# --------------------------------------------------------------------------
# Simple requirement bumps
# --------------------------------------------------------------------------

assert_level "minor bump" "version-update:semver-minor" <<'EOF'
diff --git a/crates/coraline/Cargo.toml b/crates/coraline/Cargo.toml
index 1111111..2222222 100644
--- a/crates/coraline/Cargo.toml
+++ b/crates/coraline/Cargo.toml
@@ -40,7 +40,7 @@
-tree-sitter-erlang = "0.20.0"
+tree-sitter-erlang = "0.21.0"
EOF

assert_level "patch bump" "version-update:semver-patch" <<'EOF'
diff --git a/Cargo.toml b/Cargo.toml
index 1111111..2222222 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -12,7 +12,7 @@
-serde = "1.0.200"
+serde = "1.0.219"
EOF

assert_level "major bump" "version-update:semver-major" <<'EOF'
diff --git a/Cargo.toml b/Cargo.toml
index 1111111..2222222 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -12,7 +12,7 @@
-serde = "1.0.200"
+serde = "2.0.0"
EOF

assert_level "zero-major minor bump" "version-update:semver-minor" <<'EOF'
diff --git a/Cargo.toml b/Cargo.toml
index 1111111..2222222 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -12,7 +12,7 @@
-tree-sitter-erlang = "0.20.0"
+tree-sitter-erlang = "0.21.0"
EOF

assert_level "caret requirement is stripped" "version-update:semver-minor" <<'EOF'
diff --git a/Cargo.toml b/Cargo.toml
index 1111111..2222222 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -12,7 +12,7 @@
-serde = "^1.2.3"
+serde = "^1.3.0"
EOF

# --------------------------------------------------------------------------
# Inline tables
# --------------------------------------------------------------------------

assert_level "inline table version bump" "version-update:semver-patch" <<'EOF'
diff --git a/crates/coraline/Cargo.toml b/crates/coraline/Cargo.toml
index 1111111..2222222 100644
--- a/crates/coraline/Cargo.toml
+++ b/crates/coraline/Cargo.toml
@@ -30,7 +30,7 @@
-ort = { version = "2.0.0-rc.10", default-features = false }
+ort = { version = "2.0.0-rc.11", default-features = false }
EOF

assert_level "inline table with version not first" "version-update:semver-patch" <<'EOF'
diff --git a/Cargo.toml b/Cargo.toml
index 1111111..2222222 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -30,7 +30,7 @@
-sqlite-vec = { default-features = false, version = "0.1.8" }
+sqlite-vec = { default-features = false, version = "0.1.9" }
EOF

assert_level "exact pin prerelease bump" "version-update:semver-patch" <<'EOF'
diff --git a/crates/coraline/Cargo.toml b/crates/coraline/Cargo.toml
index 1111111..2222222 100644
--- a/crates/coraline/Cargo.toml
+++ b/crates/coraline/Cargo.toml
@@ -30,7 +30,7 @@
-ort = { version = "=2.0.0-rc.11", default-features = false }
+ort = { version = "=2.0.0-rc.12", default-features = false }
EOF

assert_level "prerelease to release" "version-update:semver-patch" <<'EOF'
diff --git a/Cargo.toml b/Cargo.toml
index 1111111..2222222 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -30,7 +30,7 @@
-serde = "1.0.0-rc.1"
+serde = "1.0.0"
EOF

# --------------------------------------------------------------------------
# Multiple dependencies -- the highest level wins
# --------------------------------------------------------------------------

assert_level "highest level wins" "version-update:semver-minor" <<'EOF'
diff --git a/crates/coraline/Cargo.toml b/crates/coraline/Cargo.toml
index 1111111..2222222 100644
--- a/crates/coraline/Cargo.toml
+++ b/crates/coraline/Cargo.toml
@@ -40,7 +40,10 @@
-tree-sitter-erlang = "0.20.0"
+tree-sitter-erlang = "0.21.0"
-tree-sitter-haskell = "0.23.1"
+tree-sitter-haskell = "0.24.1"
-tree-sitter-python = "0.24.2"
+tree-sitter-python = "0.25.1"
EOF

assert_level "major beats minor" "version-update:semver-major" <<'EOF'
diff --git a/Cargo.toml b/Cargo.toml
index 1111111..2222222 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -12,7 +12,10 @@
-serde = "1.0.200"
+serde = "1.1.0"
-tokio = "1.0.0"
+tokio = "2.0.0"
EOF

assert_level "multiple manifests are classified together" "version-update:semver-patch" <<'EOF'
diff --git a/Cargo.toml b/Cargo.toml
index 1111111..2222222 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -12,7 +12,7 @@
-serde = "1.0.200"
+serde = "1.0.219"
diff --git a/crates/tree-sitter-blazor/Cargo.toml b/crates/tree-sitter-blazor/Cargo.toml
index 3333333..4444444 100644
--- a/crates/tree-sitter-blazor/Cargo.toml
+++ b/crates/tree-sitter-blazor/Cargo.toml
@@ -9,7 +9,7 @@
-tree-sitter-language = "0.1.0"
+tree-sitter-language = "0.1.1"
EOF

# --------------------------------------------------------------------------
# Changes that must not be classified
# --------------------------------------------------------------------------

assert_level "package version bump is not a dependency bump" "" <<'EOF'
diff --git a/crates/coraline/Cargo.toml b/crates/coraline/Cargo.toml
index 1111111..2222222 100644
--- a/crates/coraline/Cargo.toml
+++ b/crates/coraline/Cargo.toml
@@ -1,6 +1,6 @@
 [package]
 name = "coraline"
-version = "0.14.0"
+version = "0.15.0"
 edition = "2024"
EOF

assert_level "edition bump is not a dependency bump" "" <<'EOF'
diff --git a/Cargo.toml b/Cargo.toml
index 1111111..2222222 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -1,6 +1,6 @@
 [package]
 name = "coraline"
 edition = "2021"
+edition = "2024"
EOF

assert_level "non-cargo file is ignored" "" <<'EOF'
diff --git a/README.md b/README.md
index 1111111..2222222 100644
--- a/README.md
+++ b/README.md
@@ -1,3 +1,3 @@
-serde = "1.0.200"
+serde = "1.0.219"
EOF

assert_level "added dependency is unknown" "" <<'EOF'
diff --git a/Cargo.toml b/Cargo.toml
index 1111111..2222222 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -12,6 +12,7 @@
 [dependencies]
 serde = "1.0.200"
+tracing = "0.1.41"
EOF

assert_level "removed dependency is unknown" "" <<'EOF'
diff --git a/Cargo.toml b/Cargo.toml
index 1111111..2222222 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -12,7 +12,6 @@
 [dependencies]
 serde = "1.0.200"
-tracing = "0.1.41"
EOF

assert_level "version range is unknown" "" <<'EOF'
diff --git a/Cargo.toml b/Cargo.toml
index 1111111..2222222 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -12,7 +12,7 @@
-serde = ">=1.0, <2.0"
+serde = ">=1.1, <3.0"
EOF

assert_level "unclassifiable dependency poisons the result" "" <<'EOF'
diff --git a/Cargo.toml b/Cargo.toml
index 1111111..2222222 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -12,7 +12,10 @@
-serde = "1.0.200"
+serde = "1.0.219"
-zerocopy = ">=0.7, <0.8"
+zerocopy = ">=0.8, <0.9"
EOF

assert_level "empty diff is unknown" "" <<'EOF'
EOF

assert_level "non-diff input is unknown" "" <<'EOF'
this is not a diff at all
EOF

# --------------------------------------------------------------------------
# Downgrades are never classified -- they must not be auto-merged.
# --------------------------------------------------------------------------

assert_level "downgrade is not classified" "" <<'EOF'
diff --git a/Cargo.toml b/Cargo.toml
index 1111111..2222222 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -12,7 +12,7 @@
-serde = "1.1.0"
+serde = "1.0.219"
EOF

assert_level "downgrade poisons a mixed result" "" <<'EOF'
diff --git a/Cargo.toml b/Cargo.toml
index 1111111..2222222 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -12,7 +12,10 @@
-serde = "1.0.200"
+serde = "1.0.219"
-tokio = "1.1.0"
+tokio = "1.0.0"
EOF

assert_level "release to prerelease is not classified" "" <<'EOF'
diff --git a/Cargo.toml b/Cargo.toml
index 1111111..2222222 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -12,7 +12,7 @@
-serde = "1.0.0"
+serde = "1.0.0-rc.1"
EOF

# --------------------------------------------------------------------------

printf '\n%d test(s), %d failure(s)\n' "$tests" "$failures"

if command -v shellcheck >/dev/null 2>&1; then
  printf '\n--- shellcheck ---\n'
  # Every script in the directory, so that a sourced helper is checked
  # alongside its callers.
  shellcheck "${SCRIPT_DIR}"/*.sh || failures=$((failures + 1))
fi

if ((failures > 0)); then
  exit 1
fi
