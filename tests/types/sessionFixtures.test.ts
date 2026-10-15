import { describe, expect, it } from "vitest";
import type {
  ContentRef,
  DiffFile,
  DiffSummary,
  EventBlock,
  EventKind,
  ImageBlock,
  ImageRef,
  ImageSource,
  MessageMeta,
  SessionBlock,
  SessionMessage,
  StepBlock,
  TextBlock,
  ThinkingBlock,
  ToolCallBlock,
  ToolKind,
  ToolResultBlock,
  ToolStatus,
} from "@/types";
import claude from "../fixtures/sessions/claude.messages.json";
import codex from "../fixtures/sessions/codex.messages.json";
import gemini from "../fixtures/sessions/gemini.messages.json";
import generic from "../fixtures/sessions/generic.messages.json";
import opencode from "../fixtures/sessions/opencode.messages.json";
import pi from "../fixtures/sessions/pi.messages.json";

/**
 * tests/fixtures/sessions/*.messages.json 是后端解析结果的契约样例（Rust 侧在
 * session_manager/model.rs 里做往返校验）。JSON import 会把字面量放宽成 string，
 * 没法直接 `satisfies SessionMessage[]`，所以这里用一份运行时 schema 校验，
 * schema 本身通过 `satisfies Spec<T>` 与 src/types.ts 绑定：类型增删字段或改变
 * 可选性时，这里会编译失败。
 */

// ─── 运行时 schema ────────────────────────────────────────────────────────

type Check = (value: unknown, path: string, errors: string[]) => void;
type Field<Required extends boolean> = { required: Required; check: Check };
/** 每个键都必须声明，且必填/可选与类型一致 */
type Spec<T> = {
  [K in keyof T]-?: undefined extends T[K] ? Field<false> : Field<true>;
};

const req = (check: Check): Field<true> => ({ required: true, check });
const opt = (check: Check): Field<false> => ({ required: false, check });

const fail = (errors: string[], path: string, msg: string) =>
  errors.push(`${path}: ${msg}`);

const str: Check = (v, p, e) => {
  if (typeof v !== "string") fail(e, p, "应为 string");
};
const int: Check = (v, p, e) => {
  if (!Number.isInteger(v) || (v as number) < 0) fail(e, p, "应为非负整数");
};
const signedInt: Check = (v, p, e) => {
  if (!Number.isInteger(v)) fail(e, p, "应为整数");
};
const num: Check = (v, p, e) => {
  if (typeof v !== "number" || !Number.isFinite(v)) fail(e, p, "应为 number");
};
const bool: Check = (v, p, e) => {
  if (typeof v !== "boolean") fail(e, p, "应为 boolean");
};
/** 可选布尔字段只在为 true 时出现（后端 skip_serializing_if = Not::not） */
const trueOnly: Check = (v, p, e) => {
  if (v !== true) fail(e, p, "省略或为 true");
};
const oneOf =
  (values: readonly string[]): Check =>
  (v, p, e) => {
    if (typeof v !== "string" || !values.includes(v))
      fail(e, p, `应为 ${values.join(" | ")}，实际 ${JSON.stringify(v)}`);
  };
const arrayOf =
  (item: Check, { nonEmpty = false } = {}): Check =>
  (v, p, e) => {
    if (!Array.isArray(v)) return fail(e, p, "应为数组");
    // 后端对空数组 skip_serializing_if = Vec::is_empty
    if (nonEmpty && v.length === 0) fail(e, p, "空数组应省略");
    v.forEach((x, i) => item(x, `${p}[${i}]`, e));
  };

function object<T>(spec: Spec<T>): Check {
  const fields = spec as Record<string, Field<boolean>>;
  return (v, p, e) => {
    if (typeof v !== "object" || v === null || Array.isArray(v))
      return fail(e, p, "应为对象");
    const rec = v as Record<string, unknown>;
    for (const key of Object.keys(rec)) {
      if (!(key in fields)) fail(e, `${p}.${key}`, "类型中不存在的字段");
    }
    for (const [key, field] of Object.entries(fields)) {
      if (!(key in rec)) {
        if (field.required) fail(e, `${p}.${key}`, "缺少必填字段");
        continue;
      }
      // 后端对 None 一律省略，不会出现 null
      if (rec[key] === null) {
        fail(e, `${p}.${key}`, "不应为 null（应省略）");
        continue;
      }
      field.check(rec[key], `${p}.${key}`, e);
    }
  };
}

