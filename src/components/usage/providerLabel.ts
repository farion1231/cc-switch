import type { TFunction } from "i18next";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import type { AppId } from "@/lib/api";

/**
 * 后端给会话日志导入的用量返回的占位供应商名（`usage_stats.rs` 的
 * `provider_name_coalesce`）。这些名字同时是供应商筛选的取值，原样回传后端
 * 精确匹配，所以只在显示时翻译，不能改动传给后端的值。
 */
const SESSION_PROVIDER_APPS: Record<string, AppId> = {
  "Claude (Session)": "claude",
  "Codex (Session)": "codex",
  "Gemini (Session)": "gemini",
  "OpenCode (Session)": "opencode",
  "Grok Build (Session)": "grokbuild",
  "MiniMax Code (Session)": "mcode",
  "Pi (Session)": "pi",
};

export interface UsageProviderLabel {
  label: string;
  /** 会话日志占位名才有：说明为什么分不出具体供应商 */
  hint?: string;
}

/** 用量面板里供应商名的显示文案；空名显示「未知供应商」。 */
export function getUsageProviderLabel(
  name: string | undefined,
  t: TFunction,
): UsageProviderLabel {
  if (!name) return { label: t("usage.unknownProvider") };
  const app = SESSION_PROVIDER_APPS[name];
  if (!app) return { label: name };
  return {
    label: t("usage.sessionProvider.label", { app: APP_DISPLAY_NAME[app] }),
    hint: t("usage.sessionProvider.hint"),
  };
}

/** 悬停提示：名字本身（截断时看全名），会话日志占位名再附上说明。 */
export function usageProviderTitle({ label, hint }: UsageProviderLabel) {
  return hint ? `${label}\n${hint}` : label;
}
