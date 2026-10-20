import { memo, useState, type ReactNode } from "react";
import { ChevronRight, ExternalLink } from "lucide-react";

import { toast } from "@/lib/toast";
import { cn } from "@/lib/utils";
import type { EventBlock, SessionMessage } from "@/types";
import { highlightText } from "../utils";
import { useReaderContext } from "./context";
import { useReaderT } from "./i18n";
import { GlyphCell } from "./rowStyles";
import { openSessionLink, SessionPlainText } from "./SessionMarkdown";
import { BlockText } from "./SessionStepDetails";
import type { Translate } from "./toolSummary";
import { messageText } from "./turns";

/** PR 编号：文案里的 `#128` 或链接里的 `/pull/128` */
const prNumber = (block: EventBlock) =>
  /#(\d+)/.exec(block.text ?? "")?.[1] ??
  /\/pull\/(\d+)/.exec(block.url ?? "")?.[1];

/** Codex 子代理事件 `agt_7Kq2: Found …` → 名称 + 正文 */
const splitSubAgent = (text: string) => {
  const match = /^([^:\s]{1,40}):\s*([\s\S]*)$/.exec(text);
  return match ? { name: match[1], rest: match[2] } : { name: text, rest: "" };
};

interface EventDisplay {
  label: string;
  /** 可展开的补充正文（压缩摘要、Hook 报错、子代理消息…） */
  detail?: string;
  tone: "muted" | "danger";
}

export const describeEvent = (
  block: EventBlock,
  rt: Translate,
  body?: string,
): EventDisplay => {
  const text = block.text?.trim() ?? "";
  const extra = [body?.trim()].filter(Boolean).join("\n\n");
  switch (block.kind) {
    case "aborted":
      return { label: rt("turnAborted"), tone: "muted" };
    case "compaction":
      return {
        label: rt("event.compaction"),
        detail: [text, extra].filter(Boolean).join("\n\n") || undefined,
        tone: "muted",
      };
    case "model_change":
      return { label: rt("event.modelChange", { model: text }), tone: "muted" };
    case "thinking_level":
      return {
        label: rt("event.thinkingLevel", { level: text }),
        tone: "muted",
      };
    case "hook":
      return {
        label: rt("event.hookError"),
        detail: text || undefined,
        tone: "danger",
      };
    case "pr_link": {
      const number = prNumber(block);
      return {
        label: number
          ? rt("event.prLink", { number })
          : rt("event.prLinkPlain"),
        tone: "muted",
      };
    }
    case "slash_command":
      return {
        label: rt("event.slashCommand", { command: text }),
        detail: extra || undefined,
        tone: "muted",
      };
    case "sub_agent": {
      const { name, rest } = splitSubAgent(text);
      return {
        label: rt("event.subAgent", { name }),
        detail: [rest, extra].filter(Boolean).join("\n\n") || undefined,
        tone: "muted",
      };
    }
    case "error":
      return { label: text, detail: extra || undefined, tone: "danger" };
    default:
      return { label: text, detail: extra || undefined, tone: "muted" };
  }
};

const LinkButton = ({
  url,
  children,
}: {
  url: string;
  children: ReactNode;
}) => {
  const rt = useReaderT();
  return (
    <button
      type="button"
      title={url}
      aria-label={rt("link.openExternal", { url })}
      onClick={() => {
        openSessionLink(url).catch((error: unknown) =>
          toast.error(error instanceof Error ? error.message : String(error)),
        );
      }}
      className="inline-flex items-center gap-1 rounded-[2px] text-action-text underline-offset-[3px] hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
    >
      {children}
      <ExternalLink aria-hidden className="h-3 w-3" />
    </button>
  );
};

export interface SessionEventRowProps {
  block: EventBlock;
  body?: string;
  /** divider：轮间居中细字；inline：执行过程里的一行 */
  variant?: "divider" | "inline";
}

/**
 * 事件行：中断 / 压缩 / 模型切换 / PR 链接 / Hook 错误 / 斜杠命令 / 子代理（§6.4 Event）。
 * Hook 错误与 Gemini error 用 danger 色；有补充正文的可点开。
 */
