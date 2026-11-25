# Capturing Usage from Ephemeral & Agent-Invoked CLI Runs

When one coding agent invokes another CLI tool in non-persisted / ephemeral mode
(for example, Claude Code or Antigravity running `codex exec --ephemeral ...`,
Codex side chats, `codex-plugin-cc` `/codex:review`, or
`claude -p --no-session-persistence`), the invoked CLI does not write a local
session transcript to `~/.codex/sessions/` or `~/.claude/projects/`.

If CC Switch's local proxy takeover is disabled (such as official OAuth / direct
login mode), those runs can still be counted automatically via CC Switch's
built-in loopback OpenTelemetry (OTLP/HTTP JSON) collector.

## Supported Endpoints

CC Switch accepts sanitized OTLP/HTTP JSON log batches on loopback only:

- `POST /v1/cc-switch/otel/logs` (unified endpoint for Codex, Claude Code, and Gemini CLI)
- `POST /v1/cc-switch/codex/logs` (Codex-compatible alias)
- `POST /v1/logs` (standard OTLP/HTTP logs path)
- `GET /cc-switch/otel/health` (liveness check)

These routes are served automatically in two ways:

1. **On the main CC Switch local proxy port** (default `127.0.0.1:15721`) whenever
   the local proxy server is running — no extra port required.
2. **On a standalone loopback port** (for example `127.0.0.1:4319`) started
   automatically at CC Switch launch whenever configured in `~/.codex/config.toml`,
   `~/.claude/settings.json`, or `CC_SWITCH_OTEL_LISTEN_PORT` — works even when the
   local proxy server is OFF.

## Codex (`codex exec --ephemeral`, side chats, `codex-plugin-cc`)

Add the following to `~/.codex/config.toml` (or Codex common configuration in
CC Switch; CC Switch preserves `[otel]` across provider switches):

```toml
[otel]
log_user_prompt = false
exporter = { otlp-http = { endpoint = "http://127.0.0.1:4319/v1/cc-switch/codex/logs", protocol = "json" } }
```

Restart CC Switch after adding the `[otel]` block (or point `endpoint` directly
at your running CC Switch proxy port, e.g.
`http://127.0.0.1:15721/v1/cc-switch/codex/logs`).

- Captures `codex.sse_event` (`event.kind = response.completed`) events with
  model, token counts (including cache reads/writes), `duration_ms`, `ttft_ms`,
  `service_tier` (including `ultrafast`, `fast`, `priority`, `default`, `flex`,
  `auto`), and `model_reasoning_effort`.
- Appears under **Codex (Temporary)** (`data_source = codex_otel`).
- Automatically skips persisted threads present in `state_*.sqlite` or
  `codex_session`, and deduplicates against `proxy` logs so nothing is ever
  double-counted.

## Claude Code (`claude -p --no-session-persistence`)

Add the following to `~/.claude/settings.json` under `env` (or pass in the
environment when invoking `claude -p --no-session-persistence`):

```json
{
  "env": {
    "CLAUDE_CODE_ENABLE_TELEMETRY": "1",
    "OTEL_LOGS_EXPORTER": "otlp",
    "OTEL_EXPORTER_OTLP_PROTOCOL": "http/json",
    "OTEL_EXPORTER_OTLP_ENDPOINT": "http://127.0.0.1:4319"
  }
}
```

- Captures `claude_code.api_request` log events under **Claude (Temporary)**
  (`data_source = claude_otel`).
- Deduplicates automatically against both `proxy` logs and persisted
  `session_log` entries.

## Privacy & Security

- Loopback-only (`127.0.0.1` / `localhost`); browser requests carrying an
  `Origin` header are rejected with `403 Forbidden`.
- Only sanitized accounting fields (app, session ID, model, timestamps, token
  counts, duration/TTFT, tier/effort) are stored. Prompts, tool outputs,
  account emails, headers, and raw bodies are discarded on ingest.
- Pending OTLP queue rows (`otel_usage_ingest`) stay local-only and are never
  uploaded during WebDAV/S3 sync.
