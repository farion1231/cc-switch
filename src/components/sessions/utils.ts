import type { ReactNode } from "react";
import { createElement } from "react";
import type { AppId } from "@/lib/api";
import type { SessionMessage, SessionMeta } from "@/types";

const CODEX_IDE_CONTEXT_PREFIX = "# Context from my IDE setup:";
const CODEX_REQUEST_MARKER = "my request for codex";
export const UNKNOWN_PROJECT_DIR_KEY = "__unknown_project_dir__";

/** 会话来源：9 个应用（Claude Desktop 没有自己的会话记录，用 Claude Code 的）。 */
export const SESSION_APP_IDS = [
  "claude",
  "codex",
  "opencode",
  "hermes",
  "gemini",
  "pi",
  "grokbuild",
  "openclaw",
  "mcode",
] as const satisfies readonly AppId[];

export type SessionAppId = (typeof SESSION_APP_IDS)[number];

export const isSessionAppId = (value: string): value is SessionAppId =>
  (SESSION_APP_IDS as readonly string[]).includes(value);

/** 「会话记录在哪里」对话框：各来源的默认位置（改过配置目录的按改过的读）。 */
export const SESSION_SOURCE_PATHS: Record<SessionAppId, string[]> = {
  claude: ["~/.claude/projects"],
  codex: ["~/.codex/sessions", "~/.codex/archived_sessions"],
  gemini: ["~/.gemini/tmp/<project>/chats"],
  grokbuild: ["~/.grok/sessions", "~/.grok/archived_sessions"],
  opencode: ["~/.local/share/opencode"],
  openclaw: ["~/.openclaw/agents/<agent>/sessions"],
  hermes: ["~/.hermes/state.db", "~/.hermes/sessions"],
  pi: ["~/.pi/agent/sessions"],
  mcode: ["~/.minimax"],
};

/** MiniMax Code 的会话只能在 MiniMax Code 里删（后端也会拒绝）。 */
export const canDeleteSession = (session: SessionMeta) =>
  Boolean(session.sourcePath) && session.providerId !== "mcode";

export interface SessionProjectGroup {
  key: string;
  projectDir: string | null;
  label: string;
  sessions: SessionMeta[];
  latest: number;
}

export type SessionTimeBucket = "today" | "yesterday" | "thisWeek" | "earlier";

export interface SessionTimeGroup {
  bucket: SessionTimeBucket;
  sessions: SessionMeta[];
}

const getCodexRequestHeadingPayload = (lineText: string) => {
  if (!lineText.startsWith("#")) return null;

  const heading = lineText.replace(/^#+\s*/, "");
  const suffix = heading.toLowerCase().startsWith(CODEX_REQUEST_MARKER)
    ? heading.slice(CODEX_REQUEST_MARKER.length).trimStart()
    : null;

  if (suffix === null) return null;
  if (!suffix) return "";
  if (!/^[:：\-—]/.test(suffix)) return null;

  return suffix.replace(/^[:：\-—\s]+/, "").trim();
};

const extractCodexPromptFromIdeContext = (content: string) => {
  const trimmed = content.trim();
  if (!trimmed.startsWith(CODEX_IDE_CONTEXT_PREFIX)) {
    return null;
  }

  // VS Code injects the real prompt as the LAST "## My request for Codex:"
  // section, so keep the final matching heading. Earlier matches can be
  // headings that live inside the active selection / open file content.
  // Trade-off: if the request body itself repeats the heading, the preview
  // truncates to its trailing part (rare; see sessionUtils.test.ts).
  const lines = trimmed.replace(/\r\n/g, "\n").split("\n");
  let prompt: string | null = null;
  for (const [index, line] of lines.entries()) {
    const inlinePrompt = getCodexRequestHeadingPayload(line.trim());
    if (inlinePrompt === null) continue;

    if (inlinePrompt) {
      prompt = inlinePrompt;
      continue;
    }

    const followingPrompt = lines
      .slice(index + 1)
      .join("\n")
      .trim();
    prompt = followingPrompt || null;
  }

  return prompt;
};

export const getSessionKey = (session: SessionMeta) =>
  `${session.providerId}:${session.sessionId}:${session.sourcePath ?? ""}`;

export const getSessionTime = (session: SessionMeta) =>
  session.lastActiveAt ?? session.createdAt ?? 0;

export const sortSessionsByTime = (sessions: SessionMeta[]) =>
  [...sessions].sort((a, b) => getSessionTime(b) - getSessionTime(a));

/** Codex / Grok Build 把归档的会话放在 archived_sessions 目录下。 */
export const isArchivedSession = (session: SessionMeta) =>
  (session.providerId === "codex" || session.providerId === "grokbuild") &&
  /[\\/]archived_sessions[\\/]/.test(session.sourcePath ?? "");

export const getBaseName = (value?: string | null) => {
  if (!value) return "";
  const trimmed = value.trim();
  if (!trimmed) return "";
  const normalized = trimmed.replace(/[\\/]+$/, "");
  const parts = normalized.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || trimmed;
};

/** 家目录写成 ~，路径短一些。 */
export const shortenHomePath = (value: string) =>
  value
    .replace(/^\/Users\/[^/]+(?=\/|$)/, "~")
    .replace(/^\/home\/[^/]+(?=\/|$)/, "~")
    .replace(/^[A-Za-z]:\\Users\\[^\\]+(?=\\|$)/, "~");

export const formatTimestamp = (value?: number) => {
  if (!value) return "";
  return new Date(value).toLocaleString();
};

/** 「10月1日 08:12」这类短格式，跟随系统语言。 */
export const formatShortDateTime = (value?: number) => {
  if (!value) return "";
  return new Date(value).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
};

export const formatClock = (value?: number) => {
  if (!value) return "";
  return new Date(value).toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });
};

