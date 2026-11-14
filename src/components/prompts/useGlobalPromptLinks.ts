import { useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { toast } from "@/lib/toast";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import {
  globalIdOfLinked,
  promptsApi,
  type AppId,
  type GlobalPromptEntry,
  type GlobalPromptSaveResult,
  type Prompt,
} from "@/lib/api";
import { PROMPT_APP_IDS, useGlobalPromptsQuery } from "@/lib/query/prompts";
import { showPromptToast } from "./promptUtils";

/** 正在使用这条全局提示词的应用，按侧栏顺序 */
export function globalPromptApps(entry: GlobalPromptEntry): AppId[] {
  return PROMPT_APP_IDS.filter((app) => entry.apps[app]);
}

export function joinAppNames(t: TFunction, apps: AppId[]): string {
  return apps
    .map((app) => APP_DISPLAY_NAME[app])
    .join(t("prompts.listSeparator"));
}

/** 保存全局提示词后的 toast：成功写明同步到了哪些应用，个别应用失败单独报错。 */
export function reportGlobalSave(
  t: TFunction,
  name: string,
  syncedTo: AppId[],
  result: GlobalPromptSaveResult,
) {
  const failed = new Set(result.failures.map((failure) => failure.app));
  const synced = syncedTo.filter((app) => !failed.has(app));
  showPromptToast(t, {
    title: synced.length
      ? t("prompts.global.toast.savedSynced", {
          name,
          apps: joinAppNames(t, synced),
        })
      : t("prompts.toast.saved", { name }),
  });
  if (result.failures.length) {
    toast.error(
      t("prompts.global.toast.syncFailed", {
        apps: joinAppNames(t, [...failed]),
      }),
      { description: result.failures[0]?.error, closeButton: true },
    );
  }
}

/**
 * 应用列表里的全局提示词关联记录：认出哪些条目是全局的（显示徽标），
 * 编辑它们时改全局那份，以及把普通条目设为全局提示词。
 */
export function useGlobalPromptLinks(appId: AppId) {
  const { t } = useTranslation();
  const query = useGlobalPromptsQuery();
  const byId = useMemo(
    () => new Map((query.data ?? []).map((entry) => [entry.id, entry])),
    [query.data],
  );

  const globalOf = useCallback(
    (promptId: string): GlobalPromptEntry | undefined => {
      const globalId = globalIdOfLinked(promptId);
      return globalId ? byId.get(globalId) : undefined;
    },
    [byId],
  );

  const isGlobal = useCallback(
    (promptId: string) => globalOf(promptId) !== undefined,
    [globalOf],
  );

  /** 在应用列表里编辑关联记录：改全局那份，同步到所有使用它的应用 */
  const saveLinked = useCallback(
    async (entry: GlobalPromptEntry, prompt: Prompt) => {
      const result = await promptsApi.upsertGlobalPrompt({
        id: entry.id,
        name: prompt.name,
        content: prompt.content,
        description: prompt.description,
        createdAt: entry.createdAt,
      });
      reportGlobalSave(t, prompt.name, globalPromptApps(entry), result);
      return result;
    },
    [t],
  );

  const promote = useCallback(
    (promptId: string) => promptsApi.promoteToGlobal(appId, promptId),
    [appId],
  );

  return { globalOf, isGlobal, saveLinked, promote };
}
