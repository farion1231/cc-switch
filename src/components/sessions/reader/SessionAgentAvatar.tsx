import { memo } from "react";

import { AppGlyph } from "@/components/shell/AppGlyph";
import { isSessionAppId } from "../utils";
import { useReaderContext } from "./context";

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