export const SessionEventRow = memo(function SessionEventRow({
  block,
  body,
  variant = "divider",
}: SessionEventRowProps) {
  const rt = useReaderT();
  const { searchQuery } = useReaderContext();
  const [open, setOpen] = useState(false);
  const display = describeEvent(block, rt, body);
  const tone = display.tone === "danger" ? "text-danger-text" : "text-fg-2";
  const label = searchQuery
    ? highlightText(display.label, searchQuery)
    : display.label;
  const content =
    block.kind === "pr_link" && block.url ? (
      <LinkButton url={block.url}>{label}</LinkButton>
    ) : display.detail ? (
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        className="inline-flex min-w-0 items-center gap-0.5 rounded-control px-1 hover:bg-subtle focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <span className="truncate">{label}</span>
        <ChevronRight
          aria-hidden
          className={cn(
            "h-3 w-3 shrink-0 transition-transform motion-reduce:transition-none",
            open && "rotate-90",
          )}
        />
      </button>
    ) : (
      <span className="truncate">{label}</span>
    );

  const detail = open && display.detail && (
    <div
      className={cn(
        "mt-1 rounded-[8px] bg-subtle px-3 py-2 text-left text-caption",
        tone,
      )}
    >
      {block.full ? (
        // 长摘要只下发了预览，「显示全部」时按引用取全文
        <BlockText
          preview={display.detail}
          truncated
          full={block.full}
          previewLines={Number.MAX_SAFE_INTEGER}
          variant="plain"
          className="text-inherit"
        />
      ) : (
        <SessionPlainText
          content={display.detail}
          searchQuery={searchQuery}
          className="text-caption text-inherit"
        />
      )}
    </div>
  );

  if (variant === "inline") {
    return (
      <div className="min-w-0 px-1.5 py-1">
        <div
          className={cn(
            "flex h-6 min-w-0 items-center gap-2 text-caption",
            tone,
          )}
        >
          <GlyphCell glyph="·" className="text-fg-3" />
          {content}
        </div>
        {detail && <div className="ps-6">{detail}</div>}
      </div>
    );
  }

  return (
    <div className="min-w-0 py-2">
      <div
        role="note"
        className={cn(
          "flex min-w-0 items-center justify-center gap-2.5 text-caption",
          tone,
        )}
      >
        <span aria-hidden className="h-px w-6 shrink-0 bg-border-strong" />
        <span className="flex min-w-0 max-w-[80%] items-center">{content}</span>
        <span aria-hidden className="h-px w-6 shrink-0 bg-border-strong" />
      </div>
      {detail && <div className="mx-auto max-w-[640px]">{detail}</div>}
    </div>
  );
});

// ─── 注入的上下文（决策 D10：默认隐藏，菜单开关显示） ────────────────────

/** 注入内容的标签：AGENTS.md / environment_context / system-reminder / developer… */
export const injectedLabel = (message: SessionMessage) => {
  const text = messageText(message).trim();
  const agents = /^#\s*(AGENTS\.md|CLAUDE\.md|GEMINI\.md)\b/.exec(text);
  if (agents) return agents[1];
  const tag = /^<([a-zA-Z][\w-]*)/.exec(text);
  if (tag) return tag[1];
  if (message.role !== "user") return message.role;
  const first = text.split("\n")[0] ?? "";
  return first.length > 32 ? `${first.slice(0, 32)}…` : first;
};

export const SessionInjectedRow = memo(function SessionInjectedRow({
  message,
}: {
  message: SessionMessage;
}) {
  const rt = useReaderT();
  const { searchQuery } = useReaderContext();
  const [open, setOpen] = useState(false);
  const texts = (message.blocks ?? []).flatMap((block) =>
    block.type === "text" ? [block] : [],
  );
  const text = messageText(message);
  // 超长注入文本只下发了预览：字数标「+」，展开后可按引用取全文
  const partial = texts.some((block) => block.full);
  const chars = `${text.length.toLocaleString()}${partial ? "+" : ""}`;
  return (
    <div className="min-w-0 px-1.5 py-1">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        className="inline-flex h-6 max-w-full items-center gap-1 rounded-control pe-1.5 ps-1 text-caption text-fg-3 transition-colors hover:bg-subtle hover:text-fg-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <ChevronRight
          aria-hidden
          className={cn(
            "h-3.5 w-3.5 shrink-0 transition-transform motion-reduce:transition-none",
            open && "rotate-90",
          )}
        />
        <span className="truncate">
          {rt("injectedRow", {
            label: injectedLabel(message),
            chars,
          })}
        </span>
      </button>
      {open && (
        <div className="mt-1 max-h-[480px] overflow-auto rounded-[8px] border border-border px-3 py-2">
          {partial ? (
            texts.map((block, index) => (
              <BlockText
                key={index}
                preview={block.text}
                truncated={Boolean(block.full)}
                full={block.full}
                previewLines={Number.MAX_SAFE_INTEGER}
                variant="plain"
                className={index > 0 ? "mt-3" : undefined}
              />
            ))
          ) : (
            <SessionPlainText
              content={text}
              searchQuery={searchQuery}
              className="text-caption text-fg-2"
            />
          )}
        </div>
      )}
    </div>
  );
});
