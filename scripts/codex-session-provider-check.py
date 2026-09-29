#!/usr/bin/env python3
"""Check whether every model_provider referenced by Codex session history still
has a table in ~/.codex/config.toml.

Why this exists
---------------
Codex 0.149+ refuses to load a session whose ``model_provider`` has no matching
``[model_providers.<id>]`` table, and session metadata is immutable. cc-switch
rewrites ``config.toml`` on every provider switch, so a switch can silently
delete the table a live session depends on and the only symptom is
``Model provider 'X' not found`` when the user opens that thread.

The check is the same invariant cc-switch now enforces in
``src-tauri/src/codex_session_providers.rs``: the set of ids to protect must be
read from the session history itself, not from any list of "known" ids.

Usage
-----
    python3 scripts/codex-session-provider-check.py            # report, exit 1 if broken
    python3 scripts/codex-session-provider-check.py --json     # machine readable

Exit codes: 0 = every referenced id resolves; 1 = at least one id has no table.
"""

from __future__ import annotations

import argparse
import json
import os
import sqlite3
import sys
from pathlib import Path

# Codex writes session_meta as the first record; a few leading lines are probed
# so a truncated file degrades to "not found" instead of a full-file scan.
SESSION_META_PROBE_LINES = 8


def codex_dir() -> Path:
    return Path(os.environ.get("CODEX_HOME") or Path.home() / ".codex")


def collect_from_rollouts(root: Path) -> dict[str, int]:
    """Provider id -> rollout file count, from each file's session_meta record."""
    found: dict[str, int] = {}
    for sub in ("sessions", "archived_sessions"):
        base = root / sub
        if not base.is_dir():
            continue
        for path in base.rglob("*.jsonl"):
            provider = _first_session_meta_provider(path)
            if provider:
                found[provider] = found.get(provider, 0) + 1
    return found


def _first_session_meta_provider(path: Path) -> str | None:
    try:
        with path.open("r", errors="replace") as handle:
            for _ in range(SESSION_META_PROBE_LINES):
                line = handle.readline()
                if not line:
                    return None
                line = line.strip()
                if not line:
                    continue
                try:
                    record = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if record.get("type") != "session_meta":
                    continue
                provider = (record.get("payload") or {}).get("model_provider")
                if isinstance(provider, str) and provider.strip():
                    return provider.strip()
    except OSError:
        return None
    return None


def collect_from_state_dbs(root: Path) -> dict[str, int]:
    """Provider id -> thread row count, from Codex's per-thread state DB.

    Read-only on purpose: the state DB belongs to the running Codex app.
    """
    found: dict[str, int] = {}
    for path in _state_db_candidates(root):
        if not path.is_file():
            continue
        try:
            # file:...?mode=ro keeps SQLite from creating a WAL or taking a
            # write lock on a database the app is using.
            uri = f"file:{path}?mode=ro"
            conn = sqlite3.connect(uri, uri=True, timeout=0.5)
        except sqlite3.Error:
            continue
        try:
            rows = conn.execute(
                "SELECT model_provider, COUNT(*) FROM threads "
                "WHERE model_provider IS NOT NULL AND TRIM(model_provider) <> '' "
                "GROUP BY model_provider"
            ).fetchall()
        except sqlite3.Error:
            continue
        finally:
            conn.close()
        for provider, count in rows:
            found[provider.strip()] = found.get(provider.strip(), 0) + count
    return found


def _state_db_candidates(root: Path) -> list[Path]:
    paths = [root / "state_5.sqlite"]
    # Codex can move SQLite state out of CODEX_HOME.
    override = os.environ.get("CODEX_SQLITE_HOME")
    if override:
        paths.append(Path(override) / "state_5.sqlite")
    try:
        import tomllib

        with (root / "config.toml").open("rb") as handle:
            configured = tomllib.load(handle).get("sqlite_home")
        if isinstance(configured, str) and configured.strip():
            paths.append(Path(configured.strip()).expanduser() / "state_5.sqlite")
    except (OSError, ValueError):
        pass
    seen, unique = set(), []
    for path in paths:
        if path not in seen:
            seen.add(path)
            unique.append(path)
    return unique


def defined_provider_ids(config_path: Path) -> tuple[set[str], str | None]:
    """Ids defined as [model_providers.*] in config.toml, plus the active route."""
    try:
        import tomllib

        with config_path.open("rb") as handle:
            doc = tomllib.load(handle)
    except FileNotFoundError:
        return set(), f"{config_path} 不存在"
    except Exception as exc:  # malformed TOML is itself a finding
        return set(), f"{config_path} 解析失败: {exc}"
    return set((doc.get("model_providers") or {}).keys()), doc.get("model_provider")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--json", action="store_true", help="machine readable output")
    args = parser.parse_args()

    root = codex_dir()
    config_path = root / "config.toml"

    rollouts = collect_from_rollouts(root)
    state_rows = collect_from_state_dbs(root)
    defined, active = defined_provider_ids(config_path)

    referenced: dict[str, dict[str, int]] = {}
    for source, counts in (("rollout", rollouts), ("state_db", state_rows)):
        for provider, count in counts.items():
            referenced.setdefault(provider, {})[source] = count

    # An id is resolved either by a table or by being Codex's built-in route.
    builtin = {"openai", "oss", "azure"}
    unresolved = {
        provider: counts
        for provider, counts in referenced.items()
        if provider not in defined and provider not in builtin
    }

    if args.json:
        print(
            json.dumps(
                {
                    "codex_dir": str(root),
                    "active_model_provider": active,
                    "defined_provider_ids": sorted(defined),
                    "referenced": referenced,
                    "unresolved": unresolved,
                },
                ensure_ascii=False,
                indent=2,
            )
        )
        return 1 if unresolved else 0

    print(f"Codex 配置目录: {root}")
    print(f"顶层 model_provider: {active or '(未设置)'}")
    print(f"已定义表 [model_providers.*]: {', '.join(sorted(defined)) or '(无)'}")
    print()
    print(f"{'provider id':<24} {'rollout':>8} {'state_db':>9}  状态")
    for provider in sorted(referenced):
        counts = referenced[provider]
        if provider in unresolved:
            status = "缺表 → 这些 session 打不开"
        elif provider == active:
            status = "当前路由"
        else:
            status = "OK"
        print(
            f"{provider:<24} {counts.get('rollout', 0):>8} {counts.get('state_db', 0):>9}  {status}"
        )

    if unresolved:
        print()
        print("缺表的 provider id（按 session 数量排序）:")
        for provider, counts in sorted(
            unresolved.items(), key=lambda kv: -(sum(kv[1].values()))
        ):
            print(f"  - {provider}: {counts}")
        print()
        print("这些对话串会报 `Model provider '<id>' not found`。")
        print("修法：把对应 id 作为别名表补进 config.toml（inert，不影响新流量），")
        print("或在退出 Codex 后改写历史里的 id 为当前 provider。")
        return 1

    print()
    print("OK: 所有被 session 引用的 provider id 都有表。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
