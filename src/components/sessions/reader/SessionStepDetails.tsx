import { memo, useMemo, useState, type ReactNode } from "react";
import { ChevronRight } from "lucide-react";

import { useBlockContent } from "@/lib/query/sessions";
import { cn } from "@/lib/utils";
import type { ContentRef, ToolCallBlock, ToolStatus } from "@/types";
import { highlightText } from "../utils";
import type { AgentReaderStyle } from "./agentStyles";
import { useReaderContext } from "./context";
import { useReaderT } from "./i18n";
import { PathChip } from "./PathChip";
import { SessionCodeBlock } from "./SessionCodeBlock";
import { buildInputDiff, SessionDiff } from "./SessionDiff";
import { SessionImageGrid } from "./SessionImage";
import { isFailureStatus, shortenPath } from "./toolSummary";
import type { ToolStep } from "./turns";

const linkButton =
  "rounded-[2px] text-caption text-fg-2 underline decoration-border-strong underline-offset-[3px] transition-colors hover:text-fg-1 hover:decoration-fg-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";

/** 状态色：Gemini 的框线、Pi 的竖条用 */
export const STATUS_BORDER: Record<ToolStatus, string> = {
  success: "border-success",
  error: "border-danger",
  interrupted: "border-warning",
  pending: "border-border-strong",
  unknown: "border-border-strong",
};

/** 同上，只给起始边（Gemini 的框线左侧加粗着色）；Tailwind 需要完整类名字面量 */
const STATUS_BORDER_START: Record<ToolStatus, string> = {
  success: "border-s-success",
  error: "border-s-danger",
  interrupted: "border-s-warning",
  pending: "border-s-border-strong",
  unknown: "border-s-border-strong",
};

// ─── 可按需取全文的文本块 ─────────────────────────────────────────────────

export interface BlockTextProps {
  /** 预览文本（后端已截断到 12 行 / 1200 字） */
  preview: string;
  /** 全文行数；缺省按预览计 */
  lineCount?: number;
  /** 预览之后还有内容，需要按 full 取全文 */
  truncated: boolean;
  full?: ContentRef;
  /** 折叠时显示的行数 */
  previewLines: number;
  variant: "output" | "params" | "thinking" | "plain";
  /** 自定义全文展示（参数 JSON 美化等） */
  format?: (text: string) => string;
  /** 失败输出用正文色，其余次级色 */
  emphasis?: boolean;
  className?: string;
}

const VARIANT_CLASS = {
  output:
    "whitespace-pre rounded-[8px] bg-subtle px-3 py-2 font-mono text-caption",
  params:
    "whitespace-pre rounded-[8px] bg-subtle px-3 py-2 font-mono text-caption",
  thinking: "whitespace-pre-wrap break-words text-caption italic",
  /** 注入文本、压缩摘要等长说明 */
  plain: "whitespace-pre-wrap break-words text-caption",
} as const;

/**
 * 输出 / 参数 / 思考的文本块：默认显示前 N 行，「显示全部」时预览已完整就直接展开，
 * 否则按 ContentRef 取全文（512K 一页，「加载更多」；累计超过 2MB 提示复制源文件路径）。
 */
