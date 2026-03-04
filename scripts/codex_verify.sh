#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${OPENAI_BASE_URL:-http://127.0.0.1:15721/v1}"
API_KEY="${OPENAI_API_KEY:-sk-cc-switch-proxy}"
DB="${DB_PATH:-$HOME/.cc-switch/cc-switch.db}"
LOG="${LOG_PATH:-$HOME/.cc-switch/logs/cc-switch.log}"
SAMPLE_ROWS="${SAMPLE_ROWS:-1000}"
MIN_SAMPLE_ROWS="${MIN_SAMPLE_ROWS:-100}"
LOG_TAIL_LINES="${LOG_TAIL_LINES:-4000}"
MAX_AUTH_401_PCT="${MAX_AUTH_401_PCT:-5}"
MAX_QUOTA_429_PCT="${MAX_QUOTA_429_PCT:-5}"
MAX_UPSTREAM_5XX_PCT="${MAX_UPSTREAM_5XX_PCT:-10}"
ENABLE_CODEX_SMOKE="${ENABLE_CODEX_SMOKE:-1}"

status=0
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT
LSOF_OUT="$TMP_DIR/lsof.txt"
HEALTH_OUT="$TMP_DIR/health.json"
MODELS_OUT="$TMP_DIR/models.json"
SMOKE_OUT="$TMP_DIR/smoke.txt"
SQL_ERR_OUT="$TMP_DIR/sql.err"

sanitize() {
  sed -E 's/sk-[A-Za-z0-9._-]+/sk-***REDACTED***/g; s/(Bearer )[A-Za-z0-9._-]+/\1***REDACTED***/g'
}

url_no_scheme="${BASE_URL#*://}"
host_port="${url_no_scheme%%/*}"
if [[ "$host_port" == *:* ]]; then
  LISTEN_PORT="${host_port##*:}"
