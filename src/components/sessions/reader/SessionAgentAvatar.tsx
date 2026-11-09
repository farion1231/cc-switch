import { memo } from "react";

import type { SessionMessage } from "@/types";

import { AppGlyph } from "@/components/shell/AppGlyph";
import { formatMessageTime, isSessionAppId } from "../utils";
import { useReaderContext } from "./context";
import type { SessionTurn } from "./turns";

/**
 * 左侧 Agent 头像：每轮 Agent 输出的第一行左边显示一次。
 * 认识的来源用应用图标，其余退回风格里的 glyph 或 Agent 名首字。
 */
export const SessionAgentAvatar = memo(function SessionAgentAvatar() {
  const { providerId, style, appName } = useReaderContext();
  const known = providerId && isSessionAppId(providerId) ? providerId : null;
  const fallback = style.assistantGlyph || appName.trim().charAt(0) || "·";

  return (
    <span
      aria-hidden
      className="inline-flex h-7 w-7 shrink-0 select-none items-center justify-center rounded-full border border-border bg-surface"
    >
      {known ? (
        <AppGlyph app={known} size={16} badgeClassName="bg-surface" />
      ) : (
        <span className="font-mono text-caption font-semibold leading-none text-[var(--reader-accent)]">
          {fallback}
        </span>
      )}
    </span>
  );
});

/** 这一轮 Agent 输出的起始时间：第一步的消息时间，没有步骤时取最终回复时间 */
export const agentTurnTs = (
  turn: SessionTurn | undefined,
  messages: SessionMessage[],
): number | undefined => {
  if (!turn) return undefined;
  for (const step of turn.steps) {
    const index = "messageIndex" in step ? step.messageIndex : undefined;
    const ts = index !== undefined ? messages[index]?.ts : undefined;
    if (ts) return ts;
  }
  return turn.final?.ts;
};

/** 头像右侧的一行：Agent 名 · 时间 · 模型，高度和头像一致 */
export const SessionAgentHeader = memo(function SessionAgentHeader({
  ts,
  model,
}: {
  ts?: number;
  model?: string;
}) {
  const { appName } = useReaderContext();
  return (
    <div className="flex h-7 min-w-0 items-center gap-1.5 px-1.5 text-caption">
      <span className="shrink-0 font-semibold text-fg-1">{appName}</span>
      {ts ? (
        <time
          dateTime={new Date(ts).toISOString()}
          title={new Date(ts).toLocaleString()}
          className="shrink-0 tabular-nums text-fg-3"
        >
          {formatMessageTime(ts)}
        </time>
      ) : null}
      {model && (
        <span className="min-w-0 truncate font-mono text-fg-3">· {model}</span>
      )}
    </div>
  );
});
