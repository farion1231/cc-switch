import type { ReactNode } from "react";
import { createElement } from "react";
import type { AppId } from "@/lib/api";
import type { SessionMeta, SessionTextRange } from "@/types";

export const UNKNOWN_PROJECT_DIR_KEY = "__unknown_project_dir__";

/** 会话来源：10 个应用（Claude Desktop 没有自己的会话记录，用 Claude Code 的）。 */
export const SESSION_APP_IDS = [
  "claude",
  "codex",
  "opencode",
  "hermes",
  "gemini",
  "pi",
  "omp",
  "grokbuild",
  "openclaw",
  "mcode",
  "kimi",
  "agy",
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
  omp: ["~/.omp/agent/sessions"],
  mcode: ["~/.minimax"],
  kimi: ["~/.kimi-code/sessions"],
  agy: [
    "~/.gemini/antigravity-cli/brain/<conversation>/.system_generated/logs",
  ],
};

/** MiniMax Code 的会话只能在 MiniMax Code 里删（后端也会拒绝）。 */
export const canDeleteSession = (session: SessionMeta) =>
  Boolean(session.sourcePath) &&
  !["mcode", "agy", "kimi"].includes(session.providerId);

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

/** 阅读页里单条消息的时间：月日 + 时分，不是今年的再带上年份（如「10月1日 16:29」） */
export const formatMessageTime = (value?: number) => {
  if (!value) return "";
  const date = new Date(value);
  return date.toLocaleString(undefined, {
    ...(date.getFullYear() !== new Date().getFullYear()
      ? { year: "numeric" as const }
      : {}),
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
 * 分组用的目录 key：去掉末尾分隔符；Windows 路径（盘符或 UNC 开头）不分大小写、
 * `/` 统一成 `\`，`D:\demo` 和 `d:/demo/` 归为同一组。只用来比较，显示仍用原路径。
 */
const getProjectDirKey = (projectDir: string) => {
  const isWindowsPath = /^(?:[A-Za-z]:[\\/]|\\\\[^\\])/.test(projectDir);
  if (!isWindowsPath) return projectDir.replace(/(.)\/+$/, "$1");
  return projectDir
    .replace(/\//g, "\\")
    .replace(/(.)\\+$/, "$1")
    .toLowerCase();
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
    const key = projectDir
      ? getProjectDirKey(projectDir)
      : UNKNOWN_PROJECT_DIR_KEY;
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

/** 恢复命令前面加上进入项目目录（POSIX shell）。 */
export const buildCdResumeCommand = (projectDir: string, command: string) =>
  `cd '${projectDir.replace(/'/g, `'\\''`)}' && ${command}`;

/** 非重叠的小写字符串匹配，位置映射回原文的 UTF-16 偏移。 */
export function* findTextMatches(text: string, query: string) {
  const needle = query.trim().toLowerCase();
  if (!needle) return;
  const haystack = text.toLowerCase();
  let at = haystack.indexOf(needle);
  if (at < 0) return;

  // ASCII 小写长度不变；大段日志不需要逐码点构建偏移数组。
  if (!/[^\x00-\x7f]/.test(text)) {
    while (at >= 0) {
      yield { start: at, end: at + needle.length };
      at = haystack.indexOf(needle, at + needle.length);
    }
    return;
  }

  // 内容必须整串小写以保留词尾 Σ → ς；逐码点小写只用于计算偏移长度。
  // 默认 Unicode 小写的上下文规则不会改变长度，İ → i + ◌̇ 则映射回同一码点。
  const offsets: number[] = [];
  let originalAt = 0;
  for (const point of text) {
    const length = point.toLowerCase().length;
    for (let i = 0; i < length; i += 1) {
      offsets.push(originalAt);
    }
    originalAt += point.length;
  }

  while (at >= 0) {
    const last = offsets[at + needle.length - 1];
    yield {
      start: offsets[at],
      end: last + (text.codePointAt(last)! > 0xffff ? 2 : 1),
    };
    at = haystack.indexOf(needle, at + needle.length);
  }
}

/** 只渲染纯文本和已验证的原文范围，不重新匹配截断后的上下文。 */
export const highlightTextRanges = (
  text: string,
  ranges: readonly SessionTextRange[],
): ReactNode => {
  if (!Array.isArray(ranges)) return text;
  const valid = ranges
    .filter(
      (range) =>
        range &&
        Number.isInteger(range.start) &&
        Number.isInteger(range.end) &&
        range.start >= 0 &&
        range.start < range.end &&
        range.end <= text.length &&
        !(range.start > 0 && text.codePointAt(range.start - 1)! > 0xffff) &&
        !(text.codePointAt(range.end - 1)! > 0xffff),
    )
    .map(({ start, end }) => ({ start, end }))
    .sort((a, b) => a.start - b.start || a.end - b.end);
  const matches: SessionTextRange[] = [];
  for (const match of valid) {
    const previous = matches[matches.length - 1];
    // 小写扩展中的不同匹配可能覆盖同一原文码点；合并，避免重复显示原文。
    if (previous && match.start < previous.end) {
      previous.end = Math.max(previous.end, match.end);
    } else {
      matches.push(match);
    }
  }
  if (!matches.length) return text;

  const parts: ReactNode[] = [];
  let cursor = 0;
  for (const { start, end } of matches) {
    parts.push(
      text.slice(cursor, start),
      createElement(
        "mark",
        {
          key: start,
          className: "rounded-sm bg-warning-soft px-0.5 text-inherit",
        },
        text.slice(start, end),
      ),
    );
    cursor = end;
  }
  parts.push(text.slice(cursor));
  return parts;
};

export const highlightText = (text: string, query: string): ReactNode =>
  highlightTextRanges(text, [...findTextMatches(text, query)]);

/** 截取并压缩空白，把完整原文的命中范围映射到最终摘录的 UTF-16 坐标。 */
export const createHighlightedSnippet = (
  text: string,
  matches: readonly SessionTextRange[],
  start = 0,
  end = text.length,
): { text: string; highlights: SessionTextRange[] } => {
  const source = text.slice(start, end);
  const starts: number[] = [];
  const ends: number[] = [];
  let snippet = start > 0 ? "…" : "";
  for (const part of source.matchAll(/\s+|\S+/g)) {
    const value = part[0];
    const whitespace = /^\s/.test(value);
    const offset = snippet.length;
    snippet += whitespace ? " " : value;
    for (let i = 0; i < value.length; i += 1) {
      starts[part.index + i] = offset + (whitespace ? 0 : i);
      ends[part.index + i] = offset + (whitespace ? 1 : i + 1);
    }
  }
  const highlights = matches.flatMap((match) => {
    const left = Math.max(start, match.start);
    const right = Math.min(end, match.end);
    return left < right
      ? [{ start: starts[left - start], end: ends[right - start - 1] }]
      : [];
  });
  return {
    text: snippet + (end < text.length ? "…" : ""),
    highlights,
  };
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
