#!/usr/bin/env bash
# Denies egress on a hosted macOS runner while leaving the runner agent connected.
#
# The app runs as the same user as the runner agent, and WebKit's networking helper is not a
# child of the app, so nothing can single out the app's processes. pf therefore refuses every new
# outbound TCP connection and all outbound UDP, machine-wide, except to the system's DNS servers
# and to the hosts the runner agent talks to. A learner keeps that allowlist: it reads the
# runner's own diagnostic logs for GitHub Actions and Azure Blob host names, resolves them, adds
# their addresses and pins each name to those addresses in /etc/hosts. The addresses behind these
# names rotate, and without the pin the system resolver can hand the runner one that the table
# does not hold. The app never talks to those hosts, so for the app nothing can leave. Loopback is
# not filtered.
#
# The learner also restores networking by itself after PARITY_EGRESS_MAX_SECONDS (22 minutes by
# default), so a job that hangs while egress is denied still reconnects and reports. It records
# what pf refused and, every half minute, the load and the busiest processes.
#
#   macos-egress.sh start   load the rules and start the learner (needs sudo)
#   macos-egress.sh check   fail unless a public host is refused and a runner host is reachable
#   macos-egress.sh stop    stop the learner, restore the system rules and print the records
set -euo pipefail

STATE="${RUNNER_TEMP:-/tmp}/parity-egress"
MARK="# parity-egress pin"
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

# Allows a host's current addresses and pins the name to them, so the resolver returns no other.
pin() {
  local h=$1 ips
  ips=$(addresses "$h")
  [ -n "$ips" ] || return 0
  sudo pfctl -t runner -T add $ips >/dev/null 2>&1 || true
  for ip in $ips; do printf '%s %s %s\n' "$ip" "$h" "$MARK"; done | sudo tee -a /etc/hosts >/dev/null
  sudo dscacheutil -flushcache 2>/dev/null || true
  sudo killall -HUP mDNSResponder 2>/dev/null || true
}

restore() {
  if [ -f "$STATE/etc-hosts.orig" ]; then
    sudo cp "$STATE/etc-hosts.orig" /etc/hosts
  else
    sudo sed -i '' "/$MARK\$/d" /etc/hosts
  fi
  sudo dscacheutil -flushcache 2>/dev/null || true
  sudo killall -HUP mDNSResponder 2>/dev/null || true
  sudo pfctl -d 2>&1 | tail -1 || true
  sudo pfctl -f /etc/pf.conf 2>&1 | grep -vE 'ALTQ|flushing of rules' || true
}

snapshot() {
  echo "== $(date -u +%T) load $(sysctl -n vm.loadavg) $(memory_pressure 2>/dev/null | grep -m1 'free percentage' || true)"
  ps -axro pid,pcpu,rss,etime,comm | head -6
}

learn() {
  local known="$STATE/hosts" deadline=${PARITY_EGRESS_MAX_SECONDS:-1320} next=0
  while true; do
    if [ "$SECONDS" -ge "$deadline" ]; then
      echo "$(date -u +%T) egress denied for ${deadline}s; restoring networking"
      restore
      return
    fi
    runner_hosts > "$STATE/hosts.now"
    while read -r h; do echo "$(date -u +%T) pin $h"; pin "$h"; done < <(comm -13 "$known" "$STATE/hosts.now")
    sort -u "$STATE/hosts.now" "$known" -o "$known"
    if [ "$SECONDS" -ge "$next" ]; then snapshot >> "$STATE/monitor.log" 2>&1; next=$((SECONDS + 30)); fi
    sleep 2
  done
}

case "${1:-}" in
  start)
    mkdir -p "$STATE"
    # Scoped link-local servers (fe80::1%en0) cannot go into a pf table; the others are enough.
    dns=$(scutil --dns | awk '/nameserver\[[0-9]+\]/ {print $3}' | grep -v '%' | sort -u | tr '\n' ' ')
    [ -n "$dns" ] || { echo "no DNS servers found" >&2; exit 1; }
    runner_hosts > "$STATE/hosts"
    : > "$STATE/pins"
    while read -r h; do
      for ip in $(addresses "$h"); do printf '%s %s %s\n' "$ip" "$h" "$MARK" >> "$STATE/pins"; done
    done < "$STATE/hosts"
    [ -s "$STATE/pins" ] || { echo "no runner host resolved" >&2; exit 1; }
    seed=$(awk '{print $1}' "$STATE/pins" | sort -u | tr '\n' ' ')
    sudo cp /etc/hosts "$STATE/etc-hosts.orig"
    sudo tee -a /etc/hosts < "$STATE/pins" >/dev/null
    sudo dscacheutil -flushcache 2>/dev/null || true
    sudo killall -HUP mDNSResponder 2>/dev/null || true
    {
      echo 'set skip on lo0'
      echo "table <dns> persist { $dns }"
      echo "table <runner> persist { $seed }"
      echo 'pass out quick proto tcp to <runner> no state'
      echo 'pass out quick proto { tcp udp } to <dns> port 53 no state'
      echo 'block return out log quick proto tcp all flags S/SA'
      echo 'block return out log quick proto udp all'
      echo 'pass all no state'
    } > "$STATE/pf.conf"
    sudo pfctl -f "$STATE/pf.conf" -E 2>&1 | grep -vE 'ALTQ|flushing of rules|pf.conf' || true
    nohup bash "$0" learn > "$STATE/learner.log" 2>&1 &
    echo $! > "$STATE/learner.pid"
    # What pf refuses, for telling the app's attempts from the runner's.
    sudo ifconfig pflog0 create 2>/dev/null || true
    sudo nohup tcpdump -n -l -tttt -i pflog0 > "$STATE/blocked.log" 2>&1 &
    echo "dns: $dns"
    echo "runner hosts: $(tr '\n' ' ' < "$STATE/hosts")"
    echo "pinned: $(wc -l < "$STATE/pins" | tr -d ' ') addresses"
    ;;
  learn)
    learn
    ;;
  check)
    if curl -sS -m 10 -o /dev/null https://api.github.com 2>/dev/null; then echo "egress to api.github.com is open" >&2; exit 1; fi
    for h in results-receiver.actions.githubusercontent.com broker.actions.githubusercontent.com; do
      curl -sS -m 10 -o /dev/null "https://$h" || { echo "the runner's own host $h is unreachable" >&2; exit 1; }
    done
    echo "public hosts are refused; the runner's hosts are reachable"
    ;;
  stop)
    [ -f "$STATE/learner.pid" ] && kill "$(cat "$STATE/learner.pid")" 2>/dev/null || true
    sudo pkill -f "tcpdump -n -l -tttt -i pflog0" 2>/dev/null || true
    echo "runner table: $(sudo pfctl -t runner -T show 2>/dev/null | wc -l | tr -d ' ') addresses"
    restore
    echo "== learner"; cat "$STATE/learner.log" 2>/dev/null || true
    echo "== refused destinations (count, first and last time)"
    awk '{for (i = 3; i < NF; i++) if ($i == ">") {d = $(i + 1); sub(/:$/, "", d); n[d]++; if (!(d in f)) f[d] = $2; l[d] = $2; break}}
      END {for (d in n) print n[d], d, f[d], l[d]}' "$STATE/blocked.log" 2>/dev/null | sort -rn | head -25 || true
    echo "== monitor"; cat "$STATE/monitor.log" 2>/dev/null || true
    ;;
  *)
    echo "usage: $0 start|check|stop" >&2; exit 2
    ;;
esac
