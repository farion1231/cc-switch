import type { TFunction } from "i18next";
import type { Subagent, SubagentErrorCode } from "@/lib/api/subagents";
import { extractErrorMessage } from "@/utils/errorUtils";
import {
  SUBAGENT_COLORS,
  safeParseSubagentMarkdown,
  type SubagentColor,
  type SubagentFields,
} from "./frontmatter";

/** Claude Code 内置工具（chip 多选的固定项）；其余写法（mcp__server__tool 等）走「自定义」 */
export const BUILTIN_TOOLS = [
  "Read",
  "Edit",
  "Write",
  "Bash",
  "Glob",
  "Grep",
  "WebFetch",
  "WebSearch",
  "NotebookEdit",
  "TodoWrite",
  "Agent",
  "Skill",
] as const;

/**
 * Claude Code 界面里 subagent 颜色的大致色值。用内联色而不是 Tailwind 色板：
 * 这套设计系统把颜色收成了语义 token，没有 red-500 这类类名可用。
 */
export const SUBAGENT_COLOR_HEX: Record<SubagentColor, string> = {
  red: "#ef4444",
  blue: "#3b82f6",
  green: "#22c55e",
  yellow: "#eab308",
  purple: "#a855f7",
  orange: "#f97316",
  pink: "#ec4899",
  cyan: "#06b6d4",
};

export function colorHexOf(color: string | undefined): string | undefined {
  if (!color) return undefined;
  return (SUBAGENT_COLORS as readonly string[]).includes(color)
    ? SUBAGENT_COLOR_HEX[color as SubagentColor]
    : undefined;
}

/** 列表行用的字段；frontmatter 坏了就只剩后端给的 name / description */
export function fieldsOf(subagent: Pick<Subagent, "content">): SubagentFields {
  const parsed = safeParseSubagentMarkdown(subagent.content);
  return parsed.ok ? parsed.value.fields : {};
}

/** `<目录>/<文件名>.md`；后端给的 fileName 带不带 .md 都能拼对 */
export function subagentFilePath(dir: string, fileName: string): string {
  const file = fileName.endsWith(".md") ? fileName : `${fileName}.md`;
  const trimmed = dir.replace(/[\\/]+$/, "");
  const sep = trimmed.includes("\\") && !trimmed.includes("/") ? "\\" : "/";
  return trimmed ? `${trimmed}${sep}${file}` : file;
}

export interface ParsedSubagentError {
  code?: SubagentErrorCode;
  /** 去掉错误码前缀后的说明 */
  detail: string;
  raw: string;
}

const ERROR_RE =
  /^(SUBAGENT_(?:DRIFTED|FILE_EXISTS|INVALID|NAME_TAKEN|NOT_FOUND)):?\s*([\s\S]*)$/;

/** 后端错误是 `SUBAGENT_XXX: 详情` 形式的字符串；拆出错误码，其余错误 code 为空 */
export function parseSubagentError(error: unknown): ParsedSubagentError {
  const raw = (extractErrorMessage(error) || String(error ?? "")).trim();
  const match = raw.match(ERROR_RE);
  if (!match) return { detail: raw, raw };
  return {
    code: match[1] as SubagentErrorCode,
    detail: match[2].trim(),
    raw,
  };
}

/** 需要用户确认后带 force 重试的冲突 */
export function isForceableConflict(code: SubagentErrorCode | undefined) {
  return code === "SUBAGENT_DRIFTED" || code === "SUBAGENT_FILE_EXISTS";
}

export function describeSubagentError(
  t: TFunction,
  error: ParsedSubagentError,
): string {
  switch (error.code) {
    case "SUBAGENT_INVALID":
      return t("subagents.errors.invalid", { detail: error.detail });
    case "SUBAGENT_NAME_TAKEN":
      return t("subagents.errors.nameTaken", { detail: error.detail });
    case "SUBAGENT_NOT_FOUND":
      return t("subagents.errors.notFound");
    case "SUBAGENT_DRIFTED":
      return t("subagents.errors.drifted", { detail: error.detail });
    case "SUBAGENT_FILE_EXISTS":
      return t("subagents.errors.fileExists", { detail: error.detail });
    default:
      return error.raw || t("common.unknown");
  }
}

/** 复制为新 agent 时找一个没被占用的名字：x-copy、x-copy-2… */
export function uniqueCopyName(base: string, taken: Set<string>): string {
  const stem = `${base}-copy`.slice(0, 64);
  if (!taken.has(stem)) return stem;
  for (let index = 2; index < 100; index++) {
    const candidate = `${stem.slice(0, 60)}-${index}`;
    if (!taken.has(candidate)) return candidate;
  }
  return `${stem.slice(0, 55)}-${Date.now().toString(36)}`;
}

export function matchesQuery(
  query: string,
  values: Array<string | undefined>,
): boolean {
  const normalized = query.trim().toLocaleLowerCase();
  if (!normalized) return true;
  return values.some((value) =>
    value?.toLocaleLowerCase().includes(normalized),
  );
}
