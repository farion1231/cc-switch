#!/usr/bin/env python3
"""
Codex Desktop history visibility repair tool.

This script intentionally uses only the Python standard library so it can run
on Windows, macOS, and Linux without packaging a helper executable.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import platform
import shutil
import sqlite3
import subprocess
import sys
import time
from collections import defaultdict, deque
from dataclasses import dataclass
from pathlib import Path
from typing import Any


DEFAULT_SOURCE_PROVIDERS = [
    "openai",
    "custom",
    "ccswitch",
    "codex_model_router_v2",
    "cc_switch_codex_router",
    "codex_model_router",
]
DEFAULT_TARGET_PROVIDER = "custom"
DEFAULT_INTERACTIVE_SOURCES = {"cli", "vscode"}
VERSION = "0.1.0"


@dataclass
class ActiveStateDb:
    """记录当前 Codex Desktop 实际使用的 SQLite 路径和来源类型。"""

    path: Path
    kind: str


@dataclass
class HistoryRow:
    """承载一条 threads 表记录中修复与展示需要的字段。"""

    id: str
    title: str
    cwd: str | None
    rollout_path: str | None
    model_provider: str | None
    source: str | None
    thread_source: str | None
    archived: int
    has_user_event: int
    updated_at: int
    updated_at_ms: int
    preview: str | None = None
    first_user_message: str | None = None


@dataclass
class FocusRow:
    """记录需要推入 Desktop recent window 的会话及其新时间戳。"""

    id: str
    title: str
    cwd: str | None
    rollout_path: str | None
    updated_at: int
    updated_at_ms: int
    updated_iso: str


def strip_long_prefix(value: str | None) -> str | None:
    """去掉 Windows long-path 前缀，避免路径比较和 pathlib 打开失败。"""

    if not value:
        return value
    if value.startswith("\\\\?\\UNC\\"):
        return "\\\\" + value[8:]
    if value.startswith("\\\\?\\"):
        return value[4:]
    return value


def normalize_path(value: str | None) -> str | None:
    """把 cwd 规范化为稳定的比较形式，不修改 SQLite 中的原始值。"""

    text = strip_long_prefix(str(value).strip()) if value is not None else ""
    if not text:
        return None
    text = text.replace("/", "\\").rstrip("\\")
    if len(text) >= 2 and text[1] == ":":
        text = text[0].upper() + text[1:]
    return text


def resolve_user_path(raw: str | None) -> Path | None:
    """解析用户输入路径，支持 ~ 并去掉 Windows long-path 前缀。"""

    if raw is None or not str(raw).strip():
        return None
    text = strip_long_prefix(str(raw).strip()) or ""
    return Path(text).expanduser()


def default_codex_home() -> Path:
    """返回当前用户的默认 Codex home 目录。"""

    return Path.home() / ".codex"


def parse_top_level_model_provider(config_text: str) -> str | None:
    """读取 config.toml 顶层 model_provider，遇到表头后停止。"""

    for line in config_text.splitlines():
        stripped = line.strip()
        if stripped.startswith("["):
            break
        if not stripped.startswith("model_provider"):
            continue
        _, _, rhs = stripped.partition("=")
        value = rhs.strip().strip("\"'")
        return value or None
    return None


def parse_sqlite_home(config_text: str) -> Path | None:
    """读取 config.toml 顶层 sqlite_home，用于兼容非默认 Codex SQLite 目录。"""

    for line in config_text.splitlines():
        stripped = line.strip()
        if stripped.startswith("["):
            break
        if not stripped.startswith("sqlite_home"):
            continue
        _, _, rhs = stripped.partition("=")
        value = rhs.strip().strip("\"'")
        return resolve_user_path(value)
    return None


def resolve_active_state_db(
    codex_home: Path,
    config_text: str,
    explicit_state_db: str | None,
) -> ActiveStateDb:
    """解析 active state_5.sqlite，优先使用 Codex Desktop 26.609 的 sqlite 子目录。"""

    explicit = resolve_user_path(explicit_state_db)
    if explicit is not None:
        if not explicit.exists():
            raise FileNotFoundError(f"state DB not found: {explicit}")
        return ActiveStateDb(explicit, "explicit")

    sqlite_default = codex_home / "sqlite" / "state_5.sqlite"
    if sqlite_default.exists():
        return ActiveStateDb(sqlite_default, "sqlite_subdir")

    sqlite_home = parse_sqlite_home(config_text)
    if sqlite_home is not None and (sqlite_home / "state_5.sqlite").exists():
        return ActiveStateDb(sqlite_home / "state_5.sqlite", "configured_sqlite_home")

    legacy = codex_home / "state_5.sqlite"
    if legacy.exists():
        return ActiveStateDb(legacy, "legacy_root")

    raise FileNotFoundError(f"state_5.sqlite not found under {codex_home}")


def table_columns(con: sqlite3.Connection, table: str) -> set[str]:
    """读取 SQLite 表字段，用于兼容 Codex schema 演进。"""

    return {row[1] for row in con.execute(f"PRAGMA table_info({table})")}


def get_value(row: sqlite3.Row, key: str, default: Any = None) -> Any:
    """安全读取 sqlite3.Row 字段，缺列时返回默认值。"""

    return row[key] if key in row.keys() else default


def row_title(row: sqlite3.Row) -> str:
    """按 Codex 展示习惯从 title、preview、first_user_message 中提取标题。"""

    for key in ("title", "preview", "first_user_message"):
        value = get_value(row, key)
        if value and str(value).strip():
            return str(value).strip()
    return "Untitled"


def row_updated_ms(row: sqlite3.Row | HistoryRow) -> int:
    """读取更新时间毫秒值，缺失时退回秒级字段。"""

    if isinstance(row, HistoryRow):
        return int(row.updated_at_ms or row.updated_at * 1000 or 0)
    value = get_value(row, "updated_at_ms")
    if value is not None:
        return int(value)
    return int(get_value(row, "updated_at", 0) or 0) * 1000


def load_history_rows(con: sqlite3.Connection) -> list[HistoryRow]:
    """从 threads 表读取历史记录，转换成稳定的 Python 数据结构。"""

    if "threads" not in {
        row[0] for row in con.execute("SELECT name FROM sqlite_master WHERE type='table'")
    }:
        raise RuntimeError("threads table not found")
    columns = table_columns(con, "threads")
    if not {"id", "model_provider"} <= columns:
        raise RuntimeError("threads table missing required id/model_provider columns")
    rows = []
    for row in con.execute("SELECT * FROM threads"):
        updated_ms = row_updated_ms(row)
        rows.append(
            HistoryRow(
                id=str(get_value(row, "id", "")),
                title=row_title(row),
                cwd=get_value(row, "cwd"),
                rollout_path=get_value(row, "rollout_path"),
                model_provider=get_value(row, "model_provider"),
                source=get_value(row, "source"),
                thread_source=get_value(row, "thread_source"),
                archived=int(get_value(row, "archived", 0) or 0),
                has_user_event=int(get_value(row, "has_user_event", 0) or 0),
                updated_at=int(get_value(row, "updated_at", updated_ms // 1000) or 0),
                updated_at_ms=updated_ms,
                preview=get_value(row, "preview"),
                first_user_message=get_value(row, "first_user_message"),
            )
        )
    return rows


def iso_from_ms(ms: int) -> str:
    """把 epoch milliseconds 转成 session_index.jsonl 使用的 UTC ISO 字符串。"""

    return (
        dt.datetime.fromtimestamp(ms / 1000, tz=dt.timezone.utc)
        .isoformat(timespec="milliseconds")
        .replace("+00:00", "Z")
    )


def now_stamp() -> str:
    """生成本地备份目录时间戳。"""

    return dt.datetime.now().strftime("%Y%m%d_%H%M%S")


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    """读取 JSONL；无法解析的行以 __raw_line__ 保留，避免写回时丢数据。"""

    rows: list[dict[str, Any]] = []
    if not path.exists():
        return rows
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        try:
            value = json.loads(line)
            rows.append(value if isinstance(value, dict) else {"__raw_line__": line})
        except Exception:
            rows.append({"__raw_line__": line})
    return rows


def write_jsonl(path: Path, rows: list[dict[str, Any]]) -> None:
    """写回 JSONL，同时保留此前无法解析的原始行。"""

    lines = []
    for row in rows:
        raw = row.get("__raw_line__") if isinstance(row, dict) else None
        lines.append(raw if raw is not None else json.dumps(row, ensure_ascii=False, separators=(",", ":")))
    path.write_text("\n".join(lines) + ("\n" if lines else ""), encoding="utf-8", newline="\n")


def read_json_object(path: Path) -> dict[str, Any]:
    """读取 JSON 对象；缺失或损坏时返回空对象以便继续修复其它项。"""

    if not path.exists():
        return {}
    try:
        value = json.loads(path.read_text(encoding="utf-8", errors="replace"))
        return value if isinstance(value, dict) else {}
    except Exception:
        return {}


def write_json_object(path: Path, value: dict[str, Any]) -> None:
    """以 UTF-8 pretty JSON 写回 Codex global state。"""

    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")


def rollout_path(row: HistoryRow) -> Path | None:
    """把一条历史记录的 rollout_path 转成可访问路径。"""

    if not row.rollout_path:
        return None
    path = Path(strip_long_prefix(row.rollout_path) or "")
    return path if path.exists() else None


def rollout_has_user_event(path: Path | None) -> bool:
    """扫描 rollout 是否包含真实用户消息，用于回填 has_user_event。"""

    if path is None:
        return False
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return False
    return (
        '"type":"user_message"' in text
        or '"role":"user"' in text
        or '"user_input"' in text
    )


def rewrite_rollout_provider(path: Path, target_provider: str) -> tuple[bool, str, float]:
    """重写 rollout 中所有 payload.model_provider，并返回是否变更、文本和旧 mtime。"""

    text = path.read_text(encoding="utf-8", errors="replace")
    old_mtime = path.stat().st_mtime
    lines = []
    changed = False
    for line in text.splitlines():
        try:
            value = json.loads(line)
        except Exception:
            lines.append(line)
            continue
        payload = value.get("payload") if isinstance(value, dict) else None
        if not isinstance(payload, dict) or "model_provider" not in payload:
            lines.append(line)
            continue
        if payload.get("model_provider") != target_provider:
            payload["model_provider"] = target_provider
            changed = True
        lines.append(json.dumps(value, ensure_ascii=False, separators=(",", ":")))
    return changed, "\n".join(lines) + ("\n" if text.endswith("\n") or lines else ""), old_mtime


def is_user_thread(row: HistoryRow, include_subagents: bool) -> bool:
    """判断 thread_source 是否属于用户主线程。"""

    return include_subagents or row.thread_source in (None, "", "user")


def source_matches(row: HistoryRow, source_filter: str | None) -> bool:
    """判断 source 是否匹配筛选条件；未指定时使用 Codex 常规交互来源。"""

    if source_filter:
        return row.source == source_filter
    return row.source in DEFAULT_INTERACTIVE_SOURCES


def visible_text(row: HistoryRow) -> bool:
    """判断一条历史记录是否有可显示文本。"""

    return any((value or "").strip() for value in (row.title, row.preview, row.first_user_message))


def filter_visible_rows(
    rows: list[HistoryRow],
    target_provider: str,
    provider_update_ids: set[str],
    include_archived: bool,
    include_subagents: bool,
    source_filter: str | None,
) -> tuple[list[HistoryRow], list[str]]:
    """计算修复后可见行，并找出需要回填 has_user_event 的行。"""

    visible_rows: list[HistoryRow] = []
    user_event_updates: list[str] = []
    for row in rows:
        provider_after = target_provider if row.id in provider_update_ids else row.model_provider
        if provider_after != target_provider:
            continue
        if not include_archived and row.archived != 0:
            continue
        if not source_matches(row, source_filter):
            continue
        if not is_user_thread(row, include_subagents):
            continue
        has_user = row.has_user_event == 1 or rollout_has_user_event(rollout_path(row))
        if row.has_user_event != 1 and has_user:
            user_event_updates.append(row.id)
        if has_user and visible_text(row):
            visible_rows.append(row)
    return visible_rows, user_event_updates


def select_balanced_rows(
    visible_rows: list[HistoryRow],
    project_path: str | None,
    focus_count: int,
    max_per_project: int,
    max_total: int,
) -> list[FocusRow]:
    """先保证当前项目数量，再按项目 round-robin 填充最近窗口。"""

    sorted_rows = sorted(visible_rows, key=lambda row: (row_updated_ms(row), row.id), reverse=True)
    selected: list[HistoryRow] = []
    selected_ids: set[str] = set()
    if project_path:
        for row in sorted_rows:
            if len(selected) >= focus_count or len(selected) >= max_total:
                break
            if normalize_path(row.cwd) == project_path:
                selected.append(row)
                selected_ids.add(row.id)

    buckets: dict[str, deque[HistoryRow]] = defaultdict(deque)
    for row in sorted_rows:
        if row.id in selected_ids:
            continue
        key = normalize_path(row.cwd) or "(no cwd)"
        if len(buckets[key]) < max_per_project:
            buckets[key].append(row)

    projects = sorted(
        buckets.keys(),
        key=lambda key: row_updated_ms(buckets[key][0]) if buckets[key] else 0,
        reverse=True,
    )
    while len(selected) < max_total and any(buckets.values()):
        progressed = False
        for project in projects:
            if len(selected) >= max_total:
                break
            if buckets[project]:
                row = buckets[project].popleft()
                if row.id not in selected_ids:
                    selected.append(row)
                    selected_ids.add(row.id)
                progressed = True
        if not progressed:
            break
    return [focus_from_row(row) for row in selected]


def focus_from_row(row: HistoryRow) -> FocusRow:
    """把历史行转换为可更新时间戳和 session_index 的 focus 行。"""

    ms = row_updated_ms(row)
    return FocusRow(
        id=row.id,
        title=row.title,
        cwd=normalize_path(row.cwd),
        rollout_path=row.rollout_path,
        updated_at=ms // 1000,
        updated_at_ms=ms,
        updated_iso=iso_from_ms(ms),
    )


def assign_focus_times(rows: list[FocusRow]) -> None:
    """给 focus 行分配新的递减时间戳，使其进入 Desktop 最近窗口。"""

    if not rows:
        return
    base = max(int(time.time() * 1000), max(row.updated_at_ms for row in rows)) + 10_000
    total = len(rows)
    for index, row in enumerate(rows):
        ms = base + (total - index) * 250
        row.updated_at_ms = ms
        row.updated_at = ms // 1000
        row.updated_iso = iso_from_ms(ms)


def choose_focus_rows(
    visible_rows: list[HistoryRow],
    session_ids: list[str],
    project_path: str | None,
    count: int,
    balance_recent_window: bool,
    max_per_project: int,
    max_total: int,
) -> list[FocusRow]:
    """按 sessionIds、项目 focus 或 balanced recent window 选择待恢复会话。"""

    by_id = {row.id: row for row in visible_rows}
    if session_ids:
        selected = [focus_from_row(by_id[thread_id]) for thread_id in session_ids if thread_id in by_id]
        assign_focus_times(selected)
        return selected
    if balance_recent_window:
        selected = select_balanced_rows(visible_rows, project_path, count, max_per_project, max_total)
        assign_focus_times(selected)
        return selected
    project_rows = [
        row for row in visible_rows if project_path and normalize_path(row.cwd) == project_path
    ]
    project_rows.sort(key=lambda row: (row_updated_ms(row), row.id), reverse=True)
    selected = [focus_from_row(row) for row in project_rows[:count]]
    assign_focus_times(selected)
    return selected


def backup_state(codex_home: Path, state_db: Path, backup_dir: Path, rollout_paths: list[Path]) -> None:
    """备份 active SQLite、WAL/SHM、session_index、global state 和待写 rollout。"""

    backup_dir.mkdir(parents=True, exist_ok=False)
    db_dir = backup_dir / ("sqlite" if state_db.parent.name.lower() == "sqlite" else "legacy-root")
    db_dir.mkdir(parents=True, exist_ok=True)
    for suffix in ("", "-wal", "-shm"):
        source = Path(str(state_db) + suffix)
        if source.exists():
            shutil.copy2(source, db_dir / source.name)
    for name in ("session_index.jsonl", ".codex-global-state.json"):
        source = codex_home / name
        if source.exists():
            shutil.copy2(source, backup_dir / name)
    rollout_dir = backup_dir / "rollouts"
    rollout_dir.mkdir(parents=True, exist_ok=True)
    manifest = []
    for index, source in enumerate(rollout_paths):
        if source.exists():
            target = rollout_dir / f"{index:04d}-{source.name}"
            shutil.copy2(source, target)
            manifest.append({"source": str(source), "backup": str(target)})
    (backup_dir / "rollout-manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )


def existing_index_ids(index_path: Path) -> set[str]:
    """读取 session_index 中已有 thread id。"""

    return {row.get("id") for row in read_jsonl(index_path) if isinstance(row.get("id"), str)}


def append_missing_index(index_path: Path, missing_rows: list[HistoryRow]) -> int:
    """把缺失的可见会话追加到 session_index.jsonl。"""

    if not missing_rows:
        return 0
    rows = read_jsonl(index_path)
    ids = {row.get("id") for row in rows if isinstance(row.get("id"), str)}
    appended = 0
    for item in missing_rows:
        if item.id in ids:
            continue
        rows.append({"id": item.id, "thread_name": item.title, "updated_at": iso_from_ms(row_updated_ms(item))})
        ids.add(item.id)
        appended += 1
    write_jsonl(index_path, rows)
    return appended


def move_focus_index(index_path: Path, focus_rows: list[FocusRow]) -> tuple[int, int]:
    """把选中的会话移到 session_index 尾部，并更新标题和时间。"""

    if not focus_rows:
        return 0, 0
    rows = read_jsonl(index_path)
    selected = {row.id: row for row in focus_rows}
    seen: set[str] = set()
    kept: list[dict[str, Any]] = []
    moved_by_id: dict[str, dict[str, Any]] = {}
    titles_updated = 0
    for row in rows:
        thread_id = row.get("id")
        if isinstance(thread_id, str):
            seen.add(thread_id)
        if isinstance(thread_id, str) and thread_id in selected:
            next_row = dict(row)
            old_title = str(next_row.get("thread_name") or "").strip()
            next_row["updated_at"] = selected[thread_id].updated_iso
            if selected[thread_id].title.strip() and old_title != selected[thread_id].title.strip():
                next_row["thread_name"] = selected[thread_id].title
                titles_updated += 1
            moved_by_id[thread_id] = next_row
        else:
            kept.append(row)
    moved = []
    for focus in focus_rows:
        moved.append(
            moved_by_id.get(
                focus.id,
                {"id": focus.id, "thread_name": focus.title, "updated_at": focus.updated_iso},
            )
        )
    write_jsonl(index_path, kept + moved)
    return len(moved), titles_updated


def update_global_state(
    path: Path,
    visible_rows: list[HistoryRow],
    focus_rows: list[FocusRow],
    project_path: str | None,
    apply: bool,
) -> tuple[int, int, int]:
    """修复 workspace hints、projectless ids 和 saved roots。"""

    state = read_json_object(path)
    hints = state.get("thread-workspace-root-hints")
    if not isinstance(hints, dict):
        hints = {}
        state["thread-workspace-root-hints"] = hints
    expected: dict[str, str] = {}
    for row in visible_rows:
        cwd = normalize_path(row.cwd)
        if cwd:
            expected[row.id] = cwd
    for row in focus_rows:
        cwd = project_path or row.cwd
        if cwd:
            expected[row.id] = cwd

    hints_to_fix = sum(1 for key, value in expected.items() if hints.get(key) != value)
    ids = set(expected.keys())
    projectless = state.get("projectless-thread-ids")
    projectless_remove = 0
    if isinstance(projectless, list):
        projectless_remove = sum(1 for item in projectless if item in ids)
    roots_to_add = 0
    if project_path:
        roots = state.get("electron-saved-workspace-roots")
        if not isinstance(roots, list) or project_path not in roots:
            roots_to_add = 1

    if not apply or not (hints_to_fix or projectless_remove or roots_to_add):
        return hints_to_fix, projectless_remove, roots_to_add

    for key, value in expected.items():
        hints[key] = value
    if isinstance(projectless, list):
        state["projectless-thread-ids"] = [item for item in projectless if item not in ids]
    if project_path:
        roots = state.get("electron-saved-workspace-roots")
        if not isinstance(roots, list):
            roots = []
        if project_path not in roots:
            roots.append(project_path)
        state["electron-saved-workspace-roots"] = roots
    write_json_object(path, state)
    return hints_to_fix, projectless_remove, roots_to_add


def touch_rollout_mtimes(focus_rows: list[FocusRow]) -> int:
    """把选中 rollout 的 mtime 对齐到新 updated_at_ms。"""

    touched = 0
    for row in focus_rows:
        raw = strip_long_prefix(row.rollout_path)
        if not raw:
            continue
        path = Path(raw)
        if not path.exists():
            continue
        stat = path.stat()
        os.utime(path, (stat.st_atime, row.updated_at_ms / 1000))
        touched += 1
    return touched


def detect_running_codex_processes() -> list[str]:
    """尽量检测正在运行的 Codex Desktop/app-server，写入前用于风险提示。"""

    system = platform.system().lower()
    try:
        if system == "windows":
            output = subprocess.check_output(
                [
                    "powershell",
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-Command",
                    "Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -and ($_.CommandLine -like '*OpenAI.Codex*' -or $_.CommandLine -like '*\\codex.exe*app-server*' -or $_.CommandLine -like '*\\Codex.exe*') } | Select-Object -First 8 | ForEach-Object { \"$($_.ProcessId) $($_.Name)\" }",
                ],
                text=True,
                stderr=subprocess.DEVNULL,
            )
        else:
            output = subprocess.check_output(["ps", "-eo", "pid=,comm=,args="], text=True)
    except Exception:
        return []
    lines = []
    for line in output.splitlines():
        lower = line.lower()
        if "codex" in lower and ("app-server" in lower or "openai.codex" in lower or "codex.exe" in lower):
            lines.append(line.strip())
    return lines[:8]


def repair_history(args: argparse.Namespace) -> dict[str, Any]:
    """执行或预览完整历史可见性修复。"""

    codex_home = resolve_user_path(args.codex_home) or default_codex_home()
    config_text = (codex_home / "config.toml").read_text(encoding="utf-8", errors="replace") if (codex_home / "config.toml").exists() else ""
    live_provider = parse_top_level_model_provider(config_text)
    target_provider = args.target_provider or live_provider or DEFAULT_TARGET_PROVIDER
    source_providers = set(args.source_provider or DEFAULT_SOURCE_PROVIDERS)
    source_providers.discard(target_provider)
    active_db = resolve_active_state_db(codex_home, config_text, args.state_db)

    if args.apply and not args.force:
        running = detect_running_codex_processes()
        if running:
            raise RuntimeError("Codex Desktop/app-server is running; close it first or pass --force. " + "; ".join(running))

    con = sqlite3.connect(active_db.path)
    con.row_factory = sqlite3.Row
    rows = load_history_rows(con)
    provider_update_ids = {
        row.id for row in rows if row.model_provider in source_providers and not args.skip_provider_bucket_sync
    }
    visible_rows, user_event_update_ids = filter_visible_rows(
        rows,
        target_provider,
        provider_update_ids,
        args.include_archived,
        args.include_subagents,
        args.source_filter,
    )
    session_ids = [item for value in args.session_id for item in value.split(",") if item.strip()]
    session_ids = [item.strip() for item in session_ids]
    project_path = normalize_path(args.project_path)
    focus_rows = choose_focus_rows(
        visible_rows,
        session_ids,
        project_path,
        args.count,
        args.balance_recent_window,
        args.max_per_project,
        args.max_total,
    )
    index_path = codex_home / "session_index.jsonl"
    global_state_path = codex_home / ".codex-global-state.json"
    existing_ids = existing_index_ids(index_path)
    missing_index_rows = [row for row in visible_rows if row.id not in existing_ids]
    hints_fix, projectless_remove, roots_add = update_global_state(
        global_state_path, visible_rows, focus_rows, project_path, apply=False
    )
    rollout_updates: list[tuple[Path, str, float]] = []
    for row in rows:
        if row.id not in provider_update_ids:
            continue
        path = rollout_path(row)
        if path is None:
            continue
        changed, text, old_mtime = rewrite_rollout_provider(path, target_provider)
        if changed:
            rollout_updates.append((path, text, old_mtime))

    outcome = {
        "dryRun": not args.apply,
        "codexHome": str(codex_home),
        "stateDbPath": str(active_db.path),
        "activeDbKind": active_db.kind,
        "liveConfigModelProvider": live_provider,
        "targetProvider": target_provider,
        "sourceProviderIds": sorted(source_providers),
        "sqliteThreads": len(rows),
        "providerRowsToUpdate": len(provider_update_ids),
        "providerRowsUpdated": 0,
        "rolloutProviderLinesToUpdate": len(rollout_updates),
        "rolloutProviderLinesUpdated": 0,
        "userEventRowsToUpdate": len(user_event_update_ids),
        "userEventRowsUpdated": 0,
        "visibleCandidateRows": len(visible_rows),
        "sessionIndexMissingToAppend": len(missing_index_rows),
        "sessionIndexAppended": 0,
        "selectedSessionIds": session_ids,
        "focusSelectedCount": len(focus_rows),
        "balancedRecentWindowEnabled": args.balance_recent_window,
        "balancedRecentWindowRows": len(focus_rows) if args.balance_recent_window and not session_ids else 0,
        "maxPerProject": args.max_per_project,
        "maxTotal": args.max_total,
        "sourceFilter": args.source_filter,
        "workspaceHintsToFix": hints_fix,
        "workspaceHintsFixed": 0,
        "projectlessIdsToRemove": projectless_remove,
        "projectlessIdsRemoved": 0,
        "savedWorkspaceRootsToAdd": roots_add,
        "savedWorkspaceRootsAdded": 0,
        "rolloutMtimesToTouch": len([row for row in focus_rows if row.rollout_path]),
        "rolloutMtimesTouched": 0,
        "backupDir": None,
    }
    if not args.apply:
        con.close()
        return outcome

    backup_dir = codex_home / "backups_state" / f"codex-history-tool-{now_stamp()}"
    rollout_paths = [path for path, _, _ in rollout_updates]
    rollout_paths.extend(Path(strip_long_prefix(row.rollout_path) or "") for row in focus_rows if row.rollout_path)
    backup_state(codex_home, active_db.path, backup_dir, [path for path in rollout_paths if path.exists()])
    with con:
        if provider_update_ids:
            placeholders = ",".join("?" for _ in provider_update_ids)
            outcome["providerRowsUpdated"] = con.execute(
                f"UPDATE threads SET model_provider = ? WHERE id IN ({placeholders})",
                [target_provider, *provider_update_ids],
            ).rowcount
        if user_event_update_ids:
            placeholders = ",".join("?" for _ in user_event_update_ids)
            outcome["userEventRowsUpdated"] = con.execute(
                f"UPDATE threads SET has_user_event = 1 WHERE id IN ({placeholders})",
                user_event_update_ids,
            ).rowcount
        for row in focus_rows:
            con.execute(
                "UPDATE threads SET updated_at = ?, updated_at_ms = ? WHERE id = ?",
                (row.updated_at, row.updated_at_ms, row.id),
            )
    con.close()

    updated_rollouts = 0
    for path, text, old_mtime in rollout_updates:
        path.write_text(text, encoding="utf-8", newline="\n")
        os.utime(path, (old_mtime, old_mtime))
        updated_rollouts += 1
    outcome["rolloutProviderLinesUpdated"] = updated_rollouts
    outcome["sessionIndexAppended"] = append_missing_index(index_path, missing_index_rows)
    moved, titles = move_focus_index(index_path, focus_rows)
    outcome["sessionIndexRowsMoved"] = moved
    outcome["sessionIndexTitlesUpdated"] = titles
    hints_fixed, projectless_removed, roots_added = update_global_state(
        global_state_path, visible_rows, focus_rows, project_path, apply=True
    )
    outcome["workspaceHintsFixed"] = hints_fixed
    outcome["projectlessIdsRemoved"] = projectless_removed
    outcome["savedWorkspaceRootsAdded"] = roots_added
    if args.sync_rollout_mtime:
        outcome["rolloutMtimesTouched"] = touch_rollout_mtimes(focus_rows)
    outcome["backupDir"] = str(backup_dir)
    return outcome


def list_history(args: argparse.Namespace) -> dict[str, Any]:
    """列出 Codex 历史记录，供 UI 或人工选择 session 后定向修复。"""

    codex_home = resolve_user_path(args.codex_home) or default_codex_home()
    config_text = (codex_home / "config.toml").read_text(encoding="utf-8", errors="replace") if (codex_home / "config.toml").exists() else ""
    active_db = resolve_active_state_db(codex_home, config_text, args.state_db)
    con = sqlite3.connect(active_db.path)
    con.row_factory = sqlite3.Row
    rows = load_history_rows(con)
    con.close()

    project = normalize_path(args.project_path)
    filtered = []
    for row in rows:
        if not args.include_archived and row.archived != 0:
            continue
        if not args.include_subagents and not is_user_thread(row, False):
            continue
        if args.source_filter and row.source != args.source_filter:
            continue
        if args.provider and row.model_provider != args.provider:
            continue
        if project and normalize_path(row.cwd) != project:
            continue
        if args.query:
            query = args.query.lower()
            haystack = " ".join([row.id, row.title, row.cwd or "", row.model_provider or ""]).lower()
            if query not in haystack:
                continue
        filtered.append(row)
    filtered.sort(key=lambda row: (row.updated_at_ms, row.id), reverse=True)
    limited = filtered[: args.limit]
    return {
        "codexHome": str(codex_home),
        "stateDbPath": str(active_db.path),
        "activeDbKind": active_db.kind,
        "totalMatched": len(filtered),
        "items": [
            {
                "id": row.id,
                "title": row.title,
                "cwd": normalize_path(row.cwd),
                "modelProvider": row.model_provider,
                "source": row.source,
                "threadSource": row.thread_source,
                "archived": bool(row.archived),
                "hasUserEvent": bool(row.has_user_event),
                "updatedAtMs": row.updated_at_ms,
                "updatedAt": iso_from_ms(row.updated_at_ms) if row.updated_at_ms else None,
                "rolloutPath": strip_long_prefix(row.rollout_path),
            }
            for row in limited
        ],
    }


def add_common_args(parser: argparse.ArgumentParser) -> None:
    """注册 list 和 repair 共用的 Codex 路径参数。"""

    parser.add_argument("--codex-home", default=str(default_codex_home()), help="Codex home directory")
    parser.add_argument("--state-db", default="", help="Explicit state_5.sqlite path")


def build_parser() -> argparse.ArgumentParser:
    """构建命令行解析器。"""

    parser = argparse.ArgumentParser(description="Repair and inspect Codex Desktop history visibility")
    parser.add_argument("--version", action="version", version=f"%(prog)s {VERSION}")
    parser.add_argument("--json", action="store_true", help="Print JSON output")
    subparsers = parser.add_subparsers(dest="command", required=True)

    list_parser = subparsers.add_parser("list", help="List Codex history sessions")
    add_common_args(list_parser)
    list_parser.add_argument("--project-path", default="")
    list_parser.add_argument("--provider", default="")
    list_parser.add_argument("--source-filter", default="")
    list_parser.add_argument("--query", default="")
    list_parser.add_argument("--limit", type=int, default=80)
    list_parser.add_argument("--include-archived", action="store_true")
    list_parser.add_argument("--include-subagents", action="store_true")
    list_parser.add_argument("--json", action="store_true", help=argparse.SUPPRESS)

    repair_parser = subparsers.add_parser("repair", help="Preview or apply history visibility repair")
    add_common_args(repair_parser)
    repair_parser.add_argument("--apply", action="store_true")
    repair_parser.add_argument("--force", action="store_true")
    repair_parser.add_argument("--project-path", default="")
    repair_parser.add_argument("--session-id", action="append", default=[])
    repair_parser.add_argument("--target-provider", default="")
    repair_parser.add_argument("--source-provider", action="append")
    repair_parser.add_argument("--count", type=int, default=30)
    repair_parser.add_argument("--window-limit", type=int, default=80)
    repair_parser.add_argument("--balance-recent-window", dest="balance_recent_window", action="store_true", default=True)
    repair_parser.add_argument("--no-balance-recent-window", dest="balance_recent_window", action="store_false")
    repair_parser.add_argument("--max-per-project", type=int, default=10)
    repair_parser.add_argument("--max-total", type=int, default=300)
    repair_parser.add_argument("--source-filter", default="vscode")
    repair_parser.add_argument("--include-archived", action="store_true")
    repair_parser.add_argument("--include-subagents", action="store_true")
    repair_parser.add_argument("--skip-provider-bucket-sync", action="store_true")
    repair_parser.add_argument("--sync-rollout-mtime", dest="sync_rollout_mtime", action="store_true", default=True)
    repair_parser.add_argument("--no-sync-rollout-mtime", dest="sync_rollout_mtime", action="store_false")
    repair_parser.add_argument("--json", action="store_true", help=argparse.SUPPRESS)
    return parser


def print_human(payload: dict[str, Any]) -> None:
    """打印便于 PowerShell/终端阅读的摘要，同时避免输出完整会话内容。"""

    if "items" in payload:
        print(f"codex_home={payload['codexHome']}")
        print(f"state_db={payload['stateDbPath']} ({payload['activeDbKind']})")
        print(f"total_matched={payload['totalMatched']}")
        for item in payload["items"]:
            title = (item["title"] or "").replace("\n", " ")[:80]
            print(f"{item['updatedAt'] or '-'}  {item['id']}  {item['modelProvider']}  {item['cwd']}  {title}")
        return
    for key, value in payload.items():
        if isinstance(value, (dict, list)):
            print(f"{key}={json.dumps(value, ensure_ascii=False)}")
        else:
            print(f"{key}={value}")


def main() -> int:
    """CLI 入口，所有输出均支持机器可读 JSON。"""

    parser = build_parser()
    args = parser.parse_args()
    try:
        payload = list_history(args) if args.command == "list" else repair_history(args)
    except Exception as exc:
        if args.json:
            print(json.dumps({"ok": False, "error": str(exc)}, ensure_ascii=False), file=sys.stderr)
        else:
            print(f"error={exc}", file=sys.stderr)
        return 1
    if args.json:
        print(json.dumps(payload, ensure_ascii=False, indent=2))
    else:
        print_human(payload)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