export const BlockText = memo(function BlockText({
  preview,
  lineCount,
  truncated,
  full,
  previewLines,
  variant,
  format,
  emphasis,
  className,
}: BlockTextProps) {
  const rt = useReaderT();
  const { providerId, sourcePath, searchQuery, onCopy } = useReaderContext();
  const [showAll, setShowAll] = useState(false);
  const needsFetch = truncated && Boolean(full);
  const content = useBlockContent(providerId, sourcePath, full, {
    enabled: showAll && needsFetch,
  });

  const previewRows = useMemo(() => preview.split("\n"), [preview]);
  const totalLines = Math.max(lineCount ?? 0, previewRows.length);
  const fullText = showAll && needsFetch && content.text ? content.text : null;
  let shown: string;
  if (fullText !== null) {
    shown = fullText;
  } else if (showAll) {
    shown = preview;
  } else {
    shown = previewRows.slice(0, previewLines).join("\n");
  }
  if (format) shown = format(shown);
  const hiddenLines = showAll
    ? 0
    : totalLines - Math.min(previewLines, previewRows.length);
  const canExpand = hiddenLines > 0 || (truncated && Boolean(full));

  return (
    <div className={cn("min-w-0", className)}>
      <pre
        tabIndex={variant === "thinking" || variant === "plain" ? undefined : 0}
        className={cn(
          "m-0 max-h-[480px] overflow-auto outline-none focus-visible:ring-2 focus-visible:ring-ring",
          VARIANT_CLASS[variant],
          emphasis ? "text-fg-1" : "text-fg-2",
        )}
      >
        {searchQuery ? highlightText(shown, searchQuery) : shown}
        {!showAll && hiddenLines <= 0 && truncated && "…"}
      </pre>
      {(canExpand || showAll) && (
        <div className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-caption text-fg-3">
          {!showAll && hiddenLines > 0 && (
            <span className="tabular-nums">
              …{rt("moreLines", { count: hiddenLines })}
            </span>
          )}
          {!showAll && (
            <button
              type="button"
              onClick={() => setShowAll(true)}
              className={linkButton}
            >
              {rt("showAll")}
            </button>
          )}
          {showAll && needsFetch && content.isLoading && (
            <span role="status">{rt("loading")}</span>
          )}
          {showAll && needsFetch && Boolean(content.error) && (
            <span className="text-danger-text">{rt("loadFailed")}</span>
          )}
          {showAll && content.hasMore && (
            <button
              type="button"
              disabled={content.isFetchingMore}
              onClick={content.loadMore}
              className={linkButton}
            >
              {rt("loadMore")}
            </button>
          )}
          {showAll && content.tooLarge && (
            <>
              <span>{rt("tooLarge")}</span>
              {sourcePath && (
                <button
                  type="button"
                  onClick={() =>
                    onCopy(
                      sourcePath,
                      rt("sessionManager.sourcePathCopied", {
                        defaultValue: "已复制源文件路径",
                      }),
                    )
                  }
                  className={linkButton}
                >
                  {rt("sessionManager.copySourcePath", {
                    defaultValue: "复制源文件路径",
                  })}
                </button>
              )}
            </>
          )}
          {showAll && (
            <button
              type="button"
              onClick={() => setShowAll(false)}
              className={linkButton}
            >
              {rt("collapseContent")}
            </button>
          )}
        </div>
      )}
    </div>
  );
});

// ─── 参数 ─────────────────────────────────────────────────────────────────

const prettyJson = (text: string) => {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
};

/** 从完整的参数 JSON 里取 shell 命令全文（多行命令的标题只有首行） */
const fullCommand = (call: ToolCallBlock) => {
  if (call.inputPreview.length < call.inputTotalLen) return null;
  try {
    const input = JSON.parse(call.inputPreview) as Record<string, unknown>;
    for (const key of ["command", "cmd", "script"]) {
      const value = input[key];
      if (typeof value === "string") return value;
      if (Array.isArray(value) && value.every((v) => typeof v === "string")) {
        return value.join(" ");
      }
    }
  } catch {
    // 不是 JSON（Codex exec 的 JS 源码等）：没有可取的命令
  }
  return null;
};

const Section = ({
  label,
  children,
  collapsible,
  defaultOpen = true,
}: {
  label: string;
  children: ReactNode;
  collapsible?: boolean;
  defaultOpen?: boolean;
}) => {
  const [open, setOpen] = useState(defaultOpen);
  if (!collapsible) {
    return (
      <section className="min-w-0">
        <h4 className="m-0 mb-1 text-caption font-medium text-fg-3">{label}</h4>
        {children}
      </section>
    );
  }
  return (
    <section className="min-w-0">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        className="-ms-1 mb-1 inline-flex h-6 items-center gap-0.5 rounded-control pe-1.5 ps-0.5 text-caption font-medium text-fg-3 transition-colors hover:bg-subtle hover:text-fg-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <ChevronRight
          aria-hidden
          className={cn(
            "h-3.5 w-3.5 transition-transform motion-reduce:transition-none",
            open && "rotate-90",
          )}
        />
        {label}
      </button>
      {open && children}
    </section>
  );
};

// ─── 改动 ─────────────────────────────────────────────────────────────────

const DiffFromRef = ({
  diffRef,
  formatPath,
}: {
  diffRef: ContentRef;
  formatPath: (path: string) => string;
}) => {
  const rt = useReaderT();
  const { providerId, sourcePath, searchQuery } = useReaderContext();
  const content = useBlockContent(providerId, sourcePath, diffRef);
  if (content.isLoading) {
    return (
      <div className="h-16 rounded-[8px] border border-border bg-subtle motion-safe:animate-pulse" />
    );
  }
  if (content.error || !content.text) {
    return <p className="m-0 text-caption text-fg-3">{rt("loadFailed")}</p>;
  }
  return (
    <SessionDiff
      text={content.text}
      searchQuery={searchQuery}
      formatPath={formatPath}
    />
  );
};

