#!/usr/bin/env bash
#
# Print the author login of a pull request, refusing anything that is not
# a Dependabot pull request.
#
# Usage: dependabot-author.sh <pr-url>
#
# The author is read from the REST API (`pulls[].user.login`) rather than
# from `gh pr view --json author`. The latter reports the GitHub App slug
# (`app/dependabot`), while the webhook payload and
# `github.event.pull_request.user.login` both use the bot login
# (`dependabot[bot]`). The REST API is the one that agrees with the
# trigger, so it is the one used here.
#
# Exit codes:
#   0  the pull request is authored by Dependabot; its login is printed
#   1  the pull request exists but is not authored by Dependabot
#   2  usage error
#   3  the pull request could not be looked up

set -euo pipefail

# The bot login is overridable so the script can be pointed at a different
# bot in tests.
DEPENDABOT_LOGIN="${DEPENDABOT_LOGIN:-dependabot[bot]}"

pr_url="${1:-}"
if [ -z "$pr_url" ]; then
  echo "usage: $(basename "$0") <pr-url>" >&2
  exit 2
fi

if ! number="$(gh pr view "$pr_url" --json number --jq .number)"; then
  echo "Could not look up pull request ${pr_url}." >&2
  exit 3
fi

repository="${GITHUB_REPOSITORY:-}"
if [ -z "$repository" ]; then
  if ! repository="$(gh repo view --json nameWithOwner --jq .nameWithOwner)"; then
    echo "Could not determine the repository for ${pr_url}." >&2
    exit 3
  fi
fi

if ! author="$(gh api "repos/${repository}/pulls/${number}" --jq .user.login)"; then
  echo "Could not read the author of pull request ${pr_url}." >&2
  exit 3
fi

if [ "$author" != "$DEPENDABOT_LOGIN" ]; then
  echo "Pull request ${pr_url} is authored by '${author}', not '${DEPENDABOT_LOGIN}'." >&2
  exit 1
fi

echo "$author"
