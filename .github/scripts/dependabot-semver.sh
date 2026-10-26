#!/usr/bin/env bash
#
# Classify a dependency update as a semver major/minor/patch bump.
#
# Why this exists
# ---------------
# `dependabot/fetch-metadata` derives the update type from the
# "Updates `x` from A to B" / "Bumps x from A to B." line in the
# Dependabot commit message. Cargo *requirement* updates have no "from"
# clause:
#
#     Updates `tree-sitter-erlang` to 0.21.0
#
# because this repository does not commit `Cargo.lock`, so the updater
# rewrites the version requirement in `Cargo.toml` instead of a resolved
# version in the lockfile. With no previous version, `fetch-metadata`
# resolves `update-type` to `null`, and the auto-merge workflow's
# `semver-patch|semver-minor` condition never matches -- so Cargo
# dependency updates are never auto-merged.
#
# This script reconstructs the version pair from the unified diff of the
# Cargo manifests and prints the highest bump level it finds:
#
#     version-update:semver-major
#     version-update:semver-minor
#     version-update:semver-patch
#
# It prints nothing when the level cannot be established. Callers must
# treat an empty result as "do not auto-merge": the script is a
# conservative fallback, never a licence to merge something it does not
# fully understand.
#
# Usage:
#   git diff ... | .github/scripts/dependabot-semver.sh
#   .github/scripts/dependabot-semver.sh < patch.diff
#
# Exit status is 0 whenever the level could be determined (or could not
# be determined and was reported as "unknown"); only usage errors exit
# non-zero.

set -euo pipefail

readonly PROGRAM_NAME=${0##*/}

usage() {
  cat <<'EOF'
Usage: dependabot-semver.sh [< diff]

Reads a unified diff on stdin and prints the highest dependency bump
level it contains:

  version-update:semver-major
  version-update:semver-minor
  version-update:semver-patch

Nothing is printed when no Cargo dependency change can be classified.
EOF
}

die() {
  printf '%s: %s\n' "$PROGRAM_NAME" "$*" >&2
  exit 1
}

case "${1:-}" in
  -h | --help)
    usage
    exit 0
    ;;
  "") ;;
  *) die "unexpected argument: $1" ;;
esac

# ---------------------------------------------------------------------------
# Diff extraction
# ---------------------------------------------------------------------------
#
# Emits one `old<TAB>new` line per Cargo dependency whose version
# requirement changed, and the sentinel `__unknown__` when a dependency
# changed in a way that cannot be classified (a version range, or a
# dependency that was added or removed outright).
# The awk program is intentionally single-quoted: it must not be expanded
# by the shell.
# shellcheck disable=SC2016
readonly EXTRACT_AWK='
BEGIN {
  manifest_count = split("name version edition rust-version license license-file description homepage documentation repository readme keywords categories authors maintainers contributors include exclude publish build links default-run workspace lints profile badges default-members resolver members metadata default-features features optional package path git branch rev tag", manifest_keys, " ")
  for (i = 1; i <= manifest_count; i++) {
    manifest_key[manifest_keys[i]] = 1
  }
  is_cargo = 0
  dep_count = 0
}

/^\+\+\+ / {
  path = substr($0, 5)
  is_cargo = (path ~ /(^|\/)Cargo\.toml$/)
  next
}

/^diff --git / {
  is_cargo = 0
  next
}

/^--- / { next }

{
  marker = substr($0, 1, 1)
  if (marker != "-" && marker != "+") next
  if (!is_cargo) next
  parse_dependency(substr($0, 2))
}

function parse_dependency(body,   eq, name, rest, version, after, quote_pos, key) {
  dep_name = ""
  dep_version = ""
  dep_state = "skip"

  sub(/[[:space:]]*#.*$/, "", body)
  if (body !~ /^[A-Za-z0-9_-]+[[:space:]]*=/) return

  eq = index(body, "=")
  if (eq == 0) return
  name = substr(body, 1, eq - 1)
  gsub(/[[:space:]]/, "", name)
  if (name == "") return
  if (name in manifest_key) return

  rest = substr(body, eq + 1)
  gsub(/^[[:space:]]+/, "", rest)
  gsub(/[[:space:]]+$/, "", rest)

  version = ""
  if (rest ~ /^"[^"]*"$/) {
    version = rest
    sub(/^"/, "", version)
    sub(/"$/, "", version)
  } else if (rest ~ /^\{/) {
    if (!match(rest, /version[[:space:]]*=[[:space:]]*"/)) return
    after = substr(rest, RSTART + RLENGTH)
    quote_pos = index(after, "\"")
    if (quote_pos == 0) return
    version = substr(after, 1, quote_pos - 1)
  } else {
    return
  }

  # Strip the requirement operator (`^`, `~`, `=`, ...) so that the
  # remaining text is the bare version.
  sub(/^[ \t]*[\^~=><!]+[ \t]*/, "", version)

  key = path "\t" name

  # A range such as ">=1.2, <2.0" cannot be compared as a single version.
  if (version ~ /,/) {
    dep_name = name
    dep_state = "unknown"
    record(key)
    return
  }

  if (version !~ /^[0-9]/) return

  dep_name = name
  dep_version = version
  dep_state = "dep"
  record(key)
}

function record(key) {
  if (!(key in seen)) {
    seen[key] = 1
    keys[++dep_count] = key
  }
  if (dep_state == "unknown") {
    unknown[key] = 1
    return
  }
  if (marker == "-") {
    removed_count[key]++
    removed_version[key "\t" removed_count[key]] = dep_version
  } else {
    added_count[key]++
    added_version[key "\t" added_count[key]] = dep_version
  }
}

END {
  for (idx = 1; idx <= dep_count; idx++) {
    key = keys[idx]
    if (key in unknown) {
      print "__unknown__"
      continue
    }
    if (!(key in removed_count) || !(key in added_count)) {
      print "__unknown__"
      continue
    }
    if (removed_count[key] != added_count[key]) {
      print "__unknown__"
      continue
    }
    for (i = 1; i <= removed_count[key]; i++) {
      print removed_version[key "\t" i] "\t" added_version[key "\t" i]
    }
  }
}
'

# ---------------------------------------------------------------------------
# Semver comparison
# ---------------------------------------------------------------------------

VS_MAJOR=""
VS_MINOR=""
VS_PATCH=""

split_version() {
  local core=${1%%+*}
  core=${core%%-*}
  local major minor patch
  IFS='.' read -r major minor patch <<<"$core"
  VS_MAJOR=$major
  VS_MINOR=${minor:-0}
  VS_PATCH=${patch:-0}
}

is_numeric() {
  [[ $1 =~ ^[0-9]+$ ]]
}

# Prints "major", "minor" or "patch" for the difference between two
# versions. Returns 1 when the versions cannot be compared, or when the
# new version is not an increase -- a downgrade must never be
# auto-merged.
semver_level() {
  local old=$1 new=$2
  local om oi op nm ni np

  split_version "$old"
  om=$VS_MAJOR
  oi=$VS_MINOR
  op=$VS_PATCH

  split_version "$new"
  nm=$VS_MAJOR
  ni=$VS_MINOR
  np=$VS_PATCH

  is_numeric "$om" && is_numeric "$oi" && is_numeric "$op" || return 1
  is_numeric "$nm" && is_numeric "$ni" && is_numeric "$np" || return 1

  if ((10#$om != 10#$nm)); then
    ((10#$nm > 10#$om)) || return 1
    printf 'major\n'
    return 0
  fi
  if ((10#$oi != 10#$ni)); then
    ((10#$ni > 10#$oi)) || return 1
    printf 'minor\n'
    return 0
  fi
  if ((10#$op != 10#$np)); then
    ((10#$np > 10#$op)) || return 1
    printf 'patch\n'
    return 0
  fi
  # Identical numeric core: the difference is a prerelease or build
  # metadata change. Going from a release to a prerelease is a
  # downgrade; everything else is a patch-level change.
  if [[ $old == "$new" ]]; then
    return 1
  fi
  if [[ $old != *-* && $new == *-* ]]; then
    return 1
  fi
  printf 'patch\n'
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

pairs=$(awk "$EXTRACT_AWK")

highest_level=""
unknown=0

while IFS=$'\t' read -r old new; do
  [ -n "$old" ] || continue
  if [[ $old == "__unknown__" ]]; then
    unknown=1
    continue
  fi
  level=$(semver_level "$old" "$new") || {
    unknown=1
    continue
  }
  case $level in
    major) highest_level="version-update:semver-major" ;;
    minor)
      [[ $highest_level == "version-update:semver-major" ]] ||
        highest_level="version-update:semver-minor"
      ;;
    patch)
      [[ -n $highest_level ]] || highest_level="version-update:semver-patch"
      ;;
  esac
done <<<"$pairs"

if ((unknown == 1)); then
  exit 0
fi

printf '%s\n' "$highest_level"
