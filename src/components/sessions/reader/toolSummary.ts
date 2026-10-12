import type {
  ThinkingBlock,
  ToolCallBlock,
  ToolKind,
  ToolStatus,
} from "@/types";
import {
  READER_VERB_I18N_PREFIX,
  type AgentReaderStyle,
  type ReaderStyleId,
} from "./agentStyles";
import type { MergedStep, SessionTurn, TimelineStep, ToolStep } from "./turns";

/**
 * 步骤 / 轮的一行摘要与各 Agent 合并规则。
 * 只产出结构化数据和品牌固定字面（Ran / Explored / Called…）；需要翻译的通用文案
 * 交给调用方传入的 t()，或由组件按 i18n key 自行拼接。
 */

export type Translate = (
  key: string,
  options?: Record<string, unknown>,
) => string;

// ─── 状态 ─────────────────────────────────────────────────────────────────

/** 失败类状态：失败与中断都要常显（规则 2） */
export const isFailureStatus = (status: ToolStatus) =>
  status === "error" || status === "interrupted";

export const isFailureStep = (step: TimelineStep) =>
  (step.kind === "tool" || step.kind === "merged") &&
  isFailureStatus(step.status);

// ─── 合并 ─────────────────────────────────────────────────────────────────

const mergeKeyOf = (step: TimelineStep, style: AgentReaderStyle): string => {
  if (step.kind !== "tool" || step.status !== "success" || !step.call) {
    return "";
  }
  const { kind, server } = step.call;
  if (style.merge.readSearchRuns && (kind === "read" || kind === "search")) {
    return "explored";
  }
  if (style.merge.mcpSameServerRuns && kind === "mcp" && server) {
    return `mcp:${server}`;
  }
  return "";
};

/**
 * 按风格配置合并连续步骤：Codex 连续成功的 read/search → Explored；
 * Claude 连续成功的同一 MCP 服务器调用 → Called X N times。
 * 只合并成功的步骤（失败要单独常显），且至少 2 步才合并。
 */
export const mergeSteps = (
  steps: TimelineStep[],
  style: AgentReaderStyle,
): TimelineStep[] => {
  const merged: TimelineStep[] = [];
  let run: ToolStep[] = [];
  let runKey = "";
  const flush = () => {
    if (run.length >= 2) {
      merged.push({
        kind: "merged",
        id: `${run[0].id}+merged`,
        mergeKind: runKey === "explored" ? "explored" : "mcp",
        server: runKey === "explored" ? undefined : run[0].call?.server,
        children: run,
        status: "success",
      });
    } else {
      merged.push(...run);
    }
    run = [];
    runKey = "";
  };
  steps.forEach((step) => {
    const key = mergeKeyOf(step, style);
    if (key && key === runKey) {
      run.push(step as ToolStep);
      return;
    }
    flush();
    if (key) {
      run = [step as ToolStep];
      runKey = key;
    } else {
      merged.push(step);
    }
  });
  flush();
  return merged;
};

// ─── 格式化小工具 ─────────────────────────────────────────────────────────

/** 0.3s / 12s / 2m14s / 4m / 1h3m */
export const formatDuration = (ms: number): string => {
  if (ms < 1000) return `${Math.round(ms)}ms`;
  const seconds = ms / 1000;
  if (seconds < 10) return `${Number(seconds.toFixed(1))}s`;
  const totalSeconds = Math.round(seconds);
  if (totalSeconds < 60) return `${totalSeconds}s`;
  if (totalSeconds < 3600) {
    const m = Math.floor(totalSeconds / 60);
    const s = totalSeconds % 60;
    return s ? `${m}m${s}s` : `${m}m`;
  }
  const h = Math.floor(totalSeconds / 3600);
  const m = Math.floor((totalSeconds % 3600) / 60);
  return m ? `${h}h${m}m` : `${h}h`;
};

/** 980 / 28.5k / 1.2M */
export const formatTokens = (tokens: number): string => {
  if (tokens < 1000) return String(tokens);
  if (tokens < 1_000_000) return `${Number((tokens / 1000).toFixed(1))}k`;
  return `${Number((tokens / 1_000_000).toFixed(1))}M`;
};