const DiffSection = ({ call }: { call: ToolCallBlock }) => {
  const rt = useReaderT();
  const { projectDir, searchQuery } = useReaderContext();
  const formatPath = (path: string) => shortenPath(path, projectDir);
  const diff = call.diff;
  const inputDiff = useMemo(
    () => (diff?.full ? null : buildInputDiff(call)),
    [call, diff?.full],
  );
  if (!diff && !inputDiff) return null;

  return (
    <Section label={rt("diff")}>
      {diff && diff.files.length > 1 && (
        <ul className="m-0 mb-1.5 list-none space-y-0.5 p-0 font-mono text-caption text-fg-2">
          {diff.files.map((file) => (
            <li key={file.path} className="flex items-center gap-1.5">
              <span aria-hidden className="text-fg-3">
                └
              </span>
              <span className="min-w-0 truncate">{formatPath(file.path)}</span>
              <span className="shrink-0 tabular-nums">
                (<span className="text-success-text">+{file.added}</span>{" "}
                <span className="text-danger-text">−{file.removed}</span>)
              </span>
            </li>
          ))}
        </ul>
      )}
      {diff?.full ? (
        <DiffFromRef diffRef={diff.full} formatPath={formatPath} />
      ) : inputDiff ? (
        <SessionDiff
          text={inputDiff}
          searchQuery={searchQuery}
          formatPath={formatPath}
        />
      ) : null}
    </Section>
  );
};

// ─── 详情 ─────────────────────────────────────────────────────────────────

export interface SessionStepDetailsProps {
  step: ToolStep;
  style: AgentReaderStyle;
  /** 输出预览行数（失败自动展开 6，其余 12） */
  previewLines: number;
  id: string;
  label: string;
}

/**
 * 步骤展开区（规则 6、7）：参数（失败时展开，成功时折叠在「参数」下；shell 显示完整命令）、
 * 改动、输出预览 + 显示全部、结果图片、落盘路径。结果引出符随风格：tree / box / none。
 */
export const SessionStepDetails = memo(function SessionStepDetails({
  step,
  style,
  previewLines,
  id,
  label,
}: SessionStepDetailsProps) {
  const rt = useReaderT();
  const { projectDir } = useReaderContext();
  const { call, result, status } = step;
  const failure = isFailureStatus(status);

  const command = call?.kind === "shell" ? fullCommand(call) : null;
  const showCommand = Boolean(command && command.trim() !== call?.title.trim());
  // shell 一般直接显示完整命令；但参数超过预览上限时取不到全文，
  // 这时也给出「参数」区，按需加载完整参数（否则只剩最多 200 字的标题）
  const shellTruncated =
    call?.kind === "shell" && call.inputPreview.length < call.inputTotalLen;
  const hasParams =
    Boolean(call) &&
    call!.inputPreview.trim() !== "" &&
    (call!.kind !== "shell" || shellTruncated);

  const output = result && (result.preview || result.totalLen > 0) && (
    <BlockText
      preview={result.preview}
      lineCount={result.lineCount}
      truncated={result.truncated}
      full={result.full}
      previewLines={previewLines}
      variant="output"
      emphasis={failure}
    />
  );

  const framed =
    style.resultFrame === "tree" && output ? (
      <div className="flex min-w-0 gap-1.5">
        <span
          aria-hidden
          className="w-4 shrink-0 select-none pt-1.5 text-center font-mono text-caption leading-none text-fg-3"
        >
          {style.resultConnector}
        </span>
        <div className="min-w-0 flex-1">{output}</div>
      </div>
    ) : (
      output
    );

  return (
    <div
      id={id}
      role="region"
      aria-label={label}
      className={cn(
        "min-w-0 space-y-2 pb-2 pt-0.5",
        style.resultFrame === "box"
          ? cn(
              "ms-6 rounded-[8px] border border-s-2 border-border p-2.5",
              STATUS_BORDER_START[status],
            )
          : style.resultFrame === "none"
            ? cn("ms-6 border-s-2 ps-3", STATUS_BORDER[status])
            : "ps-6",
      )}
    >
      {showCommand && command && (
        <SessionCodeBlock code={command} language="sh" className="my-0" />
      )}
      {hasParams && call && (
        <Section
          label={rt("params")}
          collapsible
          defaultOpen={failure || !result || shellTruncated}
        >
          <BlockText
            preview={call.inputPreview}
            truncated={call.inputPreview.length < call.inputTotalLen}
            full={call.inputFull}
            previewLines={24}
            variant="params"
            format={prettyJson}
          />
        </Section>
      )}
      {call && <DiffSection call={call} />}
      {framed}
      {result?.images && result.images.length > 0 && (
        <SessionImageGrid
          images={result.images}
          variant="result"
          className="ps-6"
        />
      )}
      {result?.savedPath && (
        <div className="flex items-center gap-1.5 text-caption text-fg-3">
          <span>{rt("output")}</span>
          <PathChip path={result.savedPath} projectDir={projectDir} />
        </div>
      )}
    </div>
  );
});
