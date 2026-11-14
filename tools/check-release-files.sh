#!/usr/bin/env bash
# Checks the files of a release against its tag. Reads one "name digest" line per file on stdin
# (digest as "sha256:<hex>"). Exits 1, with a ::error:: line for each problem, when:
#   - a file name carries a version that isn't the tag's (Beta 33 once carried Beta 32's installer),
#   - there isn't exactly one Windows installer (*-setup.exe),
#   - the installer is byte-identical to another release's (needs GH_TOKEN and GITHUB_REPOSITORY).
# Usage: tools/check-release-files.sh v0.90.0-beta.33 < list
set -u
tag="${1:?usage: check-release-files.sh <tag>}"
ver="${tag#v}"
list=$(cat)
fail=0
echo "$list"

while read -r name digest; do
  [ -n "$name" ] || continue
  # makepkg forbids hyphens in pkgver: 0.90.0-beta.42 becomes 0.90.0beta.42.
  if [[ "$name" =~ ^recomplauncher-(.+)-[0-9]+-x86_64\.pkg\.tar\.zst$ ]]; then
    if [ "${BASH_REMATCH[1]}" != "${ver/-/}" ]; then
      echo "::error::$name isn't the Arch package for $ver."
      fail=1
    fi
    continue
  fi
  for found in $(echo "$name" | grep -oE '[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z]+\.[0-9]+)?' || true); do
    if [ "$found" != "$ver" ]; then
      echo "::error::$name is version $found, but this release is $ver."
      fail=1
    fi
  done
done <<< "$list"

n=$(echo "$list" | grep -c -- '-setup\.exe ' || true)
if [ "$n" != "1" ]; then
  echo "::error::Expected one Windows installer, found $n."
  fail=1
fi

exe=$(echo "$list" | grep -- '-setup\.exe ' | awk '{print $2}' | head -n1)
if [ -n "$exe" ] && [ -n "${GH_TOKEN:-}" ] && [ -n "${GITHUB_REPOSITORY:-}" ]; then
  others=$(gh api "repos/$GITHUB_REPOSITORY/releases?per_page=30" \
    --jq ".[] | select(.tag_name != \"$tag\") | .tag_name as \$t | .assets[] | select(.digest == \"$exe\") | \$t")
  if [ -n "$others" ]; then
    echo "::error::The Windows installer is the same file as the one in $others."
    fail=1
  fi
fi

[ "$fail" = 0 ] && echo "Release files match $tag."
exit "$fail"
