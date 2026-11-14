import type { ToolKind } from "@/types";
import type { SessionAppId } from "../utils";

/**
 * 会话阅读页的 Agent 风格配置。
 *
 * 五家 Agent 共用同一套版式骨架，只用这份配置区分符号、动作叫法、主题色和合并规则；
 * 没有专属配置的来源（Hermes / OpenClaw / Grok Build / MiniMax Code）一律用 generic。
 * 主题色只用于提问竖条和助手 glyph，正文保持中性（决策 D4）。
 */

export type ReaderStyleId =
  | Extract<SessionAppId, "claude" | "codex" | "gemini" | "opencode" | "pi">
  | "generic";

export interface AgentReaderStyle {
  id: ReaderStyleId;
  /** CSS 变量名，提问竖条、助手 glyph、链接 hover 用它 */
  accentVar: string;
  /** 助手名前的符号；空串表示不画 */
  assistantGlyph: string;
  userGlyph: string;
  /** 步骤状态符 */
  step: {
    success: string;
    error: string;
    pending: string;
    interrupted: string;
  };
  /** 结果行引出符：tree 用符号，box 用左边框，none 不画 */
  resultFrame: "tree" | "box" | "none";
  resultConnector: string;
  thinkingGlyph: string;
  /** 思考行标签的 i18n key（sessionManager.reader.thinking.*） */
  thinkingLabelKey: string;
  /** 标题格式：call = Name(arg)；verb = 动词 + arg；icon = 符号 + Name arg */
  titleFormat: "call" | "verb" | "icon";
  /**
   * 每类工具的动词/名称：固定字面（Agent 原生叫法，不翻译），
   * 或以 `sessionManager.reader.verb.` 开头的 i18n key（generic 用）。
   * 缺省时用工具原名。
   */
  verbs: Partial<Record<ToolKind, string>>;
  /** 合并规则 */
  merge: { readSearchRuns: boolean; mcpSameServerRuns: boolean };
  /** 元信息 */
  showDiffCounts: boolean;
  showStepCost: boolean;
  showExitCode: "always" | "nonzero";
  /** 字体：步骤标题是否 mono */
  monoTitles: boolean;
}

/** generic 动词的 i18n key 前缀；formatStepTitle 见到这个前缀才走翻译 */
export const READER_VERB_I18N_PREFIX = "sessionManager.reader.verb.";

const verbKey = (name: string) => `${READER_VERB_I18N_PREFIX}${name}`;

const claude: AgentReaderStyle = {
  id: "claude",
  accentVar: "--agent-claude",
  assistantGlyph: "⏺",
  userGlyph: ">",
  step: { success: "⏺", error: "⏺", pending: "⏺", interrupted: "⏺" },
  resultFrame: "tree",
  resultConnector: "⎿",
  thinkingGlyph: "✻",
  thinkingLabelKey: "sessionManager.reader.thinking.claude",
  titleFormat: "call",
  // Claude 用工具原名（Bash / Read / Grep…），只有 Edit 按 CLI 显示为 Update（见 toolSummary）
  verbs: {},
  merge: { readSearchRuns: false, mcpSameServerRuns: true },
  showDiffCounts: true,
  showStepCost: false,
  showExitCode: "nonzero",
  monoTitles: true,
};

const codex: AgentReaderStyle = {
  id: "codex",
  accentVar: "--agent-codex",
  assistantGlyph: "•",
  userGlyph: "›",
  step: { success: "•", error: "•", pending: "•", interrupted: "•" },
  resultFrame: "tree",
  resultConnector: "└",
  thinkingGlyph: "•",
  thinkingLabelKey: "sessionManager.reader.thinking.codex",
  titleFormat: "verb",
  verbs: {
    shell: "Ran",
    read: "Read",
    search: "Search",
    edit: "Edited",
    write: "Added",
    web: "Searched",
    mcp: "Called",
    agent: "Spawned agent",
    ask: "Asked",
    todo: "Updated plan",
  },
  merge: { readSearchRuns: true, mcpSameServerRuns: false },
  showDiffCounts: true,
  showStepCost: false,
  showExitCode: "nonzero",
  monoTitles: false,
};

