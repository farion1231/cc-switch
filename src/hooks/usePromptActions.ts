import { useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { promptsApi, type Prompt, type AppId } from "@/lib/api";

export function usePromptActions(appId: AppId) {
  const { t } = useTranslation();
  const [prompts, setPrompts] = useState<Record<string, Prompt>>({});
  const [loading, setLoading] = useState(false);
  const [currentFileContent, setCurrentFileContent] = useState<string | null>(null);

  const reload = useCallback(async () => {
    setLoading(true);
    try {
      const data = await promptsApi.getPrompts(appId);
      setPrompts(data);

      // 同时加载当前文件内容
      try {
        const content = await promptsApi.getCurrentFileContent(appId);
        setCurrentFileContent(content);
      } catch (error) {
        setCurrentFileContent(null);
      }
    } catch (error) {
      toast.error(t("prompts.loadFailed"));
    } finally {
      setLoading(false);
    }
  }, [appId, t]);

  const savePrompt = useCallback(
    async (id: string, prompt: Prompt) => {
      try {
        await promptsApi.upsertPrompt(appId, id, prompt);
        await reload();
        toast.success(t("prompts.saveSuccess"));
      } catch (error) {
        toast.error(t("prompts.saveFailed"));
        throw error;
      }
    },
    [appId, reload, t]
  );

  const deletePrompt = useCallback(
    async (id: string) => {
      try {
        await promptsApi.deletePrompt(appId, id);
        await reload();
        toast.success(t("prompts.deleteSuccess"));
      } catch (error) {
        toast.error(t("prompts.deleteFailed"));
        throw error;
      }
    },
    [appId, reload, t]
  );

  const enablePrompt = useCallback(
    async (id: string) => {
      try {
        await promptsApi.enablePrompt(appId, id);
        await reload();
        toast.success(t("prompts.enableSuccess"));
      } catch (error) {
        toast.error(t("prompts.enableFailed"));
        throw error;
      }
    },
    [appId, reload, t]
  );

  const importFromFile = useCallback(async () => {
    try {
      const id = await promptsApi.importFromFile(appId);
      await reload();
      toast.success(t("prompts.importSuccess"));
      return id;
    } catch (error) {
      toast.error(t("prompts.importFailed"));
      throw error;
    }
  }, [appId, reload, t]);

  return {
    prompts,
    loading,
    currentFileContent,
    reload,
    savePrompt,
    deletePrompt,
    enablePrompt,
    importFromFile,
  };
}