export const formatRelativeTime = (
  value: number | undefined,
  t: (key: string, options?: Record<string, unknown>) => string,
) => {
  if (!value) return "";
  const now = Date.now();
  const diff = now - value;
  const minutes = Math.floor(diff / 60000);
  const hours = Math.floor(diff / 3600000);
  const days = Math.floor(diff / 86400000);

  if (minutes < 1) return t("sessionManager.justNow");
  if (minutes < 60) return t("sessionManager.minutesAgo", { count: minutes });
  if (hours < 24) return t("sessionManager.hoursAgo", { count: hours });
  if (days < 7) return t("sessionManager.daysAgo", { count: days });
  return new Date(value).toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
  });
};

export const formatSessionTitle = (session: SessionMeta) => {
  return (
    session.title ||
    getBaseName(session.projectDir) ||
    session.sessionId.slice(0, 8)
  );
};

/** 第二行的「最后：…」：summary 和标题相同时不写。 */
export const getSessionLastText = (session: SessionMeta) => {
  const summary = session.summary?.trim();
  if (!summary) return "";
  if (summary === formatSessionTitle(session).trim()) return "";
  return summary;
};

/**
 * 按项目目录分组（选了「全部应用」时跨应用合并）。组按组内最近一条排序，未知目录排最后；
 * 组内保持传入顺序。
 */
export const groupSessionsByProject = (
  sessions: SessionMeta[],
  unknownDirectoryLabel: string,
): SessionProjectGroup[] => {
  const groups = new Map<string, SessionProjectGroup>();

  sessions.forEach((session) => {
    const projectDir = session.projectDir?.trim() || null;
    const key = projectDir ?? UNKNOWN_PROJECT_DIR_KEY;
    let group = groups.get(key);
    if (!group) {
      group = {
        key,
        projectDir,
        label: projectDir
          ? getBaseName(projectDir) || projectDir
          : unknownDirectoryLabel,
        sessions: [],
        latest: 0,
      };
      groups.set(key, group);
    }
    group.sessions.push(session);
    group.latest = Math.max(group.latest, getSessionTime(session));
  });

  return Array.from(groups.values()).sort((a, b) => {
    if (a.projectDir === null) return 1;
    if (b.projectDir === null) return -1;
    return b.latest - a.latest;
  });
};

const startOfDay = (value: number) => {
  const date = new Date(value);
  date.setHours(0, 0, 0, 0);
  return date.getTime();
};

/** 本周从周一算起。 */
export const getSessionTimeBucket = (
  value: number,
  now = Date.now(),
): SessionTimeBucket => {
  const today = startOfDay(now);
  const day = startOfDay(value);
  if (day >= today) return "today";
  const yesterday = startOfDay(today - 12 * 3600000);
  if (day >= yesterday) return "yesterday";
  const weekday = (new Date(today).getDay() + 6) % 7; // 周一 = 0
  const weekStart = startOfDay(today - weekday * 86400000 + 12 * 3600000);
  return day >= weekStart ? "thisWeek" : "earlier";
};

