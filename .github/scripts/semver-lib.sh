#!/usr/bin/env bash
#
# Version comparison helpers shared by the Dependabot classifiers.
#
# This file is meant to be sourced, not executed:
#
#   source "$(dirname "$0")/semver-lib.sh"
#
# It defines:
#
#   split_version <version>   splits a version into VS_MAJOR / VS_MINOR /
#                             VS_PATCH
#   is_numeric <string>       true when the string is a decimal integer
#   semver_level <old> <new>  prints "major", "minor" or "patch" for the
#                             difference between two versions, and returns
#                             1 when they cannot be compared or when the
#                             change is not an increase
#
# `semver_level` refuses anything that is not a plain increase: a
# downgrade, or a move from a release to a prerelease, is a change that
# must never be auto-merged.

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
