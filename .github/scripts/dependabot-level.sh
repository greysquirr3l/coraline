#!/usr/bin/env bash
#
# Classify a Dependabot pull request as a semver major/minor/patch bump.
#
# Usage: dependabot-level.sh <pr-url>
#
# Prints the bump level of the pull request:
#
#   version-update:semver-major
#   version-update:semver-minor
#   version-update:semver-patch
#
# Prints nothing when the level cannot be established. Callers must treat
# an empty result as "do not auto-merge": this script is a conservative
# classifier, never a licence to merge something it does not fully
# understand.
#
# Why this exists
# ---------------
# `dependabot/fetch-metadata` derives the update type from the Dependabot
# commit message. It cannot be used directly here, for two reasons.
#
# First, Cargo *requirement* updates have no "from" clause:
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
# Second, `fetch-metadata` reads `github.event.pull_request` from the
# event payload and fails outright when the key is absent, so it cannot
# run at all under `workflow_dispatch`.
#
# This script therefore reads the Dependabot commit message itself and
# consults two sources, in order:
#
#   1. The commit message. Dependabot records the level it computed in an
#      `update-type:` field of the metadata block, and repeats the old and
#      new versions in the prose above it. This covers every ecosystem
#      that records the old version, which is every ecosystem except
#      Cargo in this repository.
#
#   2. The unified diff of the pull request, via `dependabot-semver.sh`.
#      This is the Cargo fallback: the level is reconstructed from the
#      manifest diff, where the old requirement is still visible.
#
# Exit status is 0 whenever a level could be determined (or could not be
# determined and was reported as "unknown"); only usage errors exit
# non-zero.

set -euo pipefail

readonly PROGRAM_NAME=${0##*/}
SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
readonly SCRIPT_DIR

# shellcheck source-path=SCRIPTDIR
source "${SCRIPT_DIR}/semver-lib.sh"

usage() {
  cat <<'EOF'
Usage: dependabot-level.sh <pr-url>

Prints the highest dependency bump level of a Dependabot pull request:

  version-update:semver-major
  version-update:semver-minor
  version-update:semver-patch

Nothing is printed when the level cannot be established.
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
"")
  usage >&2
  die "a pull request URL is required"
  ;;
esac

readonly PR_URL=$1

# ---------------------------------------------------------------------------
# Parsing the commit message
# ---------------------------------------------------------------------------
#
# Dependabot writes two different shapes of commit message.
#
# A grouped update repeats one line per dependency and records the level
# it computed in a metadata block:
#
#   Updates `github/codeql-action/init` from 4.36.2 to 4.39.1
#
#   updated-dependencies:
#   - dependency-name: github/codeql-action/init
#     dependency-version: 4.39.1
#     update-type: version-update:semver-minor
#
# A single update has no per-dependency line and phrases the bump
# differently:
#
#   Bumps [actions/upload-artifact](https://github.com/actions/upload-artifact)
#   from 7.0.1 to 7.0.2.
#
# but still records the level in the metadata block.
#
# The parsing is done with awk rather than sed. POSIX awk supports
# alternation in its patterns and BSD sed -- still the default on macOS --
# does not, so a sed-based parser silently matches nothing on a
# developer's machine and matches everything on the CI runner. A
# classifier that disagrees with itself depending on the platform is
# worse than no classifier at all.
#
# The awk program emits one tab-separated record per fact found:
#
#   L<TAB>level        an explicit `update-type:` field
#   P<TAB>old<TAB>new  a version pair from the prose
# shellcheck disable=SC2016
readonly PARSE_AWK='
/^[[:space:]]*update-type:[[:space:]]*version-update:semver-(major|minor|patch)[[:space:]]*$/ {
  level = $0
  sub(/^.*version-update:semver-/, "", level)
  sub(/[[:space:]]*$/, "", level)
  print "L\t" level
}

/^Updates `[^`]*` from [^ ]+ to [^ ]+$/ {
  body = $0
  sub(/^Updates `[^`]*` from /, "", body)
  old = body
  sub(/ to .*$/, "", old)
  new = body
  sub(/^.* to /, "", new)
  print "P\t" old "\t" new
}

/^Bumps .* from [^ ]+ to [^ ]+\.$/ {
  body = $0
  sub(/^Bumps .* from /, "", body)
  old = body
  sub(/ to .*$/, "", old)
  new = body
  sub(/^.* to /, "", new)
  sub(/\.$/, "", new)
  print "P\t" old "\t" new
}
'

# ---------------------------------------------------------------------------
# Semver levels
# ---------------------------------------------------------------------------
#
# `highest_level` keeps the strongest level seen so far; `poisoned` marks
# a change that could not be classified and must therefore suppress the
# whole result.

highest_level=""
poisoned=0

record_level() {
  local level=$1
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
}

# ---------------------------------------------------------------------------
# Reading the pull request
# ---------------------------------------------------------------------------

readonly REPOSITORY=${GITHUB_REPOSITORY:-$(gh repo view --json nameWithOwner --jq .nameWithOwner)}

pr_number=$(gh pr view "$PR_URL" --json number --jq .number) ||
  die "could not resolve the pull request number for ${PR_URL}"

head_sha=$(gh api "repos/${REPOSITORY}/pulls/${pr_number}" --jq .head.sha) ||
  die "could not read the head commit of ${PR_URL}"

commit_message=$(gh api "repos/${REPOSITORY}/commits/${head_sha}" --jq .commit.message) ||
  die "could not read the commit message of ${PR_URL}"

parsed=$(printf '%s\n' "$commit_message" | awk "$PARSE_AWK")

explicit_levels=$(printf '%s\n' "$parsed" | awk -F'\t' '$1 == "L" { print $2 }')
version_pairs=$(printf '%s\n' "$parsed" | awk -F'\t' '$1 == "P" { print $2 "\t" $3 }')

# ---------------------------------------------------------------------------
# Source 1: the commit message
# ---------------------------------------------------------------------------
#
# The explicit `update-type:` field is what `fetch-metadata` prefers, so
# it is preferred here too. The version pairs are the fallback for a
# message that carries the versions but not the field.

if [[ -n $explicit_levels ]]; then
  while IFS= read -r level; do
    [[ -n $level ]] || continue
    record_level "$level"
  done <<<"$explicit_levels"
  printf '%s\n' "$highest_level"
  exit 0
fi

if [[ -n $version_pairs ]]; then
  while IFS=$'\t' read -r old new; do
    [[ -n $old ]] || continue
    if ! level=$(semver_level "$old" "$new"); then
      # A pair that cannot be compared -- a downgrade, a prerelease, a
      # non-numeric version -- suppresses the result.
      poisoned=1
      continue
    fi
    record_level "$level"
  done <<<"$version_pairs"

  if ((poisoned == 0)); then
    printf '%s\n' "$highest_level"
  fi
  exit 0
fi

# ---------------------------------------------------------------------------
# Source 2: the manifest diff
# ---------------------------------------------------------------------------
#
# A Cargo requirement update carries no old version in the commit
# message, so the level has to come from the diff, where the old
# requirement is still visible.

diff_classifier="${SCRIPT_DIR}/dependabot-semver.sh"
if [[ ! -x $diff_classifier ]]; then
  die "${diff_classifier} is missing or not executable"
fi

gh pr diff "$PR_URL" | "$diff_classifier"
