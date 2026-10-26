#!/usr/bin/env bash
#
# Delete Dependabot branches that no longer have an open pull request.
#
# Usage: dependabot-branch-cleanup.sh [--dry-run] [--prefix <prefix>]
#
# Dependabot's `delete-branch: true` removes a branch when its pull
# request is *merged*. It does not remove the branch when the pull
# request is *closed* -- and Dependabot closes a pull request whenever a
# newer version supersedes it, which it does whenever a grouped update
# rolls several versions forward at once. Those branches then sit in the
# repository forever.
#
# This script removes exactly the branches that are no longer needed: a
# branch under the configured prefix with no open pull request pointing
# at it.
#
# Options:
#   --dry-run          Report what would be deleted, delete nothing.
#   --prefix <prefix>  Branch prefix to consider (default: "dependabot/").
#
# Exit status is 0 on success, 1 on a usage error, 2 if the branch or
# pull request list could not be read.

set -euo pipefail

readonly PROGRAM_NAME=${0##*/}

dry_run=0
prefix="dependabot/"

usage() {
  cat <<'EOF'
Usage: dependabot-branch-cleanup.sh [--dry-run] [--prefix <prefix>]

Deletes branches under <prefix> that no open pull request points at.

  --dry-run          Report what would be deleted, delete nothing.
  --prefix <prefix>  Branch prefix to consider (default: "dependabot/").
EOF
}

die() {
  printf '%s: %s\n' "$PROGRAM_NAME" "$*" >&2
  exit 1
}

while [ $# -gt 0 ]; do
  case "$1" in
  --dry-run)
    dry_run=1
    shift
    ;;
  --prefix)
    [ $# -ge 2 ] || die "--prefix needs a value"
    prefix=$2
    shift 2
    ;;
  -h | --help)
    usage
    exit 0
    ;;
  *) die "unexpected argument: $1" ;;
  esac
done

[ -n "$prefix" ] || die "the branch prefix must not be empty"

# The repository is inferred from the checkout, the same way the other
# scripts in this directory do it.
repository=${GITHUB_REPOSITORY:-$(gh repo view --json nameWithOwner --jq .nameWithOwner)}
[ -n "$repository" ] || die "could not determine the repository"

default_branch=$(gh api "repos/${repository}" --jq .default_branch) ||
  die "could not read the default branch"

# The branch names, one per line.
branches=$(gh api "repos/${repository}/branches" --paginate --jq '.[].name') ||
  die "could not list the branches"

# The head branches of every open pull request. A branch in this list is
# still in use and must be kept, even if it is not under the prefix.
open_heads=$(gh pr list --repo "$repository" --state open --limit 100 \
  --json headRefName --jq '.[].headRefName') ||
  die "could not list the open pull requests"

deleted=0
skipped=0

while IFS= read -r branch; do
  [ -n "$branch" ] || continue

  # Only the configured prefix is in scope.
  case "$branch" in
  "$prefix"*) ;;
  *)
    continue
    ;;
  esac

  # Never touch the default branch, whatever the prefix says. This is
  # the safety net for a prefix that is a prefix of the default branch
  # name.
  if [ "$branch" = "$default_branch" ]; then
    printf 'skip  %s (the default branch)\n' "$branch"
    skipped=$((skipped + 1))
    continue
  fi

  # A branch with an open pull request is still in use.
  if printf '%s\n' "$open_heads" | grep -qxF "$branch"; then
    printf 'skip  %s (an open pull request uses it)\n' "$branch"
    skipped=$((skipped + 1))
    continue
  fi

  if ((dry_run == 1)); then
    printf 'would delete  %s\n' "$branch"
  else
    gh api -X DELETE "repos/${repository}/git/refs/heads/${branch}" >/dev/null ||
      die "could not delete ${branch}"
    printf 'delete  %s\n' "$branch"
  fi
  deleted=$((deleted + 1))
done <<<"$branches"

printf '%d branch(es) %s, %d skipped\n' \
  "$deleted" "$(if ((dry_run == 1)); then echo 'would be deleted'; else echo 'deleted'; fi)" "$skipped"