/** 判别联合：按 tag 字段选 schema；`cases` 的键必须覆盖联合的全部 tag 值 */
function tagged<U, Tag extends keyof U & string>(
  tag: Tag,
  cases: { [K in U[Tag] & string]: Check },
): Check {
  const table = cases as Record<string, Check>;
  return (v, p, e) => {
    const t = (v as Record<string, unknown> | null)?.[tag];
    const check = typeof t === "string" ? table[t] : undefined;
    if (!check) return fail(e, p, `未知的 ${tag}: ${JSON.stringify(t)}`);
    check(v, p, e);
  };
}

/** 列出联合类型的全部取值；少列会编译失败 */
const allOf =
  <T extends string>() =>
  <L extends readonly T[]>(
    list: L &
      ([Exclude<T, L[number]>] extends [never]
        ? unknown
        : { missing: Exclude<T, L[number]> }),
  ) =>
    list;

const TOOL_KINDS = allOf<ToolKind>()([
  "shell",
  "read",
  "search",
  "edit",
  "write",
  "web",
  "mcp",
  "agent",
  "ask",
  "todo",
  "other",
] as const);
const TOOL_STATUSES = allOf<ToolStatus>()([
  "success",
  "error",
  "interrupted",
  "pending",
  "unknown",
] as const);
const EVENT_KINDS = allOf<EventKind>()([
  "aborted",
  "compaction",
  "model_change",
  "thinking_level",
  "hook",
  "pr_link",
  "slash_command",
  "info",
  "error",
  "sub_agent",
  "other",
] as const);
const BLOCK_TYPES = allOf<SessionBlock["type"]>()([
  "text",
  "thinking",
  "tool_call",
  "tool_result",
  "image",
  "event",
  "step",
] as const);

const lit = (value: string) => oneOf([value]);

const contentRef = tagged<ContentRef, "kind">("kind", {
  jsonl: object<Extract<ContentRef, { kind: "jsonl" }>>({
    kind: req(lit("jsonl")),
    offset: req(int),
    len: req(int),
    pointer: req(str),
  }),
  sqlite: object<Extract<ContentRef, { kind: "sqlite" }>>({
    kind: req(lit("sqlite")),
    table: req(str),
    id: req(str),
    column: req(str),
    pointer: req(str),
  }),
  file: object<Extract<ContentRef, { kind: "file" }>>({
    kind: req(lit("file")),
    relPath: req(str),
    pointer: req(str),
  }),
  sidecar: object<Extract<ContentRef, { kind: "sidecar" }>>({
    kind: req(lit("sidecar")),
    relPath: req(str),
  }),
});

const imageSource = tagged<ImageSource, "kind">("kind", {
  inline: object<Extract<ImageSource, { kind: "inline" }>>({
    kind: req(lit("inline")),
    content: req(contentRef),
  }),
  local_file: object<Extract<ImageSource, { kind: "local_file" }>>({
    kind: req(lit("local_file")),
    path: req(str),
  }),
});

const imageRef = object<ImageRef>({
  source: req(imageSource),
  mediaType: req(str),
  size: req(int),
  alt: opt(str),
});

const diffSummary = object<DiffSummary>({
  files: req(
    arrayOf(
      object<DiffFile>({
        path: req(str),
        op: req(oneOf(["add", "update", "delete", "rename"])),
        added: req(int),
        removed: req(int),
      }),
    ),
  ),
  added: req(int),
  removed: req(int),
  full: opt(contentRef),
});

