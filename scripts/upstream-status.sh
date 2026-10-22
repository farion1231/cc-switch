#!/usr/bin/env bash
# Compare the newest stable upstream release with our fork's newest release.
# Prints "NO-CHANGE <fork-tag>" or "UPDATE <upstream-tag>".
set -euo pipefail

UPSTREAM_REPO="${UPSTREAM_REPO:-farion1231/cc-switch}"
FORK_REPO="${FORK_REPO:-misswell/cc-switch-light}"

upstream="$(gh release list -R "$UPSTREAM_REPO" --exclude-pre-releases --limit 1 --json tagName --jq '.[0].tagName // empty')"
[ -n "$upstream" ] || { echo "ERROR: no stable upstream release found" >&2; exit 1; }

fork="$(gh release list -R "$FORK_REPO" --exclude-pre-releases --limit 1 --json tagName --jq '.[0].tagName // empty')"
if [ -z "$fork" ]; then
  echo "UPDATE $upstream"
  exit 0
fi

upstream_version="${upstream#v}"
fork_version="${fork#v}"
newest="$(printf '%s\n%s\n' "$upstream_version" "$fork_version" | sort -V | tail -1)"
if [ "$newest" = "$upstream_version" ] && [ "$upstream_version" != "$fork_version" ]; then
  echo "UPDATE $upstream"
else
  echo "NO-CHANGE $fork"
fi
