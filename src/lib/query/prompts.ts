import { useQuery } from "@tanstack/react-query";
import { promptsApi, type AppId } from "@/lib/api";
import { APP_IDS_WITH } from "@/config/appSpecs";

/**
 * 提示词查询的 key。提示词页自己的列表走 usePromptActions；写完后 invalidate
 * `promptKeys.all`，其余读提示词的查询跟着刷新。
 */
export const promptKeys = {
  all: ["prompts"] as const,
  list: (app: AppId) => ["prompts", "list", app] as const,
  location: (app: AppId) => ["prompts", "location", app] as const,
};

/**
 * 提示词实际落盘的应用：Claude Desktop 复用 Claude Code 的提示词文件，
 * 其余应用只有能力表里 `prompts` 为 true 才能管理提示词。
 */
export function promptAppOf(app: AppId): AppId | null {
  if (app === "claude-desktop") return "claude";
  return APP_IDS_WITH.prompts.some((candidate) => candidate === app)
    ? app
    : null;
}

export function usePromptFileLocationQuery(app: AppId) {
  return useQuery({
    queryKey: promptKeys.location(app),
    queryFn: () => promptsApi.getFileLocation(app),
    staleTime: 30_000,
  });
}