const sessionBlock = tagged<SessionBlock, "type">("type", {
  text: object<TextBlock>({
    type: req(lit("text")),
    text: req(str),
    full: opt(contentRef),
  }),
  thinking: object<ThinkingBlock>({
    type: req(lit("thinking")),
    text: req(str),
    summary: opt(str),
    redacted: opt(trueOnly),
    durationMs: opt(int),
    full: opt(contentRef),
  }),
  tool_call: object<ToolCallBlock>({
    type: req(lit("tool_call")),
    id: req(str),
    rawName: req(str),
    kind: req(oneOf(TOOL_KINDS)),
    title: req(str),
    detail: opt(str),
    server: opt(str),
    inputPreview: req(str),
    inputTotalLen: req(int),
    inputFull: opt(contentRef),
    diff: opt(diffSummary),
    byUser: opt(trueOnly),
  }),
  tool_result: object<ToolResultBlock>({
    type: req(lit("tool_result")),
    callId: req(str),
    status: req(oneOf(TOOL_STATUSES)),
    preview: req(str),
    totalLen: req(int),
    lineCount: req(int),
    truncated: req(bool),
    full: opt(contentRef),
    exitCode: opt(signedInt),
    durationMs: opt(int),
    images: opt(arrayOf(imageRef, { nonEmpty: true })),
    savedPath: opt(str),
  }),
  image: object<ImageBlock>({ type: req(lit("image")), image: req(imageRef) }),
  event: object<EventBlock>({
    type: req(lit("event")),
    kind: req(oneOf(EVENT_KINDS)),
    text: opt(str),
    url: opt(str),
    full: opt(contentRef),
  }),
  step: object<StepBlock>({
    type: req(lit("step")),
    phase: req(oneOf(["start", "finish"])),
    tokens: opt(int),
    costUsd: opt(num),
    reason: opt(str),
  }),
});

const messageMeta = object<MessageMeta>({
  model: opt(str),
  inputTokens: opt(int),
  outputTokens: opt(int),
  cacheReadTokens: opt(int),
  cacheWriteTokens: opt(int),
  reasoningTokens: opt(int),
  costUsd: opt(num),
  durationMs: opt(int),
  stopReason: opt(str),
});

const sessionMessage = object<SessionMessage>({
  role: req(oneOf(["user", "assistant", "tool", "system"])),
  content: opt(str),
  ts: opt(int),
  id: opt(str),
  turnId: opt(str),
  injected: opt(trueOnly),
  blocks: opt(arrayOf(sessionBlock, { nonEmpty: true })),
  meta: opt(messageMeta),
});

// ─── 契约规则（与 Rust providers/blocks.rs 常量一致） ──────────────────────

const PREVIEW_LINES = 12;
const PREVIEW_CHARS = 1200;
const INPUT_PREVIEW_CHARS = 400;
const THINKING_PREVIEW_CHARS = 400;
const TITLE_CHARS = 200;

const charCount = (s: string) => Array.from(s).length;
/** 与 Rust `str::lines().count()` 一致：末尾换行不额外计一行 */
const lineCount = (s: string) =>
  s === "" ? 0 : s.replace(/\r?\n$/, "").split("\n").length;

const fixtures: Record<string, unknown> = {
  claude,
  codex,
  gemini,
  opencode,
  pi,
  generic,
};

const asMessages = (raw: unknown) => raw as SessionMessage[];
const allBlocks = (messages: SessionMessage[]) =>
  messages.flatMap((m) => m.blocks ?? []);