/** 按时间平铺：今天 / 昨天 / 本周 / 更早（传入的会话已按时间倒序）。 */
export const groupSessionsByTime = (
  sessions: SessionMeta[],
  now = Date.now(),
): SessionTimeGroup[] => {
  const order: SessionTimeBucket[] = [
    "today",
    "yesterday",
    "thisWeek",
    "earlier",
  ];
  const buckets = new Map<SessionTimeBucket, SessionMeta[]>();
  sessions.forEach((session) => {
    const bucket = getSessionTimeBucket(getSessionTime(session), now);
    const list = buckets.get(bucket) ?? [];
    list.push(session);
    buckets.set(bucket, list);
  });
  return order
    .filter((bucket) => buckets.has(bucket))
    .map((bucket) => ({ bucket, sessions: buckets.get(bucket)! }));
};

export const shouldHideCodexMessageFromToc = (content: string) => {
  const trimmed = content.trim();
  return (
    trimmed.startsWith("# AGENTS.md instructions for ") ||
    trimmed.startsWith("<environment_context>") ||
    (trimmed.startsWith(CODEX_IDE_CONTEXT_PREFIX) &&
      !extractCodexPromptFromIdeContext(trimmed))
  );
};

export const extractCodexPromptPreview = (content: string) => {
  return extractCodexPromptFromIdeContext(content) ?? content;
};

export const formatSessionMessagePreview = (
  content: string,
  maxLength = 50,
) => {
  return (
    content.slice(0, maxLength) + (content.length > maxLength ? "..." : "")
  );
};

