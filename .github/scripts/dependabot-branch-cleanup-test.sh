#!/usr/bin/env bash
#
# Tests for dependabot-branch-cleanup.sh.
#
# `gh` is stubbed out so the suite runs offline and so that the branch
# list and the open pull request list can be controlled exactly.

set -uo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
cleanup_script="${script_dir}/dependabot-branch-cleanup.sh"

tests=0
failures=0

stub_dir=""
tmp=""
cleanup() {
  [ -n "${stub_dir}" ] && [ -d "${stub_dir}" ] && rm -rf "${stub_dir}"
  [ -n "${tmp}" ] && [ -d "${tmp}" ] && rm -rf "${tmp}"
  return 0
}
trap cleanup EXIT

# stub_gh <branches-file> <open-heads-file> [delete-log-file]
#
# Writes a fake `gh` onto PATH. Every deletion is recorded in the delete
# log so a test can assert exactly which branches were removed.
stub_gh() {
  local branches_file="$1"
  local heads_file="$2"
  local delete_log="${3:-/dev/null}"
  stub_dir="$(mktemp -d)"
  cat >"${stub_dir}/gh" <<STUB
#!/usr/bin/env bash
if [ "\$1" = "api" ] && [ "\$2" = "-X" ] && [ "\$3" = "DELETE" ]; then
  echo "\$*" >> "${delete_log}"
  exit 0
fi
case "\$1 \$2" in
  "repo view")
    echo "greysquirr3l/coraline"
    ;;
  "api repos/greysquirr3l/coraline")
    echo "main"
    ;;
  "api repos/greysquirr3l/coraline/branches")
    cat "${branches_file}"
    ;;
  "pr list")
    cat "${heads_file}"
    ;;
  *)
    echo "unexpected gh invocation: \$*" >&2
    exit 1
    ;;
esac
STUB
  chmod +x "${stub_dir}/gh"
}

run_cleanup() {
  env "PATH=${stub_dir}:${PATH}" \
    "GITHUB_REPOSITORY=greysquirr3l/coraline" \
    "${cleanup_script}" "$@" 2>&1
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

expect_output() {
  local name="$1" expected="$2"
  shift 2
  local out
  out="$(run_cleanup "$@")"
  if [ "$out" != "$expected" ]; then
    fail "$name" "expected=[$expected]" "output=[$out]"
  else
    pass "$name"
  fi
}

tmp="$(mktemp -d)"

# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

cat >"${tmp}/branches.txt" <<'EOF'
main
dependabot/cargo/rust-deps-abc123
dependabot/github_actions/actions/checkout-7.0.1
dependabot/github_actions/github/codeql-action/init-4.38.3
feature/some-work
EOF

# Only one of the three dependabot branches still has an open pull
# request.
cat >"${tmp}/heads.txt" <<'EOF'
dependabot/github_actions/github/codeql-action/init-4.38.3
feature/some-work
EOF

: >"${tmp}/deleted.log"

# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

stub_gh "${tmp}/branches.txt" "${tmp}/heads.txt" "${tmp}/deleted.log"
expect_output "branches without an open pull request are deleted" \
  "delete  dependabot/cargo/rust-deps-abc123
delete  dependabot/github_actions/actions/checkout-7.0.1
skip  dependabot/github_actions/github/codeql-action/init-4.38.3 (an open pull request uses it)
2 branch(es) deleted, 1 skipped" \
  --prefix "dependabot/"

# The two deletions are exactly the two stale branches.
expected_deletes="api -X DELETE repos/greysquirr3l/coraline/git/refs/heads/dependabot/cargo/rust-deps-abc123
api -X DELETE repos/greysquirr3l/coraline/git/refs/heads/dependabot/github_actions/actions/checkout-7.0.1"
if [ "$(cat "${tmp}/deleted.log")" = "${expected_deletes}" ]; then
  pass "only the stale branches are deleted"
else
  fail "only the stale branches are deleted" "got:" "$(cat "${tmp}/deleted.log")"
fi

# A dry run reports without deleting.
: >"${tmp}/deleted.log"
stub_gh "${tmp}/branches.txt" "${tmp}/heads.txt" "${tmp}/deleted.log"
expect_output "a dry run reports without deleting" \
  "would delete  dependabot/cargo/rust-deps-abc123
would delete  dependabot/github_actions/actions/checkout-7.0.1
skip  dependabot/github_actions/github/codeql-action/init-4.38.3 (an open pull request uses it)
2 branch(es) would be deleted, 1 skipped" \
  --dry-run --prefix "dependabot/"

if [ ! -s "${tmp}/deleted.log" ]; then
  pass "a dry run deletes nothing"
else
  fail "a dry run deletes nothing" "got:" "$(cat "${tmp}/deleted.log")"
fi

# The default branch is never deleted, even when the prefix matches it.
# This is the safety net for a prefix that is a prefix of the default
# branch name.
cat >"${tmp}/branches-main.txt" <<'EOF'
main
EOF
: >"${tmp}/heads-main.txt"
: >"${tmp}/deleted.log"
stub_gh "${tmp}/branches-main.txt" "${tmp}/heads-main.txt" "${tmp}/deleted.log"
expect_output "the default branch is never deleted" \
  "skip  main (the default branch)
0 branch(es) deleted, 1 skipped" \
  --prefix "ma"

if [ ! -s "${tmp}/deleted.log" ]; then
  pass "the default branch is not deleted"
else
  fail "the default branch is not deleted" "got:" "$(cat "${tmp}/deleted.log")"
fi

# A branch outside the prefix is not even reported.
cat >"${tmp}/branches-main.txt" <<'EOF'
main
EOF
: >"${tmp}/deleted.log"
stub_gh "${tmp}/branches-main.txt" "${tmp}/heads-main.txt" "${tmp}/deleted.log"
expect_output "a branch outside the prefix is not reported" \
  "0 branch(es) deleted, 0 skipped" \
  --prefix "dependabot/"

# A branch outside the prefix is left alone.
cat >"${tmp}/branches-other.txt" <<'EOF'
main
feature/some-work
EOF
: >"${tmp}/deleted.log"
stub_gh "${tmp}/branches-other.txt" "${tmp}/heads.txt" "${tmp}/deleted.log"
expect_output "branches outside the prefix are left alone" \
  "0 branch(es) deleted, 0 skipped" \
  --prefix "dependabot/"

# A prefix that matches nothing deletes nothing.
: >"${tmp}/deleted.log"
stub_gh "${tmp}/branches-other.txt" "${tmp}/heads.txt" "${tmp}/deleted.log"
expect_output "a prefix that matches nothing deletes nothing" \
  "0 branch(es) deleted, 0 skipped" \
  --prefix "renovate/"

# A branch with no open pull request under a different prefix is deleted
# when that prefix is given.
cat >"${tmp}/branches-renovate.txt" <<'EOF'
main
renovate/renovate-abc
EOF
: >"${tmp}/heads-renovate.txt"
: >"${tmp}/deleted.log"
stub_gh "${tmp}/branches-renovate.txt" "${tmp}/heads-renovate.txt" "${tmp}/deleted.log"
expect_output "the prefix is configurable" \
  "delete  renovate/renovate-abc
1 branch(es) deleted, 0 skipped" \
  --prefix "renovate/"

echo
echo "${tests} test(s), ${failures} failure(s)"
[ "${failures}" -eq 0 ]
