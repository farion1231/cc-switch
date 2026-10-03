"""Opt-in Hermes hook observer. This never stores request or response bodies."""

import sqlite3
import time
import uuid
from contextlib import closing
from pathlib import Path


LEDGER_NAME = "ccswitch-usage.sqlite"
REQUIRED_HOOKS = ("post_api_request", "api_request_error", "post_auxiliary_call")


def _counter(value):
    if value is None:
        return None
    try:
        number = int(value)
    except (TypeError, ValueError):
        return None
    return number if number >= 0 else None


def _milliseconds(value):
    try:
        return max(0, round(float(value) * 1000))
    except (TypeError, ValueError):
        return None


def _baseline(home):
    state = home / "state.db"
    if not state.is_file():
        return None
    # A separate read-only connection sees live WAL frames but cannot mutate Hermes.
    try:
        with closing(sqlite3.connect(state.as_uri() + "?mode=ro", uri=True, timeout=2)) as source:
            source.execute("PRAGMA query_only=ON")
            totals = source.execute("""
                SELECT COALESCE(SUM(api_call_count), 0),
                       COALESCE(SUM(input_tokens), 0),
                       COALESCE(SUM(output_tokens), 0),
                       COALESCE(SUM(cache_read_tokens), 0),
                       COALESCE(SUM(cache_write_tokens), 0),
                       COALESCE(SUM(reasoning_tokens), 0),
                       COALESCE(SUM(CAST(CASE
                           WHEN LOWER(COALESCE(cost_status, '')) IN
                                ('actual', 'final', 'settled', 'complete')
                                AND actual_cost_usd IS NOT NULL THEN actual_cost_usd
                           ELSE COALESCE(estimated_cost_usd, '0')
                       END AS REAL)), 0)
                FROM session_model_usage
            """).fetchone()
            # Hermes can count main calls on sessions without a model-usage row.
            session_columns = {row[1] for row in source.execute("PRAGMA table_info(sessions)")}
            usage_columns = {row[1] for row in source.execute("PRAGMA table_info(session_model_usage)")}
            if {"id", "api_call_count"} <= session_columns and {"session_id", "task"} <= usage_columns:
                residual = source.execute("""
                    SELECT COALESCE(SUM(MAX(0, s.api_call_count -
                        COALESCE((SELECT SUM(u.api_call_count)
                                  FROM session_model_usage u
                                  WHERE u.session_id = s.id AND u.task = ''), 0))), 0)
                    FROM sessions s
                """).fetchone()[0]
                totals = (totals[0] + residual, *totals[1:])
            return totals
    except sqlite3.Error:
        return None


def _initialize(home):
    home.mkdir(parents=True, exist_ok=True)
    path = home / LEDGER_NAME
    with closing(sqlite3.connect(path, timeout=5)) as conn, conn:
        conn.execute("PRAGMA busy_timeout=5000")
        conn.execute("""CREATE TABLE IF NOT EXISTS request_events (
            event_id TEXT PRIMARY KEY, kind TEXT NOT NULL, session_id TEXT NOT NULL,
            task_id TEXT NOT NULL, aux_task TEXT NOT NULL, model TEXT NOT NULL,
            provider TEXT NOT NULL, started_at_ms INTEGER NOT NULL,
            ended_at_ms INTEGER NOT NULL, status TEXT NOT NULL,
            status_code INTEGER, duration_ms INTEGER,
            usage_available INTEGER NOT NULL, input_tokens INTEGER, output_tokens INTEGER,
            cache_read_tokens INTEGER, cache_write_tokens INTEGER,
            reasoning_tokens INTEGER
        )""")
        conn.execute("""CREATE TABLE IF NOT EXISTS historical_baseline (
            id INTEGER PRIMARY KEY CHECK (id = 1), ledger_id TEXT NOT NULL,
            captured_at_ms INTEGER NOT NULL,
            available INTEGER NOT NULL, request_count INTEGER, input_tokens INTEGER,
            output_tokens INTEGER, cache_read_tokens INTEGER, cache_write_tokens INTEGER,
            reasoning_tokens INTEGER, cost_usd TEXT
        )""")
        exists = conn.execute("SELECT 1 FROM historical_baseline WHERE id=1").fetchone()
        if not exists:
            baseline = _baseline(home)
            conn.execute("""INSERT OR IGNORE INTO historical_baseline VALUES
                (1, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (uuid.uuid4().hex, _milliseconds(time.time()), int(baseline is not None),
                 *(baseline[:6] if baseline else (None,) * 6),
                 str(baseline[6]) if baseline else None))


def make_collector(home):
    """Initialize one profile ledger and return the three hook callbacks' core."""
    home = Path(home)
    _initialize(home)
    path = home / LEDGER_NAME

    def collect(kind, **payload):
        request_id = str(payload.get("api_request_id") or "").strip()
        # Missing stable IDs cannot be safely deduplicated; leave a coverage gap.
        if not request_id:
            return
        category = "aux" if kind == "post_auxiliary_call" else "main"
        usage = payload.get("usage")
        valid_usage = isinstance(usage, dict) and not payload.get("streaming")
        start = _milliseconds(payload.get("started_at"))
        end = _milliseconds(payload.get("ended_at"))
        if start is None or end is None or end < start:
            return
        status = "error" if kind == "api_request_error" or payload.get("error") else "success"
        if category == "main":
            suffix = (f"error:{_counter(payload.get('retry_count')) or 0}"
                      if status == "error" else "success")
        else:
            suffix = str(_counter(payload.get("retry_count")) or 0)
        event_id = f"{category}:{request_id}:{suffix}"
        if payload.get("streaming") and status == "success":
            status = "unknown"
        # Do not persist arbitrary provider error strings or response content.
        buckets = ("input_tokens", "output_tokens", "cache_read_tokens",
                   "cache_write_tokens", "reasoning_tokens")
        values = tuple(_counter(usage.get(name)) if valid_usage else None for name in buckets)
        with closing(sqlite3.connect(path, timeout=5)) as conn, conn:
            conn.execute("PRAGMA busy_timeout=5000")
            conn.execute("""INSERT OR IGNORE INTO request_events VALUES
                (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (event_id, category,
                 str(payload.get("session_id") or "")[:256],
                 str(payload.get("task_id") or "")[:256],
                 str(payload.get("aux_task") or "")[:128],
                 str(payload.get("response_model") or payload.get("model") or "")[:256],
                 str(payload.get("provider") or "")[:128], start, end, status,
                 _counter(payload.get("status_code")), end - start,
                 int(valid_usage), *values))

    return collect


def register(ctx):
    from hermes_constants import get_hermes_home
    from hermes_cli.plugins import VALID_HOOKS

    missing = [name for name in REQUIRED_HOOKS if name not in VALID_HOOKS]
    if missing:
        raise RuntimeError(
            "CC Switch request capture requires Hermes 0.21.5 (v2026.9.24) or newer; "
            "upgrade Hermes before enabling capture. Missing hooks: " + ", ".join(missing)
        )

    collect = make_collector(get_hermes_home())
    for name in REQUIRED_HOOKS:
        def callback(_name=name, **payload):
            collect(_name, **payload)
        ctx.register_hook(name, callback)