/** 恢复命令前面加上进入项目目录（POSIX shell）。 */
export const buildCdResumeCommand = (projectDir: string, command: string) =>
  `cd '${projectDir.replace(/'/g, `'\\''`)}' && ${command}`;

// ─── 阅读页：消息 → 显示条目 ──────────────────────────────────────────────

const TOOL_CALL_LINE = /^\[Tool(?::\s*([^\]]+))?\]$/;

/** 后端把工具调用写成单独一行「[Tool: Read]」；拆出工具名和其余正文。 */
export const splitToolCalls = (content: string) => {
  const names: string[] = [];
  const rest: string[] = [];
  content.split(/\r?\n/).forEach((line) => {
    const match = TOOL_CALL_LINE.exec(line.trim());
    if (match) {
      names.push(match[1]?.trim() || "Tool");
    } else {
      rest.push(line);
    }
  });
  return { names, text: rest.join("\n").trim() };
};

/** 「Read · Grep ×3 · Edit」：连续的同名调用合并计数。 */
export const formatToolNames = (names: string[]) => {
  const runs: { name: string; count: number }[] = [];
  names.forEach((name) => {
    const last = runs[runs.length - 1];
    if (last && last.name === name) {
      last.count += 1;
    } else {
      runs.push({ name, count: 1 });
    }
  });
  return runs
    .map((run) => (run.count > 1 ? `${run.name} ×${run.count}` : run.name))
    .join(" · ");
};

export interface SessionChatItem {
  kind: "message";
  key: string;
  /** 在原始消息数组里的下标（提问目录、查找按它定位） */
  messageIndex: number;
  role: string;
  content: string;
  ts?: number;
}

export interface SessionToolItem {
  kind: "tools";
  key: string;
  messageIndex: number;
  /** 被合并进来的全部消息下标 */
  messageIndexes: number[];
  names: string[];
  output: string;
  ts?: number;
}

export type SessionDisplayItem = SessionChatItem | SessionToolItem;

/**
 * 把消息整理成阅读页的条目：连续的工具调用（只有 [Tool: X] 的助手消息）和工具输出（role=tool）
 * 合并成一条；助手消息里夹带的工具调用拆出来跟在正文后面。
 */
export const buildSessionDisplayItems = (
  messages: SessionMessage[],
): SessionDisplayItem[] => {
  const items: SessionDisplayItem[] = [];
  let pending: SessionToolItem | null = null;

  const flush = () => {
    if (pending) {
      items.push(pending);
      pending = null;
    }
  };
  const addTool = (index: number, names: string[], output: string) => {
    if (!pending) {
      pending = {
        kind: "tools",
        key: `t${index}`,
        messageIndex: index,
        messageIndexes: [],
        names: [],
        output: "",
        ts: messages[index]?.ts,
      };
    }
    const current: SessionToolItem = pending;
    current.messageIndexes.push(index);
    current.names.push(...names);
    if (output) {
      current.output = current.output ? `${current.output}\n${output}` : output;
    }
  };

  messages.forEach((message, index) => {
    const role = message.role.toLowerCase();
    if (role === "tool") {
      addTool(index, [], message.content);
      return;
    }
    if (role === "assistant") {
      const { names, text } = splitToolCalls(message.content);
      if (names.length > 0 && !text) {
        addTool(index, names, "");
        return;
      }
      flush();
      items.push({
        kind: "message",
        key: `m${index}`,
        messageIndex: index,
        role: message.role,
        content: names.length > 0 ? text : message.content,
        ts: message.ts,
      });
      if (names.length > 0) {
        addTool(index, names, "");
      }
      return;
    }
    flush();
    items.push({
      kind: "message",
      key: `m${index}`,
      messageIndex: index,
      role: message.role,
      content: message.content,
      ts: message.ts,
    });
  });
  flush();
  return items;
};

/** 「只看对话」：去掉工具条目和系统消息。 */
export const isChatItem = (item: SessionDisplayItem) =>
  item.kind === "message" && item.role.toLowerCase() !== "system";

/** 「12.4k」「860」 */
export const formatCharCount = (count: number) =>
  count >= 1000 ? `${(count / 1000).toFixed(1)}k` : String(count);

export type MarkdownBlock =
  | { type: "text"; text: string }
  | { type: "code"; lang: string; code: string };

/** 只认围栏代码块，其余原样当文字（行内代码另由 splitInlineCode 处理）。 */
export const splitMarkdownBlocks = (content: string): MarkdownBlock[] => {
  const blocks: MarkdownBlock[] = [];
  const lines = content.replace(/\r\n/g, "\n").split("\n");
  let text: string[] = [];
  let code: string[] | null = null;
  let lang = "";
  let fence = "";

  const pushText = () => {
    const value = text.join("\n").replace(/^\n+|\n+$/g, "");
    if (value.trim()) blocks.push({ type: "text", text: value });
    text = [];
  };

  lines.forEach((line) => {
    if (code === null) {
      const open = /^\s*(`{3,}|~{3,})\s*([\w.+#-]*)/.exec(line);
      if (open) {
        pushText();
        code = [];
        fence = open[1];
        lang = open[2] ?? "";
        return;
      }
      text.push(line);
      return;
    }
    const trimmed = line.trim();
    if (
      trimmed.length >= fence.length &&
      trimmed === fence[0].repeat(trimmed.length)
    ) {
      blocks.push({ type: "code", lang, code: code.join("\n") });
      code = null;
      return;
    }
    code.push(line);
  });

  if (code !== null) {
    // 没闭合的代码块照样显示成代码
    blocks.push({ type: "code", lang, code: (code as string[]).join("\n") });
  }
  pushText();
  return blocks;
};

export const splitInlineCode = (text: string) =>
  text
    .split(/(`[^`\n]+`)/)
    .filter((part) => part !== "")
    .map((part) =>
      /^`[^`\n]+`$/.test(part)
        ? { code: true, text: part.slice(1, -1) }
        : { code: false, text: part },
    );

export const highlightText = (text: string, query: string): ReactNode => {
  if (!query) return text;
  const escaped = query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const parts = text.split(new RegExp(`(${escaped})`, "gi"));
  if (parts.length === 1) return text;
  return parts.map((part, i) =>
    i % 2 === 1
      ? createElement(
          "mark",
          {
            key: i,
            className: "rounded-sm bg-warning-soft px-0.5 text-inherit",
          },
          part,
        )
      : part,
  );
};

export const countMatches = (text: string, query: string) => {
  const needle = query.trim().toLowerCase();
  if (!needle) return 0;
  const haystack = text.toLowerCase();
  let count = 0;
  let at = haystack.indexOf(needle);
  while (at >= 0) {
    count += 1;
    at = haystack.indexOf(needle, at + needle.length);
  }
  return count;
};
