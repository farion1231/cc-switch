import { memo, useMemo } from "react";

import { cn } from "@/lib/utils";
import type { ToolCallBlock } from "@/types";
import { highlightText } from "../utils";

/**
 * 改动渲染：unified diff / apply_patch 文本 →
 * 行号 + `+` success-soft / `-` danger-soft，hunk 之间用 `⋮` 隔开，多文件各有小标题。
 */

export type DiffLine =
  | { type: "file"; path: string }
  | { type: "gap" }
  | {
      type: "add" | "del" | "context";
      text: string;
      oldNo?: number;
      newNo?: number;
    };

const HUNK_HEADER = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/;
const PATCH_FILE = /^\*\*\* (?:Add|Update|Delete) File: (.+)$/;

/** 解析 unified diff（git diff、Codex FileChange）与 Codex apply_patch 文本 */
export const parseDiff = (text: string): DiffLine[] => {
  const lines: DiffLine[] = [];
  let oldNo: number | undefined;
  let newNo: number | undefined;
  let hunks = 0;
  const pushFile = (path: string) => {
    const last = lines[lines.length - 1];
    if (last?.type === "file" && last.path === path) return;
    lines.push({ type: "file", path });
    hunks = 0;
  };

  text
    .replace(/\r\n/g, "\n")
    .split("\n")
    .forEach((raw, index, all) => {
      // `--- a` / `+++ b` 只在成对出现时算文件头，否则是内容以 `--`、`++` 开头的改动行
      const fileHeaderOld =
        raw.startsWith("--- ") && all[index + 1]?.startsWith("+++ ");
      const fileHeaderNew =
        raw.startsWith("+++ ") && all[index - 1]?.startsWith("--- ");
      if (raw.startsWith("diff --git ")) {
        hunks = 0;
        return;
      }
      if (
        (raw.startsWith("index ") && hunks === 0) ||
        fileHeaderOld ||
        raw.startsWith("*** Begin Patch") ||
        raw.startsWith("*** End Patch") ||
        raw.startsWith("\\ No newline")
      ) {
        return;
      }
      if (fileHeaderNew) {
        const path = raw.slice(4).replace(/^b\//, "").trim();
        if (path && path !== "/dev/null") pushFile(path);
        return;
      }
      const patchFile = PATCH_FILE.exec(raw);
      if (patchFile) {
        pushFile(patchFile[1].trim());
        oldNo = undefined;
        newNo = undefined;
        return;
      }
      if (raw.startsWith("*** Move to: ")) {
        pushFile(raw.slice("*** Move to: ".length).trim());
        return;
      }
      if (raw.startsWith("@@")) {
        if (hunks > 0) lines.push({ type: "gap" });
        hunks += 1;
        const match = HUNK_HEADER.exec(raw);
        oldNo = match ? Number(match[1]) : undefined;
        newNo = match ? Number(match[2]) : undefined;
        return;
      }
      if (raw.startsWith("+")) {
        lines.push({ type: "add", text: raw.slice(1), newNo });
        if (newNo !== undefined) newNo += 1;
        return;
      }
      if (raw.startsWith("-")) {
        lines.push({ type: "del", text: raw.slice(1), oldNo });
        if (oldNo !== undefined) oldNo += 1;
        return;
      }
      if (raw === "" && lines.length === 0) return;
      lines.push({
        type: "context",
        text: raw.startsWith(" ") ? raw.slice(1) : raw,
        oldNo,
        newNo,
      });
      if (oldNo !== undefined) oldNo += 1;
      if (newNo !== undefined) newNo += 1;
    });

  // 末尾的空上下文行（文本以换行结尾）不显示
  while (lines.length > 0) {
    const last = lines[lines.length - 1];
    if (last.type === "context" && last.text === "") lines.pop();
    else break;
  }
  return lines;
};

const pickString = (input: Record<string, unknown>, keys: string[]) => {
  for (const key of keys) {
    const value = input[key];
    if (typeof value === "string") return value;
  }
  return undefined;
};

const splitLines = (text: string) =>
  text === "" ? [] : text.replace(/\r\n/g, "\n").replace(/\n$/, "").split("\n");

/**
 * 没有 unified diff 全文时，从调用参数还原改动：Edit 类的 old/new 字符串、Write 类的 content。
 * 参数预览被截断（JSON 不完整）时返回 null。
 */
export const buildInputDiff = (call: ToolCallBlock): string | null => {
  if (call.kind !== "edit" && call.kind !== "write") return null;
  if (call.inputPreview.length < call.inputTotalLen) return null;
  let input: unknown;
  try {
    input = JSON.parse(call.inputPreview);
  } catch {
    return null;
  }
  if (!input || typeof input !== "object") return null;
  const record = input as Record<string, unknown>;
  const pairs: { old: string; next: string }[] = [];
  const edits = Array.isArray(record.edits) ? record.edits : [record];
  edits.forEach((edit) => {
    if (!edit || typeof edit !== "object") return;
    const item = edit as Record<string, unknown>;
    const oldText = pickString(item, [
      "old_string",
      "oldString",
      "oldText",
      "old_str",
    ]);
    const newText = pickString(item, [
      "new_string",
      "newString",
      "newText",
      "new_str",
    ]);
    if (oldText !== undefined || newText !== undefined) {
      pairs.push({ old: oldText ?? "", next: newText ?? "" });
    }
  });
  if (pairs.length === 0) {
    const content = pickString(record, ["content", "file_text", "text"]);
    if (content === undefined) return null;
    pairs.push({ old: "", next: content });
  }
  return pairs
    .map(
      ({ old, next }) =>
        `@@\n${[
          ...splitLines(old).map((line) => `-${line}`),
          ...splitLines(next).map((line) => `+${line}`),
        ].join("\n")}`,
    )
    .join("\n");
};

const ROW_TONE = {
  add: "bg-diff-add",
  del: "bg-diff-del",
  context: "",
} as const;

const SIGN = { add: "+", del: "−", context: " " } as const;

export interface SessionDiffProps {
  /** unified diff / apply_patch 文本 */
  text: string;
  searchQuery?: string;
  /** 文件小标题显示用（相对路径） */
  formatPath?: (path: string) => string;
  className?: string;
}

/** unified diff 渲染：只读、横向滚动、最高 480px */
export const SessionDiff = memo(function SessionDiff({
  text,
  searchQuery,
  formatPath,
  className,
}: SessionDiffProps) {
  const lines = useMemo(() => parseDiff(text), [text]);
  const hasNumbers = lines.some(
    (line) =>
      (line.type === "add" || line.type === "del" || line.type === "context") &&
      (line.oldNo !== undefined || line.newNo !== undefined),
  );

  return (
    <div
      tabIndex={0}
      className={cn(
        "max-h-[480px] overflow-auto rounded-[8px] border border-border bg-surface font-mono text-caption outline-none focus-visible:ring-2 focus-visible:ring-ring",
        className,
      )}
    >
      <div className="min-w-max py-1">
        {lines.map((line, index) => {
          if (line.type === "file") {
            return (
              <div
                key={index}
                className="sticky left-0 border-b border-border bg-subtle px-3 py-1 text-fg-2 [&:not(:first-child)]:mt-1 [&:not(:first-child)]:border-t"
              >
                {formatPath ? formatPath(line.path) : line.path}
              </div>
            );
          }
          if (line.type === "gap") {
            return (
              <div
                key={index}
                aria-hidden
                className="select-none px-3 leading-5 text-fg-3"
              >
                ⋮
              </div>
            );
          }
          return (
            <div
              key={index}
              className={cn("flex leading-5", ROW_TONE[line.type])}
            >
              {hasNumbers && (
                <>
                  <span className="w-10 shrink-0 select-none pe-2 text-right tabular-nums text-fg-3">
                    {line.type === "add" ? "" : (line.oldNo ?? "")}
                  </span>
                  <span className="w-10 shrink-0 select-none pe-2 text-right tabular-nums text-fg-3">
                    {line.type === "del" ? "" : (line.newNo ?? "")}
                  </span>
                </>
              )}
              <span
                aria-hidden
                className={cn(
                  "w-5 shrink-0 select-none text-center",
                  line.type === "add" && "text-success-text",
                  line.type === "del" && "text-danger-text",
                )}
              >
                {SIGN[line.type]}
              </span>
              <span className="sr-only">
                {line.type === "add" ? "+" : line.type === "del" ? "-" : ""}
              </span>
              <span className="whitespace-pre pe-3 text-fg-1">
                {searchQuery
                  ? highlightText(line.text, searchQuery)
                  : line.text}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
});
