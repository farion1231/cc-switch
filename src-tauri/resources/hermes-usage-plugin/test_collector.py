import importlib.util
import sqlite3
import sys
import tempfile
import types
import unittest
from contextlib import closing
from pathlib import Path
from unittest.mock import patch


PLUGIN = Path(__file__).with_name("__init__.py")


def load_plugin():
    spec = importlib.util.spec_from_file_location("ccswitch_hermes_usage", PLUGIN)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class CollectorTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name)

    def test_exact_event_dedup_and_private_payload_is_not_saved(self):
        plugin = load_plugin()
        callback = plugin.make_collector(self.home)
        payload = dict(
            api_request_id="request-1", session_id="session-1", task_id="task-1",
            model="model-a", provider="provider-a", started_at=100.0,
            ended_at=101.25, api_duration=1.25,
            usage={"input_tokens": 4, "output_tokens": 5,
                   "cache_read_tokens": 2, "cache_write_tokens": 1,
                   "reasoning_tokens": 3},
            request={"body": {"messages": ["secret prompt"]}},
            response={"assistant_message": {"content": "secret response"}},
            base_url="https://private.example/api", api_key="secret-key",
        )
        callback("post_api_request", **payload)
        callback("post_api_request", **payload)
        ledger = self.home / "ccswitch-usage.sqlite"
        with closing(sqlite3.connect(ledger)) as conn:
            row = conn.execute("SELECT event_id, started_at_ms, ended_at_ms, input_tokens, output_tokens, status FROM request_events").fetchone()
            self.assertEqual(row, ("main:request-1:success", 100000, 101250, 4, 5, "success"))
            self.assertEqual(conn.execute("SELECT COUNT(*) FROM request_events").fetchone()[0], 1)
            self.assertEqual(conn.execute("SELECT available FROM historical_baseline").fetchone()[0], 0)
        data = ledger.read_bytes()
        for secret in (b"secret prompt", b"secret response", b"secret-key", b"private.example"):
            self.assertNotIn(secret, data)

    def test_baseline_is_frozen_and_missing_usage_stays_unknown(self):
        with closing(sqlite3.connect(self.home / "state.db")) as conn, conn:
            conn.execute("CREATE TABLE session_model_usage (api_call_count INTEGER, input_tokens INTEGER, output_tokens INTEGER, cache_read_tokens INTEGER, cache_write_tokens INTEGER, reasoning_tokens INTEGER, estimated_cost_usd TEXT, actual_cost_usd TEXT, cost_status TEXT)")
            conn.execute("INSERT INTO session_model_usage VALUES (2, 10, 20, 3, 4, 5, '0.12', NULL, 'estimated')")
        plugin = load_plugin()
        callback = plugin.make_collector(self.home)
        callback("post_auxiliary_call", api_request_id="aux-1", aux_task="title", started_at=110, ended_at=111, streaming=True)
        with closing(sqlite3.connect(self.home / "state.db")) as conn, conn:
            conn.execute("UPDATE session_model_usage SET api_call_count=20")
        plugin.make_collector(self.home)
        with closing(sqlite3.connect(self.home / "ccswitch-usage.sqlite")) as conn:
            baseline = conn.execute("SELECT request_count, input_tokens, cost_usd, available FROM historical_baseline").fetchone()
            event = conn.execute("SELECT input_tokens, output_tokens, usage_available FROM request_events").fetchone()
        self.assertEqual(baseline, (2, 10, "0.12", 1))
        self.assertEqual(event, (None, None, 0))

    def test_baseline_includes_unattributed_main_calls_and_has_stable_ledger_id(self):
        with closing(sqlite3.connect(self.home / "state.db")) as conn, conn:
            conn.execute("CREATE TABLE sessions (id TEXT, api_call_count INTEGER)")
            conn.execute("CREATE TABLE session_model_usage (session_id TEXT, task TEXT, api_call_count INTEGER, input_tokens INTEGER, output_tokens INTEGER, cache_read_tokens INTEGER, cache_write_tokens INTEGER, reasoning_tokens INTEGER, estimated_cost_usd TEXT, actual_cost_usd TEXT, cost_status TEXT)")
            conn.execute("INSERT INTO sessions VALUES ('s', 5)")
            conn.execute("INSERT INTO session_model_usage VALUES ('s', '', 2, 10, 20, 0, 0, 0, '0', NULL, 'estimated')")
        plugin = load_plugin()
        plugin.make_collector(self.home)
        with closing(sqlite3.connect(self.home / "ccswitch-usage.sqlite")) as conn:
            first = conn.execute("SELECT ledger_id, request_count FROM historical_baseline").fetchone()
        plugin.make_collector(self.home)
        with closing(sqlite3.connect(self.home / "ccswitch-usage.sqlite")) as conn:
            second = conn.execute("SELECT ledger_id, request_count FROM historical_baseline").fetchone()
        self.assertEqual(first, second)
        self.assertEqual(first[1], 5)
        self.assertEqual(len(first[0]), 32)

    def test_retry_error_and_later_success_are_distinct_attempts(self):
        plugin = load_plugin()
        callback = plugin.make_collector(self.home)
        base = dict(api_request_id="same-id", started_at=100, ended_at=101,
                    provider="p", model="m")
        callback("api_request_error", **base, retry_count=0, status_code=429)
        callback("api_request_error", **base, retry_count=1, status_code=429)
        callback("post_api_request", **base, usage={"input_tokens": 3})
        with closing(sqlite3.connect(self.home / "ccswitch-usage.sqlite")) as conn:
            ids = [row[0] for row in conn.execute("SELECT event_id FROM request_events ORDER BY event_id")]
        self.assertEqual(ids, ["main:same-id:error:0", "main:same-id:error:1",
                               "main:same-id:success"])

    def test_register_binds_all_supported_hooks_to_profile_home(self):
        plugin = load_plugin()
        hooks = {}
        context = types.SimpleNamespace(register_hook=lambda name, fn: hooks.setdefault(name, fn))
        constants = types.SimpleNamespace(get_hermes_home=lambda: self.home)
        with patch.dict(sys.modules, {"hermes_constants": constants}):
            plugin.register(context)
        self.assertEqual(set(hooks), {"post_api_request", "api_request_error",
                                      "post_auxiliary_call"})
        hooks["post_api_request"](api_request_id="r", started_at=10, ended_at=11)
        with closing(sqlite3.connect(self.home / "ccswitch-usage.sqlite")) as conn:
            count = conn.execute("SELECT COUNT(*) FROM request_events").fetchone()[0]
        self.assertEqual(count, 1)


if __name__ == "__main__":
    unittest.main()
