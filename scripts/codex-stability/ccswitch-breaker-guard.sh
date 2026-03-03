#!/usr/bin/env bash
set -euo pipefail

DB="${CCSWITCH_DB_PATH:-$HOME/.cc-switch/cc-switch.db}"
LOG="${HOME}/Library/Logs/ccswitch-breaker-guard.log"
STATE_DIR="${HOME}/.local/state/ccswitch-breaker-guard"
LOCK_DIR="$STATE_DIR/lock"
DRY_RUN=0
[[ "${1:-}" == "--dry-run" ]] && DRY_RUN=1

ts() { date -u '+%Y-%m-%dT%H:%M:%SZ'; }
log() { mkdir -p "$(dirname "$LOG")"; echo "[$(ts)] $*" | tee -a "$LOG"; }

[[ -f "$DB" ]] || { log "db_missing: $DB"; exit 0; }
mkdir -p "$STATE_DIR"
if ! mkdir "$LOCK_DIR" 2>/dev/null; then log "skip: lock busy"; exit 0; fi
trap 'rmdir "$LOCK_DIR" >/dev/null 2>&1 || true' EXIT

token_count="$(sqlite3 "$DB" "select count(*) from providers where app_type='codex' and json_extract(settings_config,'$.auth.tokens.access_token') is not null;" || echo 0)"
unhealthy_count="$(sqlite3 "$DB" "select count(*) from provider_health where app_type='codex' and is_healthy=0;" || echo 0)"
poison_key_count="$(sqlite3 "$DB" "select count(*) from provider_health where app_type='codex' and lower(coalesce(last_error,'')) like '%invalid_api_key%' and (lower(coalesce(last_error,'')) like '%sk-cc-sw%' or lower(coalesce(last_error,'')) like '%proxy_managed%');" || echo 0)"
poison_shape_count="$(sqlite3 "$DB" "select count(*) from provider_health where app_type='codex' and (lower(coalesce(last_error,'')) like '%input must be a list%' or lower(coalesce(last_error,'')) like '%stream must be set to true%' or lower(coalesce(last_error,'')) like '%store must be set to false%' or lower(coalesce(last_error,'')) like '%invalid value: ''text''%');" || echo 0)"

log "status token_count=$token_count unhealthy_count=$unhealthy_count poison_key=$poison_key_count poison_shape=$poison_shape_count"

should_heal=0
reason=""
if [[ "${poison_key_count:-0}" -gt 0 ]]; then
  should_heal=1
  reason="dummy_key_poison"
elif [[ "${token_count:-0}" -gt 0 && "${unhealthy_count:-0}" -ge "${token_count:-0}" && "${poison_shape_count:-0}" -gt 0 ]]; then
  should_heal=1
  reason="malformed_request_poison"
fi

if [[ "$should_heal" -eq 0 ]]; then log "no_action"; exit 0; fi
if [[ "$DRY_RUN" -eq 1 ]]; then log "dry_run_heal reason=$reason"; exit 0; fi

cp "$DB" "${DB}.bak.breaker-guard-$(date +%Y%m%d_%H%M%S)"
sqlite3 "$DB" "update providers set settings_config=json_set(settings_config,'$.auth.OPENAI_API_KEY',json_extract(settings_config,'$.auth.tokens.access_token')) where app_type='codex' and json_extract(settings_config,'$.auth.tokens.access_token') is not null;"
sqlite3 "$DB" "update providers set in_failover_queue=1 where app_type='codex' and json_extract(settings_config,'$.auth.tokens.access_token') is not null;"
sqlite3 "$DB" "delete from provider_cooldown where app_type='codex';"
sqlite3 "$DB" "delete from provider_health where app_type='codex';"

healthy_id="$(sqlite3 "$DB" "select id from providers where app_type='codex' and json_extract(settings_config,'$.auth.tokens.access_token') is not null order by is_current desc, sort_index asc limit 1;" || true)"
if [[ -n "$healthy_id" ]]; then
  sqlite3 "$DB" "update providers set is_current=0 where app_type='codex';"
  sqlite3 "$DB" "update providers set is_current=1 where app_type='codex' and id='$healthy_id';"
fi

log "healed reason=$reason current=${healthy_id:-none}"
