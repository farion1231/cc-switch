import { Fragment, type ReactNode } from "react";
import type { TFunction } from "i18next";
import { toast } from "@/lib/toast";
import type { AppId } from "@/lib/api";
import { extractErrorMessage } from "@/utils/errorUtils";

/** MiniMax Code 的全局说明上限（后端 validate_prompt_content，按 UTF-8 字节算）。 */
export const MCODE_PROMPT_LIMIT = 32 * 1024;

/** 带「撤销」的 toast 停 10 秒（悬停时 sonner 自动暂停计时），不带的用默认时长。 */
const UNDO_TOAST_DURATION = 10_000;

const PROMPT_FILE_NAME: Partial<Record<AppId, string>> = {
  claude: "CLAUDE.md",
  "claude-desktop": "CLAUDE.md",
  codex: "AGENTS.md",
  gemini: "GEMINI.md",
  grokbuild: "AGENTS.md",
  opencode: "AGENTS.md",
  openclaw: "AGENTS.md",
  hermes: "SOUL.md",
  pi: "AGENTS.md",
  mcode: "AGENTS.md",
  "copilot-byok": "copilot-instructions.md",
  "copilot-cli": "copilot-instructions.md",
};

export function promptFileName(app: AppId): string {
  return PROMPT_FILE_NAME[app] ?? "AGENTS.md";
}

export function utf8Bytes(text: string): number {
  return new TextEncoder().encode(text).length;
}

export function formatSize(bytes: number): string {
  return bytes < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(1)} KB`;
}

export function charCount(text: string): number {
  return Array.from(text).length;
}

/** 正文里第一行有字的内容（去掉 Markdown 标题符号），给没有说明的条目当第二行。 */
export function firstLine(text: string): string | null {
  for (const line of text.split("\n")) {
    const trimmed = line.replace(/^#+\s*/, "").trim();
    if (trimmed) return trimmed;
  }
  return null;
}

/** 「9 月 28 日更新」/「今天 18:40 更新」；没有时间戳时返回 null。 */
export function formatUpdated(
  t: TFunction,
  language: string,
  seconds: number | undefined,
  now: Date = new Date(),
): string | null {
  if (!seconds) return null;
  const date = new Date(seconds * 1000);
  if (Number.isNaN(date.getTime())) return null;
  if (date.toDateString() === now.toDateString()) {
    const time = date.toLocaleTimeString(language, {
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
    });
    return t("prompts.updatedToday", { time });
  }
  return t("prompts.updatedOn", {
    month: date.getMonth() + 1,
    day: date.getDate(),
    date: date.toLocaleDateString(language, {
      month: "short",
      day: "numeric",
      ...(date.getFullYear() !== now.getFullYear()
        ? { year: "numeric" as const }
        : {}),
    }),
  });
}

/**
 * 说明里的标识符（路径、文件名、斜杠命令）用等宽字；「名字」整体不断行。
 * 和画板 Prompts.dc.html 的 segs() 同一条规则。
 */
const SEG_RE =
  /(~[/\\][\w./\\-]*[\w/\\]|[A-Z_]+\.md|\/[a-z0-9][a-z0-9._-]*|「[^」]{1,24}」)/;
const SEG_WHOLE = new RegExp(`^${SEG_RE.source}$`);

export function renderSegs(text: string): ReactNode {
  return text
    .split(SEG_RE)
    .filter((part) => part !== "")
    .map((part, index) => {
      if (!SEG_WHOLE.test(part)) return <Fragment key={index}>{part}</Fragment>;
      if (part.startsWith("「")) {
        return (
          <span key={index} className="whitespace-nowrap">
            {part}
          </span>
        );
      }
      return (
        <code key={index} className="whitespace-nowrap font-mono text-caption">
          {part}
        </code>
      );
    });
}

export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

interface UndoToastOptions {
  title: string;
  description?: string;
  /** 不传就是普通的成功 toast */
  onUndo?: () => Promise<void>;
  /** 撤销成功后换上的确认 */
  undoneTitle?: string;
}

/**
 * 提示词页的结果 toast：第一行写做了什么，第二行写后果；能原样还原的带「撤销」。
 * toast 只有一个槽位的语义由 sonner 的 id 保证：新的顶掉旧的，旧的撤销随之失效。
 */
export function showPromptToast(t: TFunction, options: UndoToastOptions) {
  const { title, description, onUndo, undoneTitle } = options;
  if (!onUndo) {
    toast.success(title, {
      id: "prompts-result",
      description,
      closeButton: true,
    });
    return;
  }
  toast.success(title, {
    id: "prompts-result",
    description,
    closeButton: true,
    duration: UNDO_TOAST_DURATION,
    action: {
      label: t("prompts.undo"),
      onClick: () => {
        void onUndo().then(
          () => {
            if (undoneTitle) {
              toast.success(undoneTitle, {
                id: "prompts-result",
                closeButton: true,
              });
            } else {
              toast.dismiss("prompts-result");
            }
          },
          (error: unknown) => {
            toast.error(
              t("prompts.undoFailed", {
                reason: extractErrorMessage(error) || t("common.unknown"),
              }),
              { id: "prompts-result", closeButton: true },
            );
          },
        );
      },
    },
  });
}
