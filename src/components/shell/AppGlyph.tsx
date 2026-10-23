import { Monitor, Terminal } from "lucide-react";
import type { AppId } from "@/lib/api";
import { ProviderIcon } from "@/components/ProviderIcon";
import { cn } from "@/lib/utils";

/**
 * 只存在于用量统计里的应用身份：DSH 的用量来自会话日志，CC Switch 不管理它的
 * 供应商配置，所以它不进 `AppId`（侧栏、供应商页、MCP、Skills 都不该出现），
 * 只作为 `app_type` 出现在用量面板里。
 */
export type UsageOnlyAppId = "dsh";

/** 用量面板能画图标的应用：CC Switch 管理的应用 + 只在用量里出现的应用。 */
export type AppGlyphId = AppId | UsageOnlyAppId;

/** 应用名：品牌名不翻译，四种语言都写原文。 */
export const APP_DISPLAY_NAME: Record<AppGlyphId, string> = {
  claude: "Claude Code",
  "claude-desktop": "Claude Desktop",
  codex: "Codex",
  gemini: "Gemini CLI",
  grokbuild: "Grok Build",
  opencode: "OpenCode",
  openclaw: "OpenClaw",
  hermes: "Hermes",
  pi: "Pi",
  mcode: "MiniMax Code",
  kimi: "Kimi Code",
  agy: "Antigravity",
  dsh: "DeepSeek Harness",
};

const APP_ICON_NAME: Record<AppGlyphId, string> = {
  claude: "claude",
  "claude-desktop": "claude",
  codex: "openai",
  gemini: "gemini",
  grokbuild: "grok",
  opencode: "opencode",
  openclaw: "openclaw",
  hermes: "hermes",
  pi: "pi",
  mcode: "minimax",
  kimi: "kimi",
  agy: "antigravity",
  dsh: "deepseek",
};

// Claude Code 和 Claude Desktop 用同一个图标，靠右下角的小角标区分终端与桌面
const APP_BADGE_ICON: Partial<Record<AppGlyphId, typeof Terminal>> = {
  claude: Terminal,
  "claude-desktop": Monitor,
};

interface AppGlyphProps {
  app: AppGlyphId;
  /** 图标边长：侧栏 16，页头 20 */
  size?: number;
  /** 角标底色要和所在的行一致，否则看起来像缺了一块 */
  badgeClassName?: string;
  className?: string;
}

/** 应用图标 + 角标（装饰性，名称由旁边的文字或按钮的 aria-label 提供）。 */
export function AppGlyph({
  app,
  size = 16,
  badgeClassName,
  className,
}: AppGlyphProps) {
  const BadgeIcon = APP_BADGE_ICON[app];
  return (
    <span
      aria-hidden="true"
      className={cn(
        "relative inline-flex shrink-0 items-center justify-center",
        className,
      )}
      style={{ width: size + 2, height: size + 2 }}
    >
      <ProviderIcon
        icon={APP_ICON_NAME[app]}
        name=""
        size={size}
        showFallback={false}
      />
      {BadgeIcon && (
        <span
          className={cn(
            "absolute -bottom-1 -right-1 flex h-[11px] w-[11px] items-center justify-center rounded-[3px] bg-sidebar text-fg-2",
            badgeClassName,
          )}
        >
          <BadgeIcon className="h-[9px] w-[9px]" strokeWidth={3} />
        </span>
      )}
    </span>
  );
}