else
  if [[ "$BASE_URL" == https://* ]]; then
    LISTEN_PORT="443"
  else
    LISTEN_PORT="80"
  fi
fi

echo "[STEP] listener check"
if lsof -nP -iTCP:"${LISTEN_PORT}" -sTCP:LISTEN >"$LSOF_OUT" 2>/dev/null; then
  echo "[OK] ${LISTEN_PORT} listening"
else
  echo "[ERR] ${LISTEN_PORT} not listening"
  status=1
fi

echo "[STEP] /health"
if curl -sS -m 5 "${BASE_URL%/v1}/health" >"$HEALTH_OUT"; then
  echo "[OK] health reachable"
else
  echo "[ERR] health unreachable"
  status=1
fi

echo "[STEP] /v1/models"
umask 077
CURL_CFG="$(mktemp)"
trap 'rm -f "$CURL_CFG"' EXIT
cat >"$CURL_CFG" <<CFG
silent
show-error
max-time = 10
url = "$BASE_URL/models"
header = "Authorization: Bearer $API_KEY"
output = "$MODELS_OUT"
write-out = "%{http_code}"
CFG
HTTP_CODE="$(curl -K "$CURL_CFG" || true)"
if [[ "$HTTP_CODE" == "200" ]]; then
  echo "[OK] models endpoint 200"
else
  echo "[ERR] models endpoint HTTP ${HTTP_CODE:-unknown}"
  status=1
fi

if [[ "$ENABLE_CODEX_SMOKE" == "1" ]]; then
  echo "[STEP] codex exec smoke"
  if OPENAI_BASE_URL="$BASE_URL" OPENAI_API_KEY="$API_KEY" \
    codex exec --skip-git-repo-check "Reply with exactly OK" >"$SMOKE_OUT" 2>&1; then
    smoke_last_line="$(tr -d '\r' <"$SMOKE_OUT" | awk 'NF{line=$0} END{print line}')"
    if [[ "$smoke_last_line" == "OK" ]]; then
      echo "[OK] codex smoke passed"
    else
      echo "[ERR] codex smoke output mismatch (last_line='${smoke_last_line}')"
      status=1
    fi
  else
    echo "[ERR] codex smoke failed"
    tail -n 40 "$SMOKE_OUT" | sanitize
    status=1
  fi
else
  echo "[STEP] codex exec smoke"
  echo "[SKIP] ENABLE_CODEX_SMOKE=0"
fi

echo "[STEP] recent status-code distribution (last ${SAMPLE_ROWS} rows)"
if [[ -f "$DB" ]]; then
  if ! sqlite3 -header -column "$DB" "
  WITH recent AS (
    SELECT status_code
    FROM proxy_request_logs
    WHERE app_type='codex'
    ORDER BY rowid DESC
    LIMIT ${SAMPLE_ROWS}
  )
  SELECT status_code, count(*) AS cnt
  FROM recent
  GROUP BY status_code
  ORDER BY status_code;
  "; then
    echo "[ERR] failed to query status-code distribution"
    status=1
  fi

  echo "[STEP] error-ratio gate (same sample window)"
  metrics="$(sqlite3 -csv "$DB" "
  WITH recent AS (
    SELECT status_code
    FROM proxy_request_logs
    WHERE app_type='codex'
    ORDER BY rowid DESC
    LIMIT ${SAMPLE_ROWS}
  )
  SELECT
    COUNT(*) AS total,
    SUM(CASE WHEN status_code=401 THEN 1 ELSE 0 END) AS auth_401,
    SUM(CASE WHEN status_code=429 THEN 1 ELSE 0 END) AS quota_429,
    SUM(CASE WHEN status_code>=500 THEN 1 ELSE 0 END) AS upstream_5xx
  FROM recent;
  " 2>"$SQL_ERR_OUT" || true)"

  if [[ -z "$metrics" ]]; then
    echo "[ERR] failed to query error-ratio metrics"
    cat "$SQL_ERR_OUT" | sanitize
    status=1
  else
    IFS=',' read -r total auth_401 quota_429 upstream_5xx <<<"$metrics"
    total="${total:-0}"
    auth_401="${auth_401:-0}"
    quota_429="${quota_429:-0}"
    upstream_5xx="${upstream_5xx:-0}"

    echo "total=${total} auth_401=${auth_401} quota_429=${quota_429} upstream_5xx=${upstream_5xx}"

    if (( total < MIN_SAMPLE_ROWS )); then
      echo "[ERR] sample too small: total=${total}, min=${MIN_SAMPLE_ROWS}"
      status=1
    else
      auth_401_pct="$(awk -v n="$auth_401" -v d="$total" 'BEGIN{printf "%.2f", (n*100)/d}')"
      quota_429_pct="$(awk -v n="$quota_429" -v d="$total" 'BEGIN{printf "%.2f", (n*100)/d}')"
      upstream_5xx_pct="$(awk -v n="$upstream_5xx" -v d="$total" 'BEGIN{printf "%.2f", (n*100)/d}')"

      echo "auth_401_pct=${auth_401_pct}% (limit ${MAX_AUTH_401_PCT}%)"
      echo "quota_429_pct=${quota_429_pct}% (limit ${MAX_QUOTA_429_PCT}%)"
      echo "upstream_5xx_pct=${upstream_5xx_pct}% (limit ${MAX_UPSTREAM_5XX_PCT}%)"

      if ! awk -v a="$auth_401_pct" -v b="$MAX_AUTH_401_PCT" 'BEGIN{exit !(a<=b)}'; then
        echo "[ERR] auth_401 ratio exceeded"
        status=1
      fi
      if ! awk -v a="$quota_429_pct" -v b="$MAX_QUOTA_429_PCT" 'BEGIN{exit !(a<=b)}'; then
        echo "[ERR] quota_429 ratio exceeded"
        status=1
      fi
      if ! awk -v a="$upstream_5xx_pct" -v b="$MAX_UPSTREAM_5XX_PCT" 'BEGIN{exit !(a<=b)}'; then
        echo "[ERR] upstream_5xx ratio exceeded"
        status=1
      fi
    fi
  fi
else
  echo "[ERR] db not found: $DB"
  status=1
fi

echo "[STEP] loop-keyword scan (tail ${LOG_TAIL_LINES} lines)"
if [[ -f "$LOG" ]]; then
  if ! tail -n "${LOG_TAIL_LINES}" "$LOG" | \
    rg -n "no healthy upstream|所有 Provider 均失败|Incorrect API key provided|stream disconnected before completion|Reconnecting|熔断器触发" -S | \
    tail -n 60 | sanitize; then
    echo "[OK] no loop-keyword hit in recent log tail"
  fi
else
  echo "[ERR] log not found: $LOG"
  status=1
fi

if [[ "$status" -eq 0 ]]; then
  echo "[PASS] verify completed"
else
  echo "[FAIL] verify completed with errors"
fi

exit "$status"