/** $0.0081 / $0.012 / $1.25 */
export const formatCost = (usd: number): string => {
  if (usd < 0.01) return `$${Number(usd.toFixed(4))}`;
  if (usd < 1) return `$${Number(usd.toFixed(3))}`;
  return `$${usd.toFixed(2)}`;
};

const escapeRegExp = (text: string) =>
  text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

const HOME_PREFIX = /(^|[\s"'(])\/(?:Users|home)\/[^/\s"']+(?=\/)/g;

/**
 * 路径缩短：projectDir 之下显示相对路径（projectDir 本身显示为 `.`），
 * 其余家目录下的路径显示为 `~/…`。对整段文本替换，搜索标题里的 `in <path>` 也适用。
 */
export const shortenPath = (text: string, projectDir?: string | null) => {
  let result = text;
  const root = projectDir?.replace(/\/+$/, "");
  if (root) {
    const escaped = escapeRegExp(root);
    result = result
      .replace(new RegExp(`${escaped}/`, "g"), "")
      .replace(new RegExp(`${escaped}(?![\\w.-])`, "g"), ".");
  }
  return result.replace(HOME_PREFIX, "$1~");
};

// ─── 步骤标题 ─────────────────────────────────────────────────────────────

export interface StepTitle {
  /** icon 格式的前缀符号（OpenCode 的 `$` `→` `←`…）；其余格式为空串 */
  icon: string;
  /** 动作 / 工具名（品牌固定字面，generic 为翻译后的通用动词） */
  verb: string;
  /** 对象：命令、路径、搜索词、URL… */
  target: string;
  /** 次要信息：description、workdir、参数首个字段… */
  detail?: string;
  /** 拼好的一行标题 */
  text: string;
}

export interface StepTitleOptions {
  projectDir?: string | null;
  /** 翻译 generic 动词（`sessionManager.reader.verb.*`）；缺省时原样返回 key */
  t?: Translate;
}

/** 无配对的工具输出（Grok Build）用的通用动词 key */
export const TOOL_OUTPUT_VERB_KEY = `${READER_VERB_I18N_PREFIX}output`;

/** 风格自带动词之外、按原始工具名覆盖的叫法 */
const RAW_NAME_VERBS: Partial<Record<ReaderStyleId, Record<string, string>>> = {
  // Claude Code CLI 把 Edit 显示为 Update
  claude: { Edit: "Update", MultiEdit: "Update" },
  gemini: { google_web_search: "GoogleSearch" },
  opencode: { glob: "Glob", list: "List", websearch: "WebSearch" },
};

/** OpenCode TUI 的工具前缀符号 */
const OPENCODE_ICONS: Record<ToolKind, string> = {
  shell: "$",
  read: "→",
  search: "✱",
  edit: "←",
  write: "←",
  web: "%",
  mcp: "⚙",
  agent: "│",
  ask: "→",
  todo: "⚙",
  other: "⚙",
};

const PATH_KINDS: ReadonlySet<ToolKind> = new Set([
  "read",
  "search",
  "edit",
  "write",
]);

const translate = (text: string, t?: Translate) =>
  t && text.startsWith(READER_VERB_I18N_PREFIX) ? t(text) : text;

const verbFor = (
  call: ToolCallBlock,
  style: AgentReaderStyle,
  t?: Translate,
) => {
  const alias = RAW_NAME_VERBS[style.id]?.[call.rawName];
  if (alias) return alias;
  const verb = style.verbs[call.kind];
  return verb === undefined ? call.rawName : translate(verb, t);
};

/** Codex 的文件改动动词：全是新增 Added、全是删除 Deleted，多文件显示「N files」 */
const codexDiffTitle = (call: ToolCallBlock, verb: string, target: string) => {
  const files = call.diff?.files ?? [];
  if (files.length === 0) return { verb, target };
  const ops = new Set(files.map((file) => file.op));
  const onlyOp = ops.size === 1 ? files[0].op : undefined;
  return {
    verb:
      onlyOp === "add" ? "Added" : onlyOp === "delete" ? "Deleted" : "Edited",
    target: files.length > 1 ? `${files.length} files` : target,
  };
};

/**
 * 步骤标题（动作 + 对象）。
 * - call（Claude）：`Bash(cargo build)`、`Read(src/a.ts:1-40)`、`Update(path)`、MCP `server.tool(arg)`
 * - verb（Codex / Gemini / Pi / generic）：`Ran cargo build`、`Edited 2 files`、`Called server.tool(arg)`
 * - icon（OpenCode）：`$ cargo build`、`→ Read path`、`✱ Grep "p" in dir`
 */
export const formatStepTitle = (
  step: ToolStep,
  style: AgentReaderStyle,
  options: StepTitleOptions = {},
): StepTitle => {
  const { call } = step;
  if (!call) {
    const verb = translate(TOOL_OUTPUT_VERB_KEY, options.t);
    return { icon: "", verb, target: "", text: verb };
  }
  let verb = verbFor(call, style, options.t);
  let target = PATH_KINDS.has(call.kind)
    ? shortenPath(call.title, options.projectDir)
    : call.title;
  let detail = call.detail;
  if (call.kind === "read" && detail?.startsWith(":")) {
    target += detail;
    detail = undefined;
  }
  if (call.kind === "mcp" && style.titleFormat !== "icon") {
    // MCP 的对象是 server.tool，参数首个字段放进括号
    if (style.titleFormat === "call") {
      verb = call.title;
      target = detail ?? "";
    } else {
      target = `${call.title}(${detail ?? ""})`;
    }
    detail = undefined;
  }
  if (style.id === "codex" && (call.kind === "edit" || call.kind === "write")) {
    ({ verb, target } = codexDiffTitle(call, verb, target));
  }
  if (target === verb) target = "";

  let icon = "";
  let text: string;
  if (style.titleFormat === "call") {
    text = target ? `${verb}(${target})` : verb;
  } else if (style.titleFormat === "icon") {
    icon = call.rawName === "websearch" ? "◈" : OPENCODE_ICONS[call.kind];
    text = [icon, verb, target].filter(Boolean).join(" ");
  } else {
    text = target ? `${verb} ${target}` : verb;
  }
  return { icon, verb, target, detail, text };
};

export interface MergedTitle extends StepTitle {
  /** 子步骤数 */
  count: number;
  /** explored：读了几个不同文件、搜索了几次 */
  reads: number;
  searches: number;
}

/** 合并步骤标题：Codex `Explored`；Claude `Called {server} {N} times`（品牌固定字面） */
export const formatMergedTitle = (step: MergedStep): MergedTitle => {
  const count = step.children.length;
  const reads = new Set(
    step.children
      .filter((child) => child.call?.kind === "read")
      .map((child) => child.call?.title),
  ).size;
  const searches = step.children.filter(
    (child) => child.call?.kind === "search",
  ).length;
  const verb =
    step.mergeKind === "explored"
      ? "Explored"
      : `Called ${step.server} ${count} times`;
  return { icon: "", verb, target: "", text: verb, count, reads, searches };
};

// ─── 步骤元信息 ───────────────────────────────────────────────────────────

export type StatusTone = "success" | "danger" | "warning" | "muted" | "neutral";

export interface StepMeta {
  status: ToolStatus;
  /** 风格配置里的状态符 */
  glyph: string;
  /** 状态色：success → text-success-text，danger → text-danger-text… */
  tone: StatusTone;
  exitCode?: number;
  durationMs?: number;
  /** 输出行数（有 diff 计数时不给） */
  lineCount?: number;
  diff?: { added: number; removed: number; files: number };
  /** OpenCode 每步 tokens / 费用（showStepCost） */
  tokens?: number;
  costUsd?: number;
}

const STATUS_TONE: Record<ToolStatus, StatusTone> = {
  success: "success",
  error: "danger",
  interrupted: "warning",
  pending: "muted",
  unknown: "neutral",
};

export const statusGlyph = (status: ToolStatus, style: AgentReaderStyle) =>
  status === "unknown" ? style.step.success : style.step[status];

/** 步骤一行的状态与元信息：状态符 + exit + 耗时 + 行数 / +x −y + tokens/费用 */
export const formatStepMeta = (
  step: ToolStep | MergedStep,
  style: AgentReaderStyle,
): StepMeta => {
  const meta: StepMeta = {
    status: step.status,
    glyph: statusGlyph(step.status, style),
    tone: STATUS_TONE[step.status],
  };
  if (step.kind === "merged") {
    const total = step.children.reduce(
      (sum, child) => sum + (child.result?.durationMs ?? 0),
      0,
    );
    if (total > 0) meta.durationMs = total;
    return meta;
  }
  const { call, result, cost } = step;
  const exitCode = result?.exitCode;
  if (
    exitCode !== undefined &&
    (style.showExitCode === "always" || exitCode !== 0)
  ) {
    meta.exitCode = exitCode;
  }
  meta.durationMs = result?.durationMs;
  const diff = call?.diff;
  if (style.showDiffCounts && diff && diff.added + diff.removed > 0) {
    meta.diff = {
      added: diff.added,
      removed: diff.removed,
      files: diff.files.length,
    };
  } else if (result && result.lineCount > 0) {
    meta.lineCount = result.lineCount;
  }
  if (style.showStepCost && cost) {
    meta.tokens = cost.tokens;
    meta.costUsd = cost.costUsd;
  }
  return meta;
};

// ─── 思考 ─────────────────────────────────────────────────────────────────

export interface ThinkingSummary {
  /** 可见预览的字数；partial = 还有全文可取 */
  chars: number;
  partial: boolean;
  durationMs?: number;
  /** Codex summary_text / Gemini subject：有则在该行直接显示（决策 D6） */
  summary?: string;
  redacted: boolean;
}

export const summarizeThinking = (block: ThinkingBlock): ThinkingSummary => ({
  chars: block.text.length,
  partial: Boolean(block.full),
  durationMs: block.durationMs,
  summary: block.summary,
  redacted: Boolean(block.redacted),
});

// ─── 轮摘要 ───────────────────────────────────────────────────────────────

export interface TurnSummary {
  /** 工具步骤数（合并步骤按子步骤计） */
  stepCount: number;
  commandCount: number;
  /** 成功改动的不同文件数 */
  filesChanged: number;
  /** 失败 + 中断 */
  errorCount: number;
  thinkingCount: number;
  /** 提问到本轮最后一条消息的时长；没有时间戳时用各步耗时之和 */
  durationMs?: number;
  kindCounts: Partial<Record<ToolKind, number>>;
  /** OpenCode step-finish 合计（决策 D13：摘要显示合计） */
  tokens?: number;
  costUsd?: number;
}

/** 轮内全部工具步骤（展开合并步骤） */
export const toolStepsOf = (turn: SessionTurn): ToolStep[] =>
  turn.steps.flatMap((step) =>
    step.kind === "merged" ? step.children : step.kind === "tool" ? [step] : [],
  );

const sumDefined = (values: (number | undefined)[]) => {
  const present = values.filter(
    (value): value is number => value !== undefined,
  );
  return present.length ? present.reduce((a, b) => a + b, 0) : undefined;
};

/** 折叠摘要行：`执行过程 · N 步 · M 个命令 · 改了 K 个文件 · F 个失败 · ⏱ 2m14s` */
export const summarizeTurn = (turn: SessionTurn): TurnSummary => {
  const tools = toolStepsOf(turn);
  const kindCounts: Partial<Record<ToolKind, number>> = {};
  const files = new Set<string>();
  tools.forEach((step) => {
    const kind = step.call?.kind ?? "other";
    kindCounts[kind] = (kindCounts[kind] ?? 0) + 1;
    const call = step.call;
    if (!call || isFailureStatus(step.status)) return;
    if (call.diff) {
      call.diff.files.forEach((file) => files.add(file.path));
    } else if (kind === "edit" || kind === "write") {
      files.add(call.title);
    }
  });

  const start = turn.question?.ts ?? turn.ts;
  const span =
    start !== undefined && turn.endTs !== undefined ? turn.endTs - start : 0;
  const stepDurations = tools.reduce(
    (sum, step) => sum + (step.result?.durationMs ?? 0),
    0,
  );
  const durationMs =
    span > 0 ? span : stepDurations > 0 ? stepDurations : undefined;

  return {
    stepCount: tools.length,
    commandCount: kindCounts.shell ?? 0,
    filesChanged: files.size,
    errorCount: tools.filter((step) => isFailureStatus(step.status)).length,
    thinkingCount: turn.steps.filter((step) => step.kind === "thinking").length,
    durationMs,
    kindCounts,
    tokens: sumDefined(turn.stepCosts.map((cost) => cost.tokens)),
    costUsd: sumDefined(turn.stepCosts.map((cost) => cost.costUsd)),
  };
};
