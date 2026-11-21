import { useQuery } from "@tanstack/react-query";
import { promptsApi, type AppId, type GlobalPromptEntry } from "@/lib/api";

/**
 * 提示词查询的 key。提示词页自己的列表走 usePromptActions；写完后 invalidate
 * `promptKeys.all`，其余读提示词的查询跟着刷新。
 */
export const promptKeys = {
  all: ["prompts"] as const,
  list: (app: AppId) => ["prompts", "list", app] as const,
  location: (app: AppId) => ["prompts", "location", app] as const,
  global: ["prompts", "global"] as const,
};

/** 提示词页应用下拉里「全局提示词」那一项 */
export const GLOBAL_PROMPT_SCOPE = "global" as const;
export type PromptScope = AppId | typeof GLOBAL_PROMPT_SCOPE;

export function usePromptFileLocationQuery(app: AppId) {
  return useQuery({
    queryKey: promptKeys.location(app),
    queryFn: () => promptsApi.getFileLocation(app),
    staleTime: 30_000,
  });
}

/** 全局提示词列表（含每条在哪些应用里启用） */
export function useGlobalPromptsQuery(enabled = true) {
  return useQuery<GlobalPromptEntry[]>({
    queryKey: promptKeys.global,
    queryFn: () => promptsApi.getGlobalPrompts(),
    enabled,
  });
}
