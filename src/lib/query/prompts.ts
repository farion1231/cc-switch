import { useQuery } from "@tanstack/react-query";
import { appHas } from "@/config/appSpecs";
import {
  promptsApi,
  type AppId,
  type GlobalPromptEntry,
  type Prompt,
} from "@/lib/api";

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

/** 应用页对应的提示词应用；不支持提示词的返回 null。 */
export function promptAppOf(app: AppId): AppId | null {
  if (app === "claude-desktop") return "claude";
  return appHas(app, "prompts") ? app : null;
}

export function usePromptListQuery(app: AppId | null, enabled = true) {
  return useQuery({
    queryKey: promptKeys.list(app ?? "claude"),
    queryFn: () => promptsApi.getPrompts(app as AppId),
    enabled: enabled && app !== null,
  });
}

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

export function enabledPromptOf(
  prompts: Record<string, Prompt> | undefined,
): Prompt | undefined {
  if (!prompts) return undefined;
  return Object.values(prompts).find((prompt) => prompt.enabled);
}
