#!/usr/bin/env bash
# Adopt one OS's recordings from CI artifacts into parity/baseline and push them to the branch.
#
#   parity/ci/adopt-recordings.sh <os> <baseline dir from VM 1> [<baseline dir from VM 2> ...]
#
# Every VM must have recorded the same thing, or nothing is adopted. The primary OS (linux) owns
# the shared, unsuffixed files; any other OS owns only its <name>.<os>.snap files.
set -euo pipefail
os=$1; shift
first=$1
for other in "${@:2}"; do
  diff -r "$first" "$other" >/dev/null || { echo "VMs recorded different things; nothing adopted" >&2; diff -r "$first" "$other" | head -40 >&2; exit 1; }
done
if [ "$os" = linux ]; then
  rsync -a --delete --exclude='*.windows.snap' --exclude='*.macos.snap' "$first/" parity/baseline/
else
  find parity/baseline -name "*.$os.snap" -delete
  (cd "$first" && find . -name "*.$os.snap") | while read -r f; do
    mkdir -p "parity/baseline/$(dirname "$f")"
    cp "$first/$f" "parity/baseline/$f"
  done
fi
git add -A parity/baseline
if git diff --cached --quiet; then echo "$os recordings unchanged"; exit 0; fi
git diff --cached --stat
git -c user.name="github-actions[bot]" -c user.email="41898282+github-actions[bot]@users.noreply.github.com" \
  commit -q -m "parity: adopt $os recordings" -m "Recorded by CI run ${GITHUB_RUN_ID:-local} from commit ${GITHUB_SHA:-unknown}; every VM agreed."
for i in 1 2 3 4 5; do
  git pull -q --rebase origin "$GITHUB_REF_NAME" && git push -q origin "HEAD:$GITHUB_REF_NAME" && exit 0
  sleep $((i * 5))
done
echo "could not push the recordings" >&2
exit 1
