#!/usr/bin/env bash
set -euo pipefail

APP_CACHE_ROOT="${HOME}/Library/Caches/com.ccswitch.desktop"
WEBKIT_ROOT="${APP_CACHE_ROOT}/WebKit"
NETWORK_CACHE="${WEBKIT_ROOT}/NetworkCache"
QUARANTINE_ROOT="${APP_CACHE_ROOT}/.localhost3000-quarantine"
STATE_ROOT="${HOME}/.local/state/ccswitch-localhost-guard"
LOCK_DIR="${STATE_ROOT}/lock"
LOG_FILE="${HOME}/Library/Logs/ccswitch-localhost-guard.log"
RETENTION_DAYS="${RETENTION_DAYS:-14}"
DRY_RUN=0

[[ "${1:-}" == "--dry-run" ]] && DRY_RUN=1

ts() { date -u '+%Y-%m-%dT%H:%M:%SZ'; }
log() { mkdir -p "$(dirname "$LOG_FILE")"; echo "[$(ts)] $*" | tee -a "$LOG_FILE"; }

mkdir -p "$STATE_ROOT"
if ! mkdir "$LOCK_DIR" 2>/dev/null; then log "skip: lock busy"; exit 0; fi
trap 'rmdir "$LOCK_DIR" >/dev/null 2>&1 || true' EXIT

mkdir -p "$APP_CACHE_ROOT" "$WEBKIT_ROOT" "$QUARANTINE_ROOT"

hit=""
while IFS= read -r f; do
  if strings -a "$f" 2>/dev/null | grep -q "Open Presentation Preview in Obsidian first\|http://localhost:3000/"; then
    hit="$f"
    break
  fi
done < <(find "$NETWORK_CACHE" -type f 2>/dev/null)

if [[ -z "$hit" ]]; then
  log "cache clean: no localhost:3000 contamination detected"
  [[ -d "$QUARANTINE_ROOT" ]] && find "$QUARANTINE_ROOT" -mindepth 1 -maxdepth 1 -type d -mtime +"$RETENTION_DAYS" -exec rm -rf {} + 2>/dev/null || true
  exit 0
fi

stamp="$(date +%Y%m%d_%H%M%S)"
dst="${QUARANTINE_ROOT}/${stamp}/running-app/NetworkCache"
log "detected marker in $hit"
if [[ "$DRY_RUN" -eq 1 ]]; then
  log "dry-run: would move $NETWORK_CACHE -> $dst"
  exit 0
fi

mkdir -p "$(dirname "$dst")"
[[ -d "$NETWORK_CACHE" ]] && mv "$NETWORK_CACHE" "$dst"
mkdir -p "$NETWORK_CACHE"
log "quarantined network cache to $dst"