const gemini: AgentReaderStyle = {
  id: "gemini",
  accentVar: "--agent-gemini",
  assistantGlyph: "✦",
  userGlyph: ">",
  step: { success: "✓", error: "x", pending: "o", interrupted: "-" },
  resultFrame: "box",
  resultConnector: "",
  thinkingGlyph: "✦",
  thinkingLabelKey: "sessionManager.reader.thinking.gemini",
  titleFormat: "verb",
  verbs: {
    shell: "Shell",
    read: "ReadFile",
    search: "Search",
    edit: "Edit",
    write: "WriteFile",
    web: "WebFetch",
    mcp: "MCP",
    agent: "Agent",
    ask: "Ask",
    todo: "Todos",
  },
  merge: { readSearchRuns: false, mcpSameServerRuns: false },
  showDiffCounts: true,
  showStepCost: false,
  showExitCode: "nonzero",
  monoTitles: true,
};

const opencode: AgentReaderStyle = {
  id: "opencode",
  accentVar: "--agent-opencode",
  assistantGlyph: "",
  userGlyph: "",
  step: { success: "✓", error: "✗", pending: "~", interrupted: "✗" },
  resultFrame: "tree",
  resultConnector: "│",
  thinkingGlyph: "",
  thinkingLabelKey: "sessionManager.reader.thinking.opencode",
  titleFormat: "icon",
  verbs: {
    shell: "",
    read: "Read",
    search: "Grep",
    edit: "Edit",
    write: "Wrote",
    web: "WebFetch",
    agent: "Task",
    ask: "Asked",
    todo: "Todos",
  },
  merge: { readSearchRuns: false, mcpSameServerRuns: false },
  showDiffCounts: true,
  showStepCost: true,
  showExitCode: "nonzero",
  monoTitles: true,
};

const pi: AgentReaderStyle = {
  id: "pi",
  accentVar: "--agent-pi",
  assistantGlyph: "",
  userGlyph: "",
  // Pi 原生用整块背景表状态；这里统一为圆点 + 左侧竖条着色
  step: { success: "●", error: "●", pending: "●", interrupted: "●" },
  resultFrame: "none",
  resultConnector: "",
  thinkingGlyph: "",
  thinkingLabelKey: "sessionManager.reader.thinking.pi",
  titleFormat: "verb",
  verbs: {
    shell: "bash",
    read: "read",
    search: "grep",
    edit: "edit",
    write: "write",
    web: "fetch",
  },
  merge: { readSearchRuns: false, mcpSameServerRuns: false },
  showDiffCounts: true,
  showStepCost: false,
  showExitCode: "nonzero",
  monoTitles: true,
};

const generic: AgentReaderStyle = {
  id: "generic",
  accentVar: "--agent-generic",
  assistantGlyph: "•",
  userGlyph: "",
  step: { success: "•", error: "•", pending: "•", interrupted: "•" },
  resultFrame: "tree",
  resultConnector: "└",
  thinkingGlyph: "•",
  thinkingLabelKey: "sessionManager.reader.thinking.generic",
  titleFormat: "verb",
  verbs: {
    shell: verbKey("run"),
    read: verbKey("read"),
    search: verbKey("search"),
    edit: verbKey("edit"),
    write: verbKey("write"),
    web: verbKey("web"),
    mcp: verbKey("mcp"),
    agent: verbKey("agent"),
    ask: verbKey("ask"),
    todo: verbKey("todo"),
    other: verbKey("other"),
  },
  merge: { readSearchRuns: false, mcpSameServerRuns: false },
  showDiffCounts: true,
  showStepCost: false,
  showExitCode: "nonzero",
  monoTitles: true,
};

export const AGENT_READER_STYLES: Readonly<
  Record<ReaderStyleId, AgentReaderStyle>
> = { claude, codex, gemini, opencode, pi, generic };

/** 按会话来源取风格；没有专属配置的来源映射到 generic */
export const getAgentReaderStyle = (
  providerId: string | null | undefined,
): AgentReaderStyle =>
  providerId &&
  Object.prototype.hasOwnProperty.call(AGENT_READER_STYLES, providerId)
    ? AGENT_READER_STYLES[providerId as ReaderStyleId]
    : AGENT_READER_STYLES.generic;