describe("session reader fixtures", () => {
  it.each(Object.entries(fixtures))(
    "%s 符合 SessionMessage 类型",
    (name, raw) => {
      const errors: string[] = [];
      arrayOf(sessionMessage, { nonEmpty: true })(raw, name, errors);
      expect(errors).toEqual([]);
    },
  );

  it.each(Object.entries(fixtures))(
    "%s 有 blocks 的消息不带 content（前端从 blocks 推导）",
    (name, raw) => {
      const offenders = asMessages(raw).flatMap((message, index) =>
        (message.blocks?.length ?? 0) > 0 && message.content !== undefined
          ? [`${name}[${index}]`]
          : [],
      );
      expect(offenders).toEqual([]);
    },
  );

  it.each(Object.entries(fixtures))(
    "%s 的 tool_result 都能配上同一轮里更早的 tool_call",
    (name, raw) => {
      const messages = asMessages(raw);
      const calls = new Map<string, { turnId?: string; index: number }>();
      const problems: string[] = [];
      messages.forEach((message, index) => {
        for (const block of message.blocks ?? []) {
          if (block.type === "tool_call") {
            if (calls.has(block.id))
              problems.push(
                `${name}[${index}] 重复的 tool_call id ${block.id}`,
              );
            calls.set(block.id, { turnId: message.turnId, index });
          }
          // callId 为空串表示源数据没有配对信息（Grok Build），按孤儿结果处理
          if (block.type === "tool_result" && block.callId !== "") {
            const call = calls.get(block.callId);
            if (!call) {
              problems.push(
                `${name}[${index}] 找不到 tool_call ${block.callId}`,
              );
            } else if (call.turnId !== message.turnId) {
              problems.push(`${name}[${index}] ${block.callId} 跨轮配对`);
            }
          }
        }
      });
      expect(problems).toEqual([]);
    },
  );

  it.each(Object.entries(fixtures))(
    "%s 的预览遵守长度上限与统计口径",
    (name, raw) => {
      const problems: string[] = [];
      allBlocks(asMessages(raw)).forEach((block, i) => {
        const at = `${name} block#${i}`;
        if (block.type === "tool_result") {
          if (lineCount(block.preview) > PREVIEW_LINES)
            problems.push(`${at} 预览超过 ${PREVIEW_LINES} 行`);
          if (charCount(block.preview) > PREVIEW_CHARS)
            problems.push(`${at} 预览超过 ${PREVIEW_CHARS} 字`);
          if (block.truncated) {
            if (block.totalLen <= charCount(block.preview))
              problems.push(`${at} truncated 但 totalLen 不大于预览`);
            if (!block.full && !block.savedPath)
              problems.push(`${at} truncated 但没有 full / savedPath`);
          } else {
            if (block.totalLen !== charCount(block.preview))
              problems.push(`${at} totalLen 与预览字数不符`);
            if (block.lineCount !== lineCount(block.preview))
              problems.push(`${at} lineCount 与预览行数不符`);
          }
        }
        if (block.type === "tool_call") {
          if (charCount(block.title) > TITLE_CHARS + 1)
            problems.push(`${at} 标题过长`);
          if (charCount(block.inputPreview) > INPUT_PREVIEW_CHARS)
            problems.push(`${at} 参数预览过长`);
          if (block.inputTotalLen < charCount(block.inputPreview))
            problems.push(`${at} inputTotalLen 小于预览`);
          if (block.kind === "mcp" && !block.server)
            problems.push(`${at} mcp 调用缺少 server`);
          if (block.diff) {
            const sum = (k: "added" | "removed") =>
              block.diff!.files.reduce((n, f) => n + f[k], 0);
            if (
              sum("added") !== block.diff.added ||
              sum("removed") !== block.diff.removed
            )
              problems.push(`${at} diff 合计与文件明细不符`);
          }
        }
        if (block.type === "thinking") {
          if (charCount(block.text) > THINKING_PREVIEW_CHARS && !block.full)
            problems.push(`${at} 思考超过预览长度却没有 full`);
          if (block.redacted && block.text !== "")
            problems.push(`${at} redacted 思考不应有正文`);
        }
      });
      expect(problems).toEqual([]);
    },
  );

  it("6 份 fixture 合起来覆盖所有块类型、工具类别、状态与事件", () => {
    const blocks = Object.values(fixtures).flatMap((raw) =>
      allBlocks(asMessages(raw)),
    );
    const seen = <T>(values: T[]) => [...new Set(values)].sort();

    expect(seen(blocks.map((b) => b.type))).toEqual([...BLOCK_TYPES].sort());
    expect(
      seen(blocks.flatMap((b) => (b.type === "tool_call" ? [b.kind] : []))),
    ).toEqual([...TOOL_KINDS].sort());
    expect(
      seen(blocks.flatMap((b) => (b.type === "tool_result" ? [b.status] : []))),
    ).toEqual([...TOOL_STATUSES].sort());
    expect(
      seen(blocks.flatMap((b) => (b.type === "event" ? [b.kind] : []))),
    ).toEqual([...EVENT_KINDS].sort());

    const images = blocks.flatMap((b) =>
      b.type === "image"
        ? [b.image]
        : b.type === "tool_result"
          ? (b.images ?? [])
          : [],
    );
    expect(seen(images.map((img) => img.source.kind))).toEqual([
      "inline",
      "local_file",
    ]);
    // 每份 fixture 至少有一条注入消息或事件之外的真人提问
    for (const [name, raw] of Object.entries(fixtures)) {
      const questions = asMessages(raw).filter(
        (m) => m.role === "user" && !m.injected,
      );
      expect(questions.length, name).toBeGreaterThan(0);
    }
  });
});
