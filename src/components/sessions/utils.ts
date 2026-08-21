import type { ReactNode } from "react";
import { createElement } from "react";
import type { AppId } from "@/lib/api";
import type { SessionMeta } from "@/types";

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
  "copilot-byok",
  "copilot-cli",
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
  "copilot-byok": ["VS Code/User/workspaceStorage/<workspace>/chatSessions"],
  "copilot-cli": ["~/.copilot/session-state"],
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

/** 恢复命令前面加上进入项目目录（POSIX shell）。 */
export const buildCdResumeCommand = (projectDir: string, command: string) =>
  `cd '${projectDir.replace(/'/g, `'\\''`)}' && ${command}`;

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
