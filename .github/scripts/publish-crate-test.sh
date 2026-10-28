#!/usr/bin/env bash
#
# Tests for publish-crate.sh.
#
# `cargo` is stubbed so the suite runs offline and so each outcome can
# be forced: a successful publish, a publish that fails because the
# version is already on the index, and a publish that fails for any
# other reason.
#
# The third case is the one that matters. Without `set -o pipefail` in
# the script, the pipeline's exit status is tee's -- always 0 -- so the
# success branch is taken even when cargo fails, and the script exits 0
# having published nothing. A release built on that would look green
# and ship nothing. This test fails if that regression comes back.

set -uo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
publish_script="${script_dir}/publish-crate.sh"

tests=0
failures=0

stub_dir=""
cleanup() {
  if [ -n "${stub_dir}" ] && [ -d "${stub_dir}" ]; then
    rm -rf "${stub_dir}"
  fi
}
trap cleanup EXIT

# The PATH the suite starts with, restored before each test so that one
# test's stubs cannot satisfy another test's calls.
ORIGINAL_PATH="${PATH}"

# stub_cargo <exit-code> [message]
#
# Writes a fake `cargo` onto PATH. It answers `cargo metadata` with a
# fixed version and makes `cargo publish` exit with the given code and
# print the given message.
stub_cargo() {
  local exit_code="$1"
  local message="${2:-}"

  cat > "${stub_dir}/cargo" <<EOF
#!/usr/bin/env bash
if [ "\${1:-}" = "metadata" ]; then
  echo '{"packages":[{"name":"coraline","version":"9.9.9"}]}'
  exit 0
fi
if [ -n "${message}" ]; then
  echo "${message}" >&2
fi
exit ${exit_code}
EOF
  chmod +x "${stub_dir}/cargo"
}

# stub_jq writes a fake `jq` that always prints a version string.
#
# It has to drain stdin before printing. Real `jq` reads the whole
# stream; a stub that printed and exited immediately would send SIGPIPE
# to `cargo metadata` upstream, and with `pipefail` in the script that
# SIGPIPE (141) would fail the script before it ever reached the
# publish step.
stub_jq() {
  cat > "${stub_dir}/jq" <<'EOF'
#!/usr/bin/env bash
cat > /dev/null
echo "9.9.9"
EOF
  chmod +x "${stub_dir}/jq"
}

setup() {
  cleanup
  stub_dir="$(mktemp -d)"
  stub_jq
  # Prepend rather than append, and reset the saved original PATH each
  # time. Appending would let an earlier test's stub win, because PATH
  # is searched left to right and the first `cargo` found is the one
  # that runs.
  PATH="${stub_dir}:${ORIGINAL_PATH}"
}

check() {
  local name="$1"
  local expected="$2"
  local actual="$3"

  tests=$((tests + 1))
  if [ "$expected" = "$actual" ]; then
    echo "ok ${tests} - ${name}"
  else
    failures=$((failures + 1))
    echo "not ok ${tests} - ${name}"
    echo "#   expected: ${expected}"
    echo "#   actual:   ${actual}"
  fi
}

# A publish that succeeds reports PUBLISHED.
test_publish_succeeds() {
  setup
  stub_cargo 0 ""

  local output status
  output="$("${publish_script}" coraline "${stub_dir}/out.log" 2>&1)"
  status=$?

  check "a successful publish exits 0" "0" "${status}"
  check "a successful publish reports PUBLISHED" "PUBLISHED coraline@9.9.9" "${output}"
}

# A publish that fails because the version is already on the index is a
# success, but it is a *different* success: the caller must know not to
# wait for the index to catch up.
test_publish_already_exists() {
  setup
  stub_cargo 1 "error: crate coraline@9.9.9 already exists on crates.io index"

  local output status
  output="$("${publish_script}" coraline "${stub_dir}/out.log" 2>&1)"
  status=$?

  check "an already-published crate exits 0" "0" "${status}"
  # The script's own verdict is the last line; cargo's message is
  # echoed above it because the stub writes to stderr.
  check "an already-published crate reports ALREADY_PUBLISHED" \
    "ALREADY_PUBLISHED coraline@9.9.9" "$(echo "${output}" | tail -1)"
}

# A publish that fails for any other reason must fail loudly. This is
# the regression test: without pipefail this case reports success.
test_publish_real_failure() {
  setup
  stub_cargo 1 "error: failed to publish to registry at https://crates.io"

  local output status
  output="$("${publish_script}" coraline "${stub_dir}/out.log" 2>&1)"
  status=$?

  check "a genuine publish failure exits 1" "1" "${status}"

  local reported="no"
  case "${output}" in
    *"failed to publish to registry"*) reported="yes" ;;
  esac
  check "a genuine publish failure is reported" "yes" "${reported}"
}

# The failure log is preserved so it can be inspected after the fact.
test_failure_log_is_written() {
  setup
  stub_cargo 1 "error: token authentication failed"

  "${publish_script}" coraline "${stub_dir}/out.log" > /dev/null 2>&1 || true

  local logged="no"
  case "$(cat "${stub_dir}/out.log" 2>/dev/null)" in
    *"token authentication failed"*) logged="yes" ;;
  esac
  check "the failure log is written" "yes" "${logged}"
}

# stdout must carry the verdict and nothing else.
#
# The release workflow captures stdout and branches on it with a `case`
# statement. When cargo's own output shared stdout with the verdict, the
# `case` fell through to its catch-all branch and the release failed
# with "unexpected publish outcome" -- even though the script had
# correctly reported ALREADY_PUBLISHED. cargo's output goes to stderr
# for exactly this reason.
test_stdout_is_only_the_verdict() {
  setup
  stub_cargo 1 "error: crate coraline@9.9.9 already exists on crates.io index"

  local stdout only
  stdout="$("${publish_script}" coraline "${stub_dir}/out.log" 2>/dev/null)"
  only="$(echo "${stdout}" | wc -l | tr -d ' ')"

  check "stdout carries exactly one line" "1" "${only}"
  check "stdout is the verdict" "ALREADY_PUBLISHED coraline@9.9.9" "${stdout}"
}

# cargo's output must still reach stderr, so it is not lost from the
# CI log.
test_cargo_output_goes_to_stderr() {
  setup
  stub_cargo 1 "error: crate coraline@9.9.9 already exists on crates.io index"

  local stderr
  stderr="$("${publish_script}" coraline "${stub_dir}/out.log" 2>&1 >/dev/null)"

  local seen="no"
  case "${stderr}" in
    *"already exists on crates.io index"*) seen="yes" ;;
  esac
  check "cargo's output reaches stderr" "yes" "${seen}"
}

echo "1..$((tests + 6))"
test_publish_succeeds
test_publish_already_exists
test_publish_real_failure
test_failure_log_is_written
test_stdout_is_only_the_verdict
test_cargo_output_goes_to_stderr

if [ "${failures}" -gt 0 ]; then
  echo "# ${failures} of ${tests} tests failed"
  exit 1
fi

echo "# all ${tests} tests passed"
exit 0
