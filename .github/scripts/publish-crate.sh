#!/usr/bin/env bash
#
# Publishes a crate to crates.io, treating "already published" as
# success.
#
# The three outcomes are:
#
#   published          -> exit 0, prints "PUBLISHED"
#   already published  -> exit 0, prints "ALREADY_PUBLISHED"
#   anything else      -> exit 1, prints the captured log
#
# The distinction between the first two matters: the release workflow
# waits for a freshly published crate to become available on the crates.io
# index before publishing the crate that depends on it. A crate that was
# already on the index needs no wait.
#
# Usage:
#   publish-crate.sh <package-name> <log-path>
#
# Environment:
#   CARGO_REGISTRY_TOKEN   passed through to cargo publish
#
# `set -o pipefail` is load-bearing. cargo's output is piped through
# `tee` so the log is available for inspection, and without pipefail the
# pipeline's exit status is tee's -- which is always 0. The success
# branch would then be taken for *every* outcome, including a hard
# failure like a rejected token, and the step would go green having
# published nothing.

set -euo pipefail

package="${1:?usage: publish-crate.sh <package-name> <log-path>}"
log_path="${2:?usage: publish-crate.sh <package-name> <log-path>}"

# The version is read for the log message only; cargo decides what it
# actually publishes.
version="$(cargo metadata --no-deps --format-version 1 \
  | jq -r --arg pkg "$package" '.packages[] | select(.name == $pkg) | .version')"

if cargo publish --package "$package" 2>&1 | tee "$log_path"; then
  echo "PUBLISHED $package@$version"
  exit 0
fi

if grep -q "already exists" "$log_path"; then
  echo "ALREADY_PUBLISHED $package@$version"
  exit 0
fi

echo "Publish failed with unexpected error:" >&2
cat "$log_path" >&2
exit 1