import { useState, useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { promptsApi, type Prompt, type AppId } from "@/lib/api";

const EMPTY_PROMPTS: Record<string, Prompt> = {};

export function usePromptActions(appId: AppId) {
  const { t } = useTranslation();
  const [prompts, setPrompts] = useState<Record<string, Prompt>>({});
  const [promptsAppId, setPromptsAppId] = useState<AppId | null>(null);
  const [loading, setLoading] = useState(false);
  const [currentFileContent, setCurrentFileContent] = useState<string | null>(
    null,
  );
  const [currentFileAppId, setCurrentFileAppId] = useState<AppId | null>(null);
  const reloadGenerationRef = useRef(0);
  const currentAppIdRef = useRef(appId);
  const promptsAppIdRef = useRef<AppId | null>(null);
  currentAppIdRef.current = appId;

  const visiblePrompts = promptsAppId === appId ? prompts : EMPTY_PROMPTS;
  const visibleCurrentFileContent =
    currentFileAppId === appId ? currentFileContent : null;

  const updatePromptsForApp = useCallback(
    (
      targetAppId: AppId,
      updater: (current: Record<string, Prompt>) => Record<string, Prompt>,
    ) => {
      if (currentAppIdRef.current !== targetAppId) return;

      const previousAppId = promptsAppIdRef.current;
      setPrompts((current) =>
        updater(previousAppId === targetAppId ? current : EMPTY_PROMPTS),
      );
      promptsAppIdRef.current = targetAppId;
      setPromptsAppId(targetAppId);
    },
    [],
  );

  useEffect(
    () => () => {
      reloadGenerationRef.current += 1;
    },
    [],
  );

  const reload = useCallback(async (): Promise<boolean> => {
    const requestAppId = appId;
    if (currentAppIdRef.current !== requestAppId) return false;

    const requestGeneration = ++reloadGenerationRef.current;
    const isCurrentRequest = () =>
      reloadGenerationRef.current === requestGeneration &&
      currentAppIdRef.current === requestAppId;

    setLoading(true);
    try {
      const data = await promptsApi.getPrompts(requestAppId);
      if (!isCurrentRequest()) return false;
      updatePromptsForApp(requestAppId, () => data);

      // 同时加载当前文件内容
      try {
        const content = await promptsApi.getCurrentFileContent(requestAppId);
        if (!isCurrentRequest()) return false;
        setCurrentFileContent(content);
        setCurrentFileAppId(requestAppId);
      } catch (error) {
        if (isCurrentRequest()) {
          setCurrentFileContent(null);
          setCurrentFileAppId(requestAppId);
        }
      }
      return true;
    } catch (error) {
      if (isCurrentRequest()) {
        toast.error(t("prompts.loadFailed"));
      }
      return false;
    } finally {
      if (isCurrentRequest()) {
        setLoading(false);
      }
    }
  }, [appId, t, updatePromptsForApp]);

  const savePrompt = useCallback(
    async (id: string, prompt: Prompt) => {
      try {
        await promptsApi.upsertPrompt(appId, id, prompt);
        updatePromptsForApp(appId, (current) => ({
          ...current,
          [id]: prompt,
        }));
        const refreshed =
          currentAppIdRef.current === appId ? await reload() : false;
        toast.success(t("prompts.saveSuccess"), { closeButton: true });
        return refreshed;
      } catch (error) {
        toast.error(t("prompts.saveFailed"));
        throw error;
      }
    },
    [appId, reload, t, updatePromptsForApp],
  );

  const deletePrompt = useCallback(
    async (id: string) => {
      try {
        await promptsApi.deletePrompt(appId, id);
        updatePromptsForApp(appId, (current) => {
          const next = { ...current };
          delete next[id];
          return next;
        });
        const refreshed =
          currentAppIdRef.current === appId ? await reload() : false;
        toast.success(t("prompts.deleteSuccess"), { closeButton: true });
        return refreshed;
      } catch (error) {
        toast.error(t("prompts.deleteFailed"));
        throw error;
      }
    },
    [appId, reload, t, updatePromptsForApp],
  );

  const enablePrompt = useCallback(
    async (id: string) => {
      try {
        await promptsApi.enablePrompt(appId, id);
        updatePromptsForApp(appId, (current) =>
          Object.fromEntries(
            Object.entries(current).map(([key, prompt]) => [
              key,
              { ...prompt, enabled: key === id },
            ]),
          ),
        );
        const refreshed =
          currentAppIdRef.current === appId ? await reload() : false;
        toast.success(t("prompts.enableSuccess"), { closeButton: true });
        return refreshed;
      } catch (error) {
        toast.error(t("prompts.enableFailed"));
        throw error;
      }
    },
    [appId, reload, t, updatePromptsForApp],
  );

  const toggleEnabled = useCallback(
    async (id: string, enabled: boolean) => {
      // Optimistic update
      const previousPrompts = visiblePrompts;
      const mutationGeneration = reloadGenerationRef.current;

      // 如果要启用当前提示词，先禁用其他所有提示词
      if (enabled) {
        const updatedPrompts = Object.keys(visiblePrompts).reduce(
          (acc, key) => {
            acc[key] = {
              ...visiblePrompts[key],
              enabled: key === id,
            };
            return acc;
          },
          {} as Record<string, Prompt>,
        );
        updatePromptsForApp(appId, () => updatedPrompts);
      } else {
        updatePromptsForApp(appId, (current) => ({
          ...current,
          [id]: {
            ...current[id],
            enabled: false,
          },
        }));
      }

      try {
        if (enabled) {
          await promptsApi.enablePrompt(appId, id);
          toast.success(t("prompts.enableSuccess"), { closeButton: true });
        } else {
          // 禁用提示词 - 需要后端支持
          await promptsApi.upsertPrompt(appId, id, {
            ...visiblePrompts[id],
            enabled: false,
          });
          toast.success(t("prompts.disableSuccess"), { closeButton: true });
        }
        return currentAppIdRef.current === appId ? await reload() : false;
      } catch (error) {
        // Rollback on failure
        if (
          currentAppIdRef.current === appId &&
          reloadGenerationRef.current === mutationGeneration
        ) {
          updatePromptsForApp(appId, () => previousPrompts);
        }
        toast.error(
          enabled ? t("prompts.enableFailed") : t("prompts.disableFailed"),
        );
        throw error;
      }
    },
    [appId, reload, t, updatePromptsForApp, visiblePrompts],
  );

  const importFromFile = useCallback(async () => {
    try {
      const id = await promptsApi.importFromFile(appId);
      if (currentAppIdRef.current === appId) {
        await reload();
      }
      toast.success(t("prompts.importSuccess"), { closeButton: true });
      return id;
    } catch (error) {
      toast.error(t("prompts.importFailed"));
      throw error;
    }
  }, [appId, reload, t]);

  return {
    prompts: visiblePrompts,
    loading,
    currentFileContent: visibleCurrentFileContent,
    reload,
    savePrompt,
    deletePrompt,
    enablePrompt,
    toggleEnabled,
    importFromFile,
  };
}
