#!/usr/bin/env bash
set -euo pipefail

AUTH="${HOME}/.codex/auth.json"
[[ -f "$AUTH" ]] || exit 0

tmp="$(mktemp)"
if jq '
  . as $o
  | .auth_mode = "apikey"
  | .OPENAI_API_KEY = (
      if (($o.tokens.access_token // "") | length) > 0 then
        $o.tokens.access_token
      elif (($o.OPENAI_API_KEY // "") | length) > 0 and ($o.OPENAI_API_KEY != "PROXY_MANAGED") then
        $o.OPENAI_API_KEY
      else
        "sk-cc-switch-proxy"
      end
    )
' "$AUTH" > "$tmp"; then
  mv "$tmp" "$AUTH"
  chmod 600 "$AUTH" || true
else
  rm -f "$tmp"
  exit 1
fi
