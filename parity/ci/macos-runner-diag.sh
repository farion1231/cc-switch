#!/usr/bin/env bash
# Prints what tells a cancelled macOS job apart: the pf rule counters before the rules are
# unloaded, and the runner's own log lines about cancellation, job renewal and connection
# failures. A cancel message from the service means the cause is GitHub; renewal or connect
# errors before it mean egress denial starved the runner agent. The repository is public, so this
# prints matching lines only and never uploads the runner's logs.
set -u

echo "== pf rules with counters"
sudo pfctl -vvsr 2>/dev/null | grep -v ALTQ || true

listener=$(ps -axo command= | grep -m1 -E '[R]unner\.Listener' | awk '{print $1}')
echo "== runner listener: ${listener:-not found}"
root=$listener
diag=""
while [ -n "$root" ] && [ "$root" != "/" ]; do
  root=$(dirname "$root")
  if [ -d "$root/_diag" ]; then diag="$root/_diag"; break; fi
done
if [ -z "$diag" ]; then
  diag=$(find /Users /opt -maxdepth 5 -type d -name _diag 2>/dev/null | head -1)
fi
echo "== runner diagnostics: ${diag:-not found}"
[ -n "$diag" ] || exit 0

pattern='cancel|renew|abandon|lost|HttpRequestException|SocketException|refused|timed out|TaskCanceled'
for kind in Runner Worker; do
  f=$(ls -t "$diag"/${kind}_*.log 2>/dev/null | head -1)
  echo "== ${kind} log: ${f:-none}"
  [ -n "$f" ] && grep -iE "$pattern" "$f" | tail -60 | cut -c1-300
done
exit 0
