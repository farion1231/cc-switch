#!/usr/bin/env bash
set -euo pipefail

DB="${DB_PATH:-$HOME/.cc-switch/cc-switch.db}"
TS="$(date +%Y%m%d_%H%M%S)"
BAK="${DB}.bak.hotfix.${TS}"
APP_BIN="/Applications/CC Switch.app/Contents/MacOS/cc-switch"
RESTART_AFTER="${RESTART_AFTER:-1}"
LOCK_DIR="${DB}.hotfix.lock"
WAS_RUNNING=0

if [[ ! -f "$DB" ]]; then
  echo "[ERR] DB not found: $DB" >&2
  exit 1
fi

cleanup() {
  rmdir "$LOCK_DIR" >/dev/null 2>&1 || true
}

restore_backup() {
  if [[ -f "$BAK" ]]; then
    cp "$BAK" "$DB"
    rm -f "${DB}-wal" "${DB}-shm"
    echo "[ROLLBACK] restored DB from backup: $BAK"
  fi
}

on_error() {
  local code=$?
  echo "[ERR] hotfix failed (exit=${code}), starting rollback"
  restore_backup
  if [[ "$RESTART_AFTER" == "1" && "$WAS_RUNNING" -eq 1 ]]; then
    open -a "CC Switch" || true
  fi
  cleanup
  exit "$code"
}

if ! mkdir "$LOCK_DIR" 2>/dev/null; then
  echo "[ERR] hotfix lock exists: $LOCK_DIR" >&2
  echo "[HINT] another hotfix may be running; remove lock only if stale." >&2
  exit 3
fi
trap on_error ERR INT TERM

# Stop writer process before DB patching to avoid concurrent writes.
if pgrep -f "$APP_BIN" >/dev/null 2>&1; then
  echo "[INFO] stopping CC Switch before DB patch"
  WAS_RUNNING=1
  pkill -f "$APP_BIN" || true
  for _ in {1..10}; do
    if ! pgrep -f "$APP_BIN" >/dev/null 2>&1; then
      break
    fi
    sleep 0.3
  done
fi

# Guard: required tables must exist.
for t in providers proxy_config provider_health provider_cooldown; do
  if ! sqlite3 "$DB" "SELECT 1 FROM sqlite_master WHERE type='table' AND name='$t';" | grep -q 1; then
    echo "[ERR] required table missing: $t" >&2
    exit 2
  fi
done

sqlite3 "$DB" ".backup '$BAK'"
echo "[OK] backup created via sqlite backup API: $BAK"

sqlite3 -bail "$DB" <<'SQL'
BEGIN IMMEDIATE;

-- Upstream auth must use real access_token when available.
UPDATE providers
SET settings_config = json_set(
  settings_config,
  '$.auth.auth_mode', 'apikey',
  '$.auth.OPENAI_API_KEY', json_extract(settings_config, '$.auth.tokens.access_token')
)
WHERE app_type='codex'
  AND json_extract(settings_config, '$.auth.tokens.access_token') IS NOT NULL
  AND length(json_extract(settings_config, '$.auth.tokens.access_token')) > 0;

-- Ensure codex providers participate in failover.
UPDATE providers
SET in_failover_queue = 1
WHERE app_type='codex'
  AND json_extract(settings_config, '$.auth.tokens.access_token') IS NOT NULL
  AND length(json_extract(settings_config, '$.auth.tokens.access_token')) > 0;

-- Clear stale health/cooldown that can lock the pool.
DELETE FROM provider_health WHERE app_type='codex';
DELETE FROM provider_cooldown WHERE app_type='codex';

-- Keep proxy enabled.
UPDATE proxy_config
SET proxy_enabled = 1,
    enabled = 1,
    auto_failover_enabled = 1,
    updated_at = datetime('now')
WHERE app_type='codex';

COMMIT;
SQL

echo "[OK] db hotfix applied"

# Quick integrity check.
sqlite3 "$DB" "PRAGMA integrity_check;" | grep -q '^ok$'
echo "[OK] sqlite integrity check passed"

# Show provider status only (avoid leaking token prefixes).
echo "[INFO] codex provider auth status:"
sqlite3 -header -column "$DB" "
SELECT
  name,
  CASE WHEN json_extract(settings_config, '$.auth.OPENAI_API_KEY') IS NULL THEN 'missing' ELSE 'present' END AS api_key_state,
  CASE WHEN json_extract(settings_config, '$.auth.tokens.access_token') IS NULL THEN 'missing' ELSE 'present' END AS token_state,
  in_failover_queue
FROM providers
WHERE app_type='codex';
"

if [[ "$RESTART_AFTER" == "1" ]]; then
  echo "[INFO] starting CC Switch"
  open -a "CC Switch" || true
  sleep 2
fi

trap - ERR INT TERM
cleanup

echo "[ROLLBACK] RESTART_AFTER=0 pkill -f '$APP_BIN' || true; cp '$BAK' '$DB'; rm -f '${DB}-wal' '${DB}-shm'; open -a 'CC Switch'"
echo "[NEXT] run codex_verify.sh"
