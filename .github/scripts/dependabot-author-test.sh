#!/usr/bin/env bash
#
# Tests for dependabot-author.sh.
#
# `gh` is stubbed out so the suite runs offline, and so the GitHub App
# slug (`app/dependabot`) can be exercised next to the bot login
# (`dependabot[bot]`).

set -uo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
author_script="${script_dir}/dependabot-author.sh"

tests=0
failures=0

stub_dir=""
cleanup() {
  if [ -n "${stub_dir}" ] && [ -d "${stub_dir}" ]; then
    rm -rf "${stub_dir}"
  fi
}
trap cleanup EXIT

# stub_gh <author-login> [api-exit-code]
#
# Writes a fake `gh` onto PATH that answers the two calls the script
# makes: `gh pr view ... --json number` and `gh api repos/.../pulls/N`.
stub_gh() {
  local author="$1"
  local api_exit="${2:-0}"
  stub_dir="$(mktemp -d)"
  cat >"${stub_dir}/gh" <<STUB
#!/usr/bin/env bash
printf '%s\n' "\$*" >>"${stub_dir}/calls.log"
case "\$1 \$2" in
  "pr view")
    echo "42"
    ;;
  "api repos/"*)
    if [ "${api_exit}" -ne 0 ]; then
      echo "gh: API error" >&2
      exit "${api_exit}"
    fi
    echo "${author}"
    ;;
  *)
    echo "unexpected gh invocation: \$*" >&2
    exit 1
    ;;
esac
STUB
  chmod +x "${stub_dir}/gh"
}

run_author() {
  env "PATH=${stub_dir}:${PATH}" \
    "GITHUB_REPOSITORY=greysquirr3l/coraline" \
    "${author_script}" "$@"
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

# --- dependabot pull request is accepted -----------------------------------
stub_gh "dependabot[bot]"
out="$(run_author "https://github.com/greysquirr3l/coraline/pull/1" 2>&1)"
status=$?
if [ "$status" -eq 0 ] && [ "$out" = "dependabot[bot]" ]; then
  pass "dependabot pull request is accepted"
else
  fail "dependabot pull request is accepted" "exit=$status" "output=$out"
fi

# --- the REST API is used, not `gh pr view --json author` ------------------
if grep -q "api repos/greysquirr3l/coraline/pulls/42" "${stub_dir}/calls.log"; then
  pass "reads the author from the REST pull request endpoint"
else
  fail "reads the author from the REST pull request endpoint" \
    "calls: $(tr '\n' '|' <"${stub_dir}/calls.log")"
fi

if grep -q "author" "${stub_dir}/calls.log"; then
  fail "does not read the author from \`gh pr view --json author\`" \
    "that field reports the app slug, not the bot login"
else
  pass "does not read the author from \`gh pr view --json author\`"
fi

# --- human-authored pull request is rejected --------------------------------
stub_gh "greysquirr3l"
out="$(run_author "https://github.com/greysquirr3l/coraline/pull/1" 2>&1)"
status=$?
if [ "$status" -eq 1 ]; then
  pass "human-authored pull request is rejected"
else
  fail "human-authored pull request is rejected" "exit=$status" "output=$out"
fi

# --- the GitHub App slug is rejected ---------------------------------------
# `gh pr view --json author` reports `app/dependabot`; accepting it would
# mean the check silently stopped matching the webhook payload.
stub_gh "app/dependabot"
out="$(run_author "https://github.com/greysquirr3l/coraline/pull/1" 2>&1)"
status=$?
if [ "$status" -eq 1 ]; then
  pass "github app slug is rejected"
else
  fail "github app slug is rejected" "exit=$status" "output=$out"
fi

# --- a missing argument is a usage error ------------------------------------
out="$(run_author 2>&1)"
status=$?
if [ "$status" -eq 2 ]; then
  pass "missing argument is a usage error"
else
  fail "missing argument is a usage error" "exit=$status" "output=$out"
fi

# --- a failed API read is reported as a lookup failure ---------------------
stub_gh "dependabot[bot]" 1
out="$(run_author "https://github.com/greysquirr3l/coraline/pull/1" 2>&1)"
status=$?
if [ "$status" -eq 3 ]; then
  pass "failed API read is a lookup failure"
else
  fail "failed API read is a lookup failure" "exit=$status" "output=$out"
fi

# --- the bot login is configurable ------------------------------------------
stub_gh "renovate[bot]"
out="$(env "PATH=${stub_dir}:${PATH}" \
  "GITHUB_REPOSITORY=greysquirr3l/coraline" \
  "DEPENDABOT_LOGIN=renovate[bot]" \
  "${author_script}" \
  "https://github.com/greysquirr3l/coraline/pull/1" 2>&1)"
status=$?
if [ "$status" -eq 0 ] && [ "$out" = "renovate[bot]" ]; then
  pass "the expected bot login is configurable"
else
  fail "the expected bot login is configurable" "exit=$status" "output=$out"
fi

echo
echo "${tests} test(s), ${failures} failure(s)"
[ "${failures}" -eq 0 ]
