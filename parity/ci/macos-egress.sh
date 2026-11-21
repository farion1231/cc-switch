#!/usr/bin/env bash
# Denies egress on a hosted macOS runner while leaving the runner agent connected.
#
# The app runs as the same user as the runner agent, and WebKit's networking helper is not a
# child of the app, so nothing can single out the app's processes. pf therefore refuses every new
# outbound TCP connection and all outbound UDP, machine-wide, except to the system's DNS servers
# and to the hosts the runner agent talks to. A learner keeps that allowlist: it reads the
# runner's own diagnostic logs for GitHub Actions and Azure Blob host names, resolves them and adds
# their addresses. The app never talks to those hosts, so for the app nothing can leave. Loopback
# is not filtered.
#
#   macos-egress.sh start   load the rules and start the learner (needs sudo)
#   macos-egress.sh check   fail unless a public host is refused and a runner host is reachable
#   macos-egress.sh stop    stop the learner and restore the system rules
set -euo pipefail

STATE="${RUNNER_TEMP:-/tmp}/parity-egress"
SUFFIXES='([a-z0-9-]+\.)+(actions\.githubusercontent\.com|blob\.core\.windows\.net)'
SEED="broker.actions.githubusercontent.com pipelines.actions.githubusercontent.com results-receiver.actions.githubusercontent.com"

diag_dir() {
  local listener root
  listener=$(ps -axo command= | grep -m1 -E '[R]unner\.Listener' | awk '{print $1}' || true)
  root=$listener
  while [ -n "$root" ] && [ "$root" != "/" ]; do
    root=$(dirname "$root")
    if [ -d "$root/_diag" ]; then echo "$root/_diag"; return; fi
  done
}

runner_hosts() {
  local diag
  diag=$(diag_dir)
  {
    printf '%s\n' $SEED
    [ -n "$diag" ] && cat "$diag"/Runner_*.log "$diag"/Worker_*.log 2>/dev/null | grep -oiE "$SUFFIXES" || true
  } | tr 'A-Z' 'a-z' | sort -u
}

addresses() {
  dig +short +time=2 +tries=1 A "$1" 2>/dev/null | grep -E '^[0-9.]+$' || true
  dig +short +time=2 +tries=1 AAAA "$1" 2>/dev/null | grep -E '^[0-9a-f:]+$' | grep ':' || true
}

learn() {
  local known="$STATE/hosts" ips
  : > "$known"
  while true; do
    runner_hosts > "$STATE/hosts.now"
    while read -r h; do
      ips=$(addresses "$h")
      [ -n "$ips" ] && sudo pfctl -t runner -T add $ips >/dev/null 2>&1 || true
    done < <(comm -13 "$known" "$STATE/hosts.now")
    sort -u "$STATE/hosts.now" "$known" -o "$known"
    sleep 2
    # Addresses behind these names rotate; refresh all of them every half minute.
    if [ $((SECONDS % 30)) -lt 2 ]; then
      while read -r h; do ips=$(addresses "$h"); [ -n "$ips" ] && sudo pfctl -t runner -T add $ips >/dev/null 2>&1 || true; done < "$known"
    fi
  done
}

case "${1:-}" in
  start)
    mkdir -p "$STATE"
    # Scoped link-local servers (fe80::1%en0) cannot go into a pf table; the others are enough.
    dns=$(scutil --dns | awk '/nameserver\[[0-9]+\]/ {print $3}' | grep -v '%' | sort -u | tr '\n' ' ')
    [ -n "$dns" ] || { echo "no DNS servers found" >&2; exit 1; }
    seed=""
    while read -r h; do seed="$seed $(addresses "$h" | tr '\n' ' ')"; done < <(runner_hosts)
    [ -n "${seed// /}" ] || { echo "no runner host resolved" >&2; exit 1; }
    {
      echo 'set skip on lo0'
      echo "table <dns> persist { $dns }"
      echo "table <runner> persist { $seed }"
      echo 'pass out quick proto tcp to <runner> no state'
      echo 'pass out quick proto { tcp udp } to <dns> port 53 no state'
      echo 'block return out quick proto tcp all flags S/SA'
      echo 'block return out quick proto udp all'
      echo 'pass all no state'
    } > "$STATE/pf.conf"
    sudo pfctl -f "$STATE/pf.conf" -E 2>&1 | grep -vE 'ALTQ|flushing of rules|pf.conf' || true
    nohup bash "$0" learn > "$STATE/learner.log" 2>&1 &
    echo $! > "$STATE/learner.pid"
    echo "dns: $dns"
    echo "runner hosts: $(runner_hosts | tr '\n' ' ')"
    ;;
  learn)
    learn
    ;;
  check)
    if curl -sS -m 10 -o /dev/null https://api.github.com 2>/dev/null; then echo "egress to api.github.com is open" >&2; exit 1; fi
    curl -sS -m 10 -o /dev/null https://results-receiver.actions.githubusercontent.com \
      || { echo "the runner's own hosts are unreachable" >&2; exit 1; }
    echo "public hosts are refused; the runner's hosts are reachable"
    ;;
  stop)
    [ -f "$STATE/learner.pid" ] && kill "$(cat "$STATE/learner.pid")" 2>/dev/null || true
    echo "runner table: $(sudo pfctl -t runner -T show 2>/dev/null | wc -l | tr -d ' ') addresses"
    sudo pfctl -d 2>&1 | tail -1 || true
    sudo pfctl -f /etc/pf.conf 2>&1 | grep -vE 'ALTQ|flushing of rules' || true
    ;;
  *)
    echo "usage: $0 start|check|stop" >&2; exit 2
    ;;
esac
