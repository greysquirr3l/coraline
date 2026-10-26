#!/usr/bin/env bash
#
# Tests for dependabot-level.sh.
#
# `gh` is stubbed out so the suite runs offline and so that the commit
# message of each pull request can be controlled exactly.

set -uo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
level_script="${script_dir}/dependabot-level.sh"

tests=0
failures=0

stub_dir=""
cleanup() {
  if [ -n "${stub_dir}" ] && [ -d "${stub_dir}" ]; then
    rm -rf "${stub_dir}"
  fi
}
trap cleanup EXIT

# stub_gh <commit-message-file> [diff-file]
#
# Writes a fake `gh` onto PATH that answers the four calls the script
# makes: `gh pr view --json number`, `gh api .../pulls/N` (head sha),
# `gh api .../commits/S` (commit message) and `gh pr diff`.
stub_gh() {
  local message_file="$1"
  local diff_file="${2:-/dev/null}"
  stub_dir="$(mktemp -d)"
  cat >"${stub_dir}/gh" <<STUB
#!/usr/bin/env bash
case "\$1 \$2" in
  "pr view")
    echo "42"
    ;;
  "api repos/"*"/pulls/"*)
    echo "abc123"
    ;;
  "api repos/"*"/commits/"*)
    cat "${message_file}"
    ;;
  "pr diff")
    cat "${diff_file}"
    ;;
  *)
    echo "unexpected gh invocation: \$*" >&2
    exit 1
    ;;
esac
STUB
  chmod +x "${stub_dir}/gh"
}

run_level() {
  env "PATH=${stub_dir}:${PATH}" \
    "GITHUB_REPOSITORY=greysquirr3l/coraline" \
    "${level_script}" \
    "https://github.com/greysquirr3l/coraline/pull/42" 2>&1
}

pass() {
  tests=$((tests + 1))
  echo "ok   $1"
}

fail() {
  tests=$((tests + 1))
  failures=$((failures + 1))
  echo "FAIL $1"
  if [ $# -gt 1 ]; then
    shift
    printf '     %s\n' "$@"
  fi
}

expect_level() {
  local name="$1" expected="$2"
  local out status
  out="$(run_level)"
  status=$?
  if [ "$status" -ne 0 ]; then
    fail "$name" "exit=$status" "output=$out"
  elif [ "$out" != "$expected" ]; then
    fail "$name" "expected=[$expected]" "output=[$out]"
  else
    pass "$name"
  fi
}

# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}" "${stub_dir}"' EXIT

# A GitHub Actions update, as Dependabot writes it: the level is in the
# metadata block.
cat >"${tmp}/actions-explicit.yaml" <<'EOF'
ci: bump the codeql group with 4 updates

Updates `github/codeql-action/init` from 4.36.2 to 4.39.1
Updates `github/codeql-action/upload-sarif` from 4.37.7 to 4.39.1

updated-dependencies:
- dependency-name: github/codeql-action/init
  dependency-version: 4.39.1
  dependency-type: direct:production
  update-type: version-update:semver-minor
  dependency-group: codeql
- dependency-name: github/codeql-action/upload-sarif
  dependency-version: 4.39.1
  dependency-type: direct:production
  update-type: version-update:semver-minor
  dependency-group: codeql
EOF

# The same update without the metadata block: the level has to come from
# the "from ... to ..." line.
cat >"${tmp}/actions-pairs.txt" <<'EOF'
ci: bump actions/checkout from 4.2.2 to 5.0.0

Updates `actions/checkout` from 4.2.2 to 5.0.0
EOF

# A Cargo requirement update: no "from" clause and no update-type field,
# so the level has to come from the diff.
cat >"${tmp}/cargo.txt" <<'EOF'
chore(deps): update the rust-deps group

Updates `tree-sitter-erlang` to 0.21.0
Updates `tree-sitter-haskell` to 0.24.1

updated-dependencies:
- dependency-name: tree-sitter-erlang
  dependency-version: 0.21.0
  dependency-type: direct:production
EOF

cat >"${tmp}/cargo.diff" <<'EOF'
diff --git a/crates/coraline/Cargo.toml b/crates/coraline/Cargo.toml
index 1111111..2222222 100644
--- a/crates/coraline/Cargo.toml
+++ b/crates/coraline/Cargo.toml
@@ -12,7 +12,7 @@
-tree-sitter-erlang = "0.20.0"
+tree-sitter-erlang = "0.21.0"
EOF

# A single (not grouped) dependency update phrases the bump differently:
# there is no "Updates `x` from A to B" line, only "Bumps [x](url) from
# A to B.".
cat >"${tmp}/single-bumps.txt" <<'EOF'
ci: bump actions/download-artifact from 8.0.1 to 8.0.2

Bumps [actions/download-artifact](https://github.com/actions/download-artifact) from 8.0.1 to 8.0.2.
- [Release notes](https://github.com/actions/download-artifact/releases)

---
updated-dependencies:
- dependency-name: actions/download-artifact
  dependency-version: 8.0.2
  dependency-type: direct:production
  update-type: version-update:semver-patch
...
EOF

# The same message without the metadata block, so the level has to come
# from the "Bumps ..." prose line.
cat >"${tmp}/single-bumps-no-field.txt" <<'EOF'
ci: bump actions/upload-artifact from 7.0.1 to 7.0.2

Bumps [actions/upload-artifact](https://github.com/actions/upload-artifact) from 7.0.1 to 7.0.2.
- [Release notes](https://github.com/actions/upload-artifact/releases)
EOF

# A message that carries a pair that cannot be compared.
cat >"${tmp}/downgrade.txt" <<'EOF'
Updates `serde` from 1.0.200 to 1.0.100
EOF

# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

stub_gh "${tmp}/actions-explicit.yaml"
expect_level "explicit update-type field is used" "version-update:semver-minor"

stub_gh "${tmp}/actions-pairs.txt"
expect_level "version pair is classified" "version-update:semver-major"

stub_gh "${tmp}/cargo.txt" "${tmp}/cargo.diff"
expect_level "cargo update falls back to the diff" "version-update:semver-minor"

stub_gh "${tmp}/single-bumps.txt"
expect_level "single update uses its update-type field" "version-update:semver-patch"

stub_gh "${tmp}/single-bumps-no-field.txt"
expect_level "single update falls back to the bumps prose" "version-update:semver-patch"

stub_gh "${tmp}/cargo.txt"
expect_level "cargo update without a diff is not classified" ""

stub_gh "${tmp}/downgrade.txt"
expect_level "a downgrade is not classified" ""

# An empty message and an empty diff must not be classified.
: >"${tmp}/empty.txt"
stub_gh "${tmp}/empty.txt"
expect_level "an empty message is not classified" ""

# A message that mentions the dependency but not in the recognised form.
cat >"${tmp}/noise.txt" <<'EOF'
chore(deps): bump the rust-deps group

Updates the requirements on tree-sitter-erlang, tree-sitter-haskell and
tree-sitter-php to permit the latest version.
EOF
stub_gh "${tmp}/noise.txt"
expect_level "a message without versions is not classified" ""

echo
echo "${tests} test(s), ${failures} failure(s)"
[ "${failures}" -eq 0 ]
