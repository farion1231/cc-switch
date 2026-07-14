#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Codex WeChat Claw PoC.

This is a small iLink long-poll bridge:
WeChat iLink -> codex app-server/exec -> WeChat iLink.
"""

from __future__ import annotations

import argparse
import base64
import atexit
import json
import os
import queue
import re
import secrets
import struct
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any


ILINK_BASE_URL = "https://ilinkai.weixin.qq.com"
ILINK_APP_ID = "bot"
CHANNEL_VERSION = "2.1.3"
ILINK_APP_CLIENT_VERSION = str((2 << 16) | (1 << 8) | 3)

EP_GET_UPDATES = "ilink/bot/getupdates"
EP_SEND_MESSAGE = "ilink/bot/sendmessage"
EP_SEND_TYPING = "ilink/bot/sendtyping"
EP_GET_CONFIG = "ilink/bot/getconfig"
EP_GET_BOT_QR = "ilink/bot/get_bot_qrcode"
EP_GET_QR_STATUS = "ilink/bot/get_qrcode_status"

ITEM_TEXT = 1
ITEM_IMAGE = 2
ITEM_VIDEO = 3
ITEM_FILE = 5
ITEM_VOICE = 6

MSG_TYPE_BOT = 2
MSG_STATE_FINISH = 2

ANSI_RE = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]")
LOG_FILE: Path | None = None


@dataclass
class CodexConfig:
    backend: str = "app-server"
    command: str = "codex"
    app_server_command: str = ""
    workspace: str = "."
    model: str = ""
    profile: str = ""
    sandbox: str = "read-only"
    approval_policy: str = "never"
    timeout_seconds: int = 300
    extra_args: list[str] = field(default_factory=list)
    prompt_prefix: str = "回复使用中文，格式适合微信阅读。"


@dataclass
class BridgeConfig:
    base_url: str = ILINK_BASE_URL
    token: str = ""
    account_id: str = ""
    allow_all: bool = False
    allow_user_ids: set[str] = field(default_factory=set)
    require_prefix: bool = True
    prefix: str = "/codex"
    allow_id_command_for_unauthorized: bool = True
    poll_timeout_seconds: int = 35
    retry_delay_seconds: int = 2
    backoff_delay_seconds: int = 30
    max_consecutive_failures: int = 3
    max_reply_chars: int = 1800
    enable_typing: bool = True
    state_dir: Path = field(
        default_factory=lambda: Path.home() / ".codex-plus-plus" / "wechat-claw"
    )
    codex: CodexConfig = field(default_factory=CodexConfig)


def log(message: str) -> None:
    line = f"{time.strftime('[%Y-%m-%d %H:%M:%S]')} {message}"
    print(line, flush=True)
    if LOG_FILE:
        try:
            LOG_FILE.parent.mkdir(parents=True, exist_ok=True)
            with LOG_FILE.open("a", encoding="utf-8") as f:
                f.write(line + "\n")
        except Exception:
            pass


def load_json_file(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as f:
        data = json.load(f)
    if not isinstance(data, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return data


def load_config(path: Path) -> BridgeConfig:
    raw = load_json_file(path)
    codex_raw = raw.get("codex") or {}
    if not isinstance(codex_raw, dict):
        raise ValueError("codex config must be an object")

    cfg = BridgeConfig(
        base_url=str(raw.get("base_url") or ILINK_BASE_URL),
        token=str(raw.get("token") or ""),
        account_id=str(raw.get("account_id") or ""),
        allow_all=bool(raw.get("allow_all", False)),
        allow_user_ids=set(str(x) for x in raw.get("allow_user_ids") or []),
        require_prefix=bool(raw.get("require_prefix", True)),
        prefix=str(raw.get("prefix") or "/codex"),
        allow_id_command_for_unauthorized=bool(
            raw.get("allow_id_command_for_unauthorized", True)
        ),
        poll_timeout_seconds=int(raw.get("poll_timeout_seconds", 35)),
        retry_delay_seconds=int(raw.get("retry_delay_seconds", 2)),
        backoff_delay_seconds=int(raw.get("backoff_delay_seconds", 30)),
        max_consecutive_failures=int(raw.get("max_consecutive_failures", 3)),
        max_reply_chars=int(raw.get("max_reply_chars", 1800)),
        enable_typing=bool(raw.get("enable_typing", True)),
        state_dir=Path(str(raw.get("state_dir") or "")) if raw.get("state_dir") else Path.home() / ".codex-plus-plus" / "wechat-claw",
        codex=CodexConfig(
            backend=str(codex_raw.get("backend") or "app-server"),
            command=str(codex_raw.get("command") or "codex"),
            app_server_command=str(codex_raw.get("app_server_command") or ""),
            workspace=str(codex_raw.get("workspace") or "."),
            model=str(codex_raw.get("model") or ""),
            profile=str(codex_raw.get("profile") or ""),
            sandbox=str(codex_raw.get("sandbox") or "read-only"),
            approval_policy=str(codex_raw.get("approval_policy") or "never"),
            timeout_seconds=int(codex_raw.get("timeout_seconds", 300)),
            extra_args=[str(x) for x in codex_raw.get("extra_args") or []],
            prompt_prefix=str(codex_raw.get("prompt_prefix") or CodexConfig().prompt_prefix),
        ),
    )

    cfg.token = os.environ.get("WEIXIN_TOKEN") or os.environ.get("ILINK_TOKEN") or cfg.token
    cfg.base_url = os.environ.get("WEIXIN_BASE_URL") or os.environ.get("ILINK_BASE_URL") or cfg.base_url
    cfg.account_id = os.environ.get("WEIXIN_ACCOUNT_ID") or cfg.account_id

    return cfg


def random_wechat_uin() -> str:
    val = struct.unpack(">I", secrets.token_bytes(4))[0]
    return base64.b64encode(str(val).encode("utf-8")).decode("ascii")


def ilink_headers(token: str, body: bytes | None = None) -> dict[str, str]:
    headers = {
        "Content-Type": "application/json",
        "AuthorizationType": "ilink_bot_token",
        "X-WECHAT-UIN": random_wechat_uin(),
        "iLink-App-Id": ILINK_APP_ID,
        "iLink-App-ClientVersion": ILINK_APP_CLIENT_VERSION,
    }
    if body is not None:
        headers["Content-Length"] = str(len(body))
    if token:
        headers["Authorization"] = f"Bearer {token}"
    return headers


def api_post(base_url: str, endpoint: str, payload: dict[str, Any], token: str, timeout: int) -> dict[str, Any]:
    if "base_info" not in payload:
        payload = {**payload, "base_info": {"channel_version": CHANNEL_VERSION}}
    body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
    url = base_url.rstrip("/") + "/" + endpoint.lstrip("/")
    request = urllib.request.Request(url, data=body, headers=ilink_headers(token, body), method="POST")
    with urllib.request.urlopen(request, timeout=timeout) as resp:
        raw = resp.read().decode("utf-8", errors="replace")
    return json.loads(raw)


def api_get(base_url: str, endpoint_with_query: str, timeout: int) -> dict[str, Any]:
    url = base_url.rstrip("/") + "/" + endpoint_with_query.lstrip("/")
    request = urllib.request.Request(url, headers=ilink_headers("", None), method="GET")
    with urllib.request.urlopen(request, timeout=timeout) as resp:
        raw = resp.read().decode("utf-8", errors="replace")
    return json.loads(raw)


def load_sync_buf(cfg: BridgeConfig) -> str:
    path = cfg.state_dir / "get_updates_buf.txt"
    try:
        return path.read_text("utf-8")
    except FileNotFoundError:
        return ""


def save_sync_buf(cfg: BridgeConfig, buf: str) -> None:
    cfg.state_dir.mkdir(parents=True, exist_ok=True)
    (cfg.state_dir / "get_updates_buf.txt").write_text(buf, "utf-8")


def extract_text(item_list: list[dict[str, Any]]) -> str:
    for item in item_list:
        if item.get("type") == ITEM_TEXT:
            text = (item.get("text_item") or {}).get("text", "")
            ref = item.get("ref_msg") or {}
            ref_item = ref.get("message_item") or {}
            if not ref_item:
                return text
            ref_text = extract_text([ref_item])
            if ref_text:
                return f"[引用: {ref_text}]\n{text}"
            return text

    for item in item_list:
        if item.get("type") == ITEM_VOICE:
            return (item.get("voice_item") or {}).get("text", "")

    return ""


def describe_non_text_items(item_list: list[dict[str, Any]]) -> list[str]:
    descriptions: list[str] = []
    for item in item_list:
        item_type = item.get("type")
        if item_type == ITEM_IMAGE:
            descriptions.append("[图片消息：当前 PoC 暂未下载图片]")
        elif item_type == ITEM_VIDEO:
            descriptions.append("[视频消息：当前 PoC 暂未下载视频]")
        elif item_type == ITEM_FILE:
            file_item = item.get("file_item") or {}
            name = file_item.get("file_name") or "文件"
            descriptions.append(f"[文件消息：{name}，当前 PoC 暂未下载文件]")
        elif item_type == ITEM_VOICE and not (item.get("voice_item") or {}).get("text"):
            descriptions.append("[语音消息：未包含转写文本]")
    return descriptions


def normalize_prompt(text: str, item_list: list[dict[str, Any]]) -> str:
    parts = [text.strip()] if text.strip() else []
    parts.extend(describe_non_text_items(item_list))
    return "\n".join(parts).strip()


def is_authorized(cfg: BridgeConfig, user_id: str) -> bool:
    return cfg.allow_all or user_id in cfg.allow_user_ids


def strip_prefix(cfg: BridgeConfig, text: str) -> tuple[bool, str]:
    if not cfg.require_prefix:
        return True, text.strip()

    stripped = text.strip()
    prefix = cfg.prefix.strip()
    if not prefix:
        return True, stripped

    if stripped == prefix:
        return True, ""
    if stripped.startswith(prefix + " "):
        return True, stripped[len(prefix):].strip()
    if stripped.startswith(prefix + "\n"):
        return True, stripped[len(prefix):].strip()
    return False, stripped


def clean_codex_output(stdout: str, stderr: str) -> str:
    text = stdout.strip() or stderr.strip()
    text = ANSI_RE.sub("", text)
    lines = [line.rstrip() for line in text.splitlines()]
    while lines and not lines[0].strip():
        lines.pop(0)
    while lines and not lines[-1].strip():
        lines.pop()
    return "\n".join(lines).strip()


def build_wechat_prompt(codex: CodexConfig, prompt: str) -> str:
    full_prompt = (
        "请直接回答这条微信消息，不要确认收到，不要要求用户再发送消息。"
        f"微信消息：{prompt}"
    )
    if codex.prompt_prefix:
        full_prompt += "。" + codex.prompt_prefix.strip()
    return full_prompt


def run_codex(cfg: BridgeConfig, prompt: str) -> str:
    codex = cfg.codex
    full_prompt = build_wechat_prompt(codex, prompt)

    args = [
        codex.command,
        "--ask-for-approval",
        codex.approval_policy,
        "exec",
        "-C",
        codex.workspace,
        "--sandbox",
        codex.sandbox,
    ]
    if codex.model:
        args.extend(["--model", codex.model])
    if codex.profile:
        args.extend(["--profile", codex.profile])
    args.extend(codex.extra_args)
    args.append(full_prompt)

    log("调用 codex exec")
    try:
        proc = subprocess.run(
            args,
            text=True,
            capture_output=True,
            timeout=codex.timeout_seconds,
            encoding="utf-8",
            errors="replace",
        )
    except subprocess.TimeoutExpired:
        return f"Codex 执行超时（>{codex.timeout_seconds}s）。"
    except FileNotFoundError:
        return f"找不到 Codex 命令：{codex.command}"

    output = clean_codex_output(proc.stdout, proc.stderr)
    if proc.returncode != 0:
        return output or f"Codex 执行失败，退出码 {proc.returncode}。"
    return output or "Codex 没有返回内容。"


def resolve_app_server_command(codex: CodexConfig) -> str:
    if codex.app_server_command:
        return codex.app_server_command

    command = codex.command
    lower = command.replace("\\", "/").lower()
    if lower.endswith("/codex.cmd"):
        npm_dir = Path(command).parent
        native = (
            npm_dir
            / "node_modules"
            / "@openai"
            / "codex"
            / "node_modules"
            / "@openai"
            / "codex-win32-x64"
            / "vendor"
            / "x86_64-pc-windows-msvc"
            / "bin"
            / "codex.exe"
        )
        if native.exists():
            return str(native)
    return command


class AppServerCodexClient:
    def __init__(self, cfg: BridgeConfig):
        self.cfg = cfg
        self.command = resolve_app_server_command(cfg.codex)
        self.proc: subprocess.Popen[str] | None = None
        self.messages: queue.Queue[dict[str, Any]] = queue.Queue()
        self.next_id = 1
        self.threads: dict[str, str] = {}
        self.lock = threading.Lock()

    def close(self) -> None:
        proc = self.proc
        self.proc = None
        if not proc:
            return
        try:
            proc.terminate()
            proc.wait(timeout=3)
        except Exception:
            try:
                proc.kill()
            except Exception:
                pass

    def _reader(self, stream: Any, name: str) -> None:
        for line in stream:
            line = line.strip()
            if not line:
                continue
            if name == "stderr":
                log(f"app-server stderr: {line[:500]}")
                continue
            try:
                self.messages.put(json.loads(line))
            except Exception:
                log(f"app-server 输出无法解析：{line[:500]}")

    def _start(self) -> None:
        if self.proc and self.proc.poll() is None:
            return

        args = [self.command, "app-server", "--stdio"]
        log(f"启动 codex app-server: {self.command}")
        self.proc = subprocess.Popen(
            args,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            bufsize=1,
        )
        assert self.proc.stdout is not None
        assert self.proc.stderr is not None
        threading.Thread(target=self._reader, args=(self.proc.stdout, "stdout"), daemon=True).start()
        threading.Thread(target=self._reader, args=(self.proc.stderr, "stderr"), daemon=True).start()
        self._request(
            "initialize",
            {
                "clientInfo": {"name": "codex-wechat", "version": "0.1.0"},
                "capabilities": None,
            },
            timeout=20,
        )

    def _send(self, message: dict[str, Any]) -> None:
        if not self.proc or not self.proc.stdin:
            raise RuntimeError("app-server 未启动")
        self.proc.stdin.write(json.dumps(message, ensure_ascii=False) + "\n")
        self.proc.stdin.flush()

    def _request(self, method: str, params: Any, timeout: int | None = None) -> dict[str, Any]:
        req_id = self.next_id
        self.next_id += 1
        self._send({"id": req_id, "method": method, "params": params})

        deadline = time.time() + (timeout or self.cfg.codex.timeout_seconds)
        while time.time() < deadline:
            try:
                msg = self.messages.get(timeout=0.5)
            except queue.Empty:
                if self.proc and self.proc.poll() is not None:
                    raise RuntimeError(f"app-server 已退出，退出码 {self.proc.returncode}")
                continue
            if msg.get("id") == req_id:
                if "error" in msg:
                    raise RuntimeError(f"{method} 失败：{msg['error']}")
                return msg.get("result") or {}
            # 其它通知留给当前请求外的事件循环自然丢弃。
        raise TimeoutError(f"{method} 超时")

    def _ensure_thread(self, user_id: str) -> str:
        thread_id = self.threads.get(user_id)
        if thread_id:
            return thread_id

        params: dict[str, Any] = {
            "cwd": self.cfg.codex.workspace,
            "approvalPolicy": self.cfg.codex.approval_policy,
            "sandbox": self.cfg.codex.sandbox,
            "ephemeral": True,
            "baseInstructions": "你是运行在微信里的 Codex 助手。全程中文，只把最终答复发给微信用户。",
        }
        if self.cfg.codex.model:
            params["model"] = self.cfg.codex.model
        result = self._request("thread/start", params, timeout=60)
        thread_id = result["thread"]["id"]
        self.threads[user_id] = thread_id
        return thread_id

    def run(self, user_id: str, prompt: str) -> str:
        with self.lock:
            self._start()
            thread_id = self._ensure_thread(user_id)
            full_prompt = build_wechat_prompt(self.cfg.codex, prompt)
            result = self._request(
                "turn/start",
                {
                    "threadId": thread_id,
                    "input": [{"type": "text", "text": full_prompt, "text_elements": []}],
                    "approvalPolicy": self.cfg.codex.approval_policy,
                    "cwd": self.cfg.codex.workspace,
                },
                timeout=30,
            )
            turn_id = result["turn"]["id"]
            return self._collect_turn(thread_id, turn_id)

    def _collect_turn(self, thread_id: str, turn_id: str) -> str:
        deadline = time.time() + self.cfg.codex.timeout_seconds
        final_item_id = ""
        final_chunks: list[str] = []
        completed = False
        error = ""

        while time.time() < deadline and not completed:
            try:
                msg = self.messages.get(timeout=1)
            except queue.Empty:
                if self.proc and self.proc.poll() is not None:
                    raise RuntimeError(f"app-server 已退出，退出码 {self.proc.returncode}")
                continue

            method = msg.get("method")
            params = msg.get("params") or {}
            if params.get("threadId") != thread_id:
                continue

            if method == "item/started":
                item = params.get("item") or {}
                if item.get("type") == "agentMessage" and item.get("phase") == "final_answer":
                    final_item_id = str(item.get("id") or "")
            elif method == "item/agentMessage/delta":
                if final_item_id and params.get("turnId") == turn_id and params.get("itemId") == final_item_id:
                    final_chunks.append(str(params.get("delta") or ""))
            elif method == "item/completed":
                item = params.get("item") or {}
                if item.get("type") == "agentMessage" and item.get("phase") == "final_answer":
                    text = str(item.get("text") or "")
                    if text:
                        final_chunks = [text]
            elif method == "turn/completed" and params.get("turn", {}).get("id") == turn_id:
                turn = params.get("turn") or {}
                if turn.get("status") == "failed":
                    error = str((turn.get("error") or {}).get("message") or "turn failed")
                completed = True

        if not completed:
            raise TimeoutError("app-server turn 超时")
        if error:
            raise RuntimeError(error)

        reply = "".join(final_chunks).strip()
        return reply or "Codex 没有返回最终答复。"


class CodexRunner:
    def __init__(self, cfg: BridgeConfig):
        self.cfg = cfg
        self.app_server: AppServerCodexClient | None = None
        if cfg.codex.backend == "app-server":
            self.app_server = AppServerCodexClient(cfg)
            atexit.register(self.app_server.close)

    def run(self, user_id: str, prompt: str) -> str:
        if self.app_server:
            try:
                log("调用 codex app-server")
                return self.app_server.run(user_id, prompt)
            except Exception as exc:
                log(f"app-server 调用失败，回退 exec：{exc}")
        return run_codex(self.cfg, prompt)


def split_reply(text: str, max_chars: int) -> list[str]:
    if max_chars <= 0 or len(text) <= max_chars:
        return [text]

    chunks: list[str] = []
    rest = text
    while len(rest) > max_chars:
        cut = rest.rfind("\n", 0, max_chars)
        if cut < max_chars // 2:
            cut = max_chars
        chunks.append(rest[:cut].strip())
        rest = rest[cut:].strip()
    if rest:
        chunks.append(rest)
    return chunks


def send_text(cfg: BridgeConfig, to_user_id: str, text: str, context_token: str = "") -> None:
    for chunk in split_reply(text, cfg.max_reply_chars):
        msg = {
            "from_user_id": "",
            "to_user_id": to_user_id,
            "client_id": "codex-claw-" + secrets.token_hex(8),
            "message_type": MSG_TYPE_BOT,
            "message_state": MSG_STATE_FINISH,
            "item_list": [{"type": ITEM_TEXT, "text_item": {"text": chunk}}],
        }
        if context_token:
            msg["context_token"] = context_token
        api_post(cfg.base_url, EP_SEND_MESSAGE, {"msg": msg}, cfg.token, timeout=15)


def get_typing_ticket(cfg: BridgeConfig, user_id: str, context_token: str) -> str:
    payload: dict[str, Any] = {"ilink_user_id": user_id}
    if context_token:
        payload["context_token"] = context_token
    resp = api_post(cfg.base_url, EP_GET_CONFIG, payload, cfg.token, timeout=10)
    return str(resp.get("typing_ticket") or "")


def send_typing(cfg: BridgeConfig, user_id: str, ticket: str, status: int) -> None:
    if not ticket:
        return
    api_post(
        cfg.base_url,
        EP_SEND_TYPING,
        {"ilink_user_id": user_id, "typing_ticket": ticket, "status": status},
        cfg.token,
        timeout=10,
    )


def handle_command(cfg: BridgeConfig, user_id: str, text: str) -> str:
    command = text.strip()
    if command in {"/id", "/whoami", f"{cfg.prefix} id", f"{cfg.prefix} /id"}:
        return f"你的 iLink user_id：{user_id}"
    if command in {"/ping", f"{cfg.prefix} ping"}:
        return "pong"
    if command in {"/help", f"{cfg.prefix} help"}:
        return (
            "Codex 微信助手 PoC\n"
            f"- 发送 `{cfg.prefix} 你的问题` 调用 Codex\n"
            "- 发送 `/id` 查看你的 iLink user_id\n"
            "- 默认只响应 allow_user_ids 中的用户"
        )
    return ""


def process_message(
    cfg: BridgeConfig,
    runner: CodexRunner,
    msg: dict[str, Any],
    seen: dict[str, float],
) -> None:
    if msg.get("message_type", 1) != 1:
        return

    user_id = str(msg.get("from_user_id") or "")
    if not user_id or user_id == cfg.account_id:
        return

    msg_id = str(msg.get("message_id") or "")
    now = time.time()
    for key, ts in list(seen.items()):
        if now - ts > 300:
            del seen[key]
    if msg_id:
        if msg_id in seen:
            return
        seen[msg_id] = now

    context_token = str(msg.get("context_token") or "")
    item_list = msg.get("item_list") or []
    if not isinstance(item_list, list):
        item_list = []

    text = extract_text(item_list).strip()
    command_reply = handle_command(cfg, user_id, text)
    if command_reply and (is_authorized(cfg, user_id) or cfg.allow_id_command_for_unauthorized):
        send_text(cfg, user_id, command_reply, context_token)
        return

    if not is_authorized(cfg, user_id):
        log(f"忽略未授权用户：{user_id[:12]}")
        return

    accepted, stripped = strip_prefix(cfg, text)
    if not accepted:
        return

    prompt = normalize_prompt(stripped, item_list)
    if not prompt:
        send_text(cfg, user_id, "没有可处理的文本内容。", context_token)
        return

    log(f"收到消息 user={user_id[:12]} len={len(prompt)}")
    typing_ticket = ""
    if cfg.enable_typing:
        try:
            typing_ticket = get_typing_ticket(cfg, user_id, context_token)
            send_typing(cfg, user_id, typing_ticket, 1)
        except Exception as exc:
            log(f"发送 typing 失败：{exc}")

    try:
        reply = runner.run(user_id, prompt)
        send_text(cfg, user_id, reply, context_token)
    finally:
        if cfg.enable_typing and typing_ticket:
            try:
                send_typing(cfg, user_id, typing_ticket, 0)
            except Exception:
                pass


def poll_loop(cfg: BridgeConfig) -> None:
    global LOG_FILE
    LOG_FILE = cfg.state_dir / "codex-wechat.log"
    if not cfg.token:
        raise SystemExit("缺少 token：请在配置里填写 token，或设置 WEIXIN_TOKEN / ILINK_TOKEN。")
    if not cfg.allow_all and not cfg.allow_user_ids:
        log("未配置 allow_user_ids，除 /id 外不会处理任何用户消息。")

    cfg.state_dir.mkdir(parents=True, exist_ok=True)
    sync_buf = load_sync_buf(cfg)
    seen: dict[str, float] = {}
    failures = 0
    runner = CodexRunner(cfg)

    log(f"启动 Codex 微信助手，base={cfg.base_url}，backend={cfg.codex.backend}")
    while True:
        try:
            resp = api_post(
                cfg.base_url,
                EP_GET_UPDATES,
                {"get_updates_buf": sync_buf},
                cfg.token,
                timeout=cfg.poll_timeout_seconds + 5,
            )
            ret = resp.get("ret")
            errcode = resp.get("errcode")
            if (ret not in (None, 0)) or (errcode not in (None, 0)):
                failures += 1
                log(f"getupdates 返回错误 ret={ret} errcode={errcode} errmsg={resp.get('errmsg', '')}")
                time.sleep(cfg.backoff_delay_seconds if failures >= cfg.max_consecutive_failures else cfg.retry_delay_seconds)
                if failures >= cfg.max_consecutive_failures:
                    failures = 0
                continue

            failures = 0
            sync_buf = str(resp.get("get_updates_buf") or sync_buf)
            if sync_buf:
                save_sync_buf(cfg, sync_buf)

            msgs = resp.get("msgs") or []
            if msgs:
                log(f"收到 {len(msgs)} 条 iLink 消息")
            for msg in msgs:
                if isinstance(msg, dict):
                    try:
                        process_message(cfg, runner, msg, seen)
                    except Exception as exc:
                        log(f"处理消息失败：{exc}")

        except KeyboardInterrupt:
            log("收到中断，退出。")
            return
        except urllib.error.URLError as exc:
            failures += 1
            log(f"iLink 网络错误：{exc}")
            time.sleep(cfg.backoff_delay_seconds if failures >= cfg.max_consecutive_failures else cfg.retry_delay_seconds)
            if failures >= cfg.max_consecutive_failures:
                failures = 0
        except Exception as exc:
            failures += 1
            log(f"轮询异常：{exc}")
            time.sleep(cfg.backoff_delay_seconds if failures >= cfg.max_consecutive_failures else cfg.retry_delay_seconds)
            if failures >= cfg.max_consecutive_failures:
                failures = 0


def qr_login(output: Path, bot_type: str, timeout_seconds: int) -> None:
    resp = api_get(ILINK_BASE_URL, f"{EP_GET_BOT_QR}?bot_type={urllib.parse.quote(bot_type)}", timeout=35)
    qrcode = str(resp.get("qrcode") or "")
    qrcode_url = str(resp.get("qrcode_img_content") or "")
    if not qrcode:
        raise SystemExit("获取二维码失败：响应里没有 qrcode。")

    print("请用微信扫描下面的二维码链接：")
    print(qrcode_url)
    print()

    deadline = time.time() + timeout_seconds
    current_base = ILINK_BASE_URL
    while time.time() < deadline:
        status_resp = api_get(
            current_base,
            f"{EP_GET_QR_STATUS}?qrcode={urllib.parse.quote(qrcode)}",
            timeout=35,
        )
        status = str(status_resp.get("status") or "wait")
        if status == "wait":
            print(".", end="", flush=True)
        elif status == "scaned":
            print("\n已扫码，请在微信中确认。")
        elif status == "scaned_but_redirect":
            redirect_host = str(status_resp.get("redirect_host") or "")
            if redirect_host:
                current_base = "https://" + redirect_host
                print(f"\n切换 iLink base_url：{current_base}")
        elif status == "confirmed":
            token = str(status_resp.get("bot_token") or "")
            account_id = str(status_resp.get("ilink_bot_id") or "")
            base_url = str(status_resp.get("baseurl") or current_base or ILINK_BASE_URL)
            user_id = str(status_resp.get("ilink_user_id") or "")
            if not token or not account_id:
                raise SystemExit("扫码成功但没有拿到 bot_token/account_id。")

            config = load_json_file(output) if output.exists() else {}
            config.update({"base_url": base_url, "token": token, "account_id": account_id})
            if user_id:
                config["login_user_id"] = user_id
            output.parent.mkdir(parents=True, exist_ok=True)
            output.write_text(json.dumps(config, ensure_ascii=False, indent=2) + "\n", "utf-8")
            print(f"\n登录成功，配置已写入：{output}")
            print("注意：该文件包含微信 iLink token，不要提交到 Git。")
            return
        elif status == "expired":
            raise SystemExit("\n二维码已过期，请重新运行 login。")
        time.sleep(1)

    raise SystemExit("\n登录超时。")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Codex WeChat Claw PoC")
    sub = parser.add_subparsers(dest="command", required=True)

    run_parser = sub.add_parser("run", help="启动 iLink -> codex app-server/exec -> iLink 转发")
    run_parser.add_argument("--config", required=True, type=Path, help="配置文件路径")

    login_parser = sub.add_parser("login", help="扫码获取 iLink token 并写入配置")
    login_parser.add_argument("--output", required=True, type=Path, help="要写入的本地配置文件")
    login_parser.add_argument("--bot-type", default="3", help="iLink bot_type，默认 3")
    login_parser.add_argument("--timeout", default=480, type=int, help="扫码超时时间，秒")

    args = parser.parse_args(argv)
    if args.command == "run":
        poll_loop(load_config(args.config))
        return 0
    if args.command == "login":
        qr_login(args.output, args.bot_type, args.timeout)
        return 0
    raise SystemExit(f"unknown command: {args.command}")


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
