import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { systemPromptApi, type InjectionToggle } from "@/lib/api";
import type { AppId } from "@/lib/api";

// ===== 文件内容 =====

export const useSystemPromptFile = (appId: AppId) =>
  useQuery({
    queryKey: ["systemPromptFile", appId],
    queryFn: () => systemPromptApi.getFile(appId),
    staleTime: 0,
  });

export const useSaveSystemPromptFile = (appId: AppId) => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (content: string) => systemPromptApi.saveFile(appId, content),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["systemPromptFile", appId] });
    },
  });
};

// ===== 注入开关 =====

export const useInjectionToggle = (appId: AppId) =>
  useQuery({
    queryKey: ["injectionToggle", appId],
    queryFn: () => systemPromptApi.getToggle(appId),
    staleTime: 0,
  });

export const useSetInjectionToggle = (appId: AppId) => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (toggle: InjectionToggle) =>
      systemPromptApi.setToggle(appId, toggle),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["injectionToggle", appId] });
    },
  });
};

// ===== 共享规则 =====

export const useSharedPrompt = () =>
  useQuery({
    queryKey: ["sharedPrompt"],
    queryFn: () => systemPromptApi.getShared(),
    staleTime: 0,
  });

export const useSaveSharedPrompt = () => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (content: string) => systemPromptApi.saveShared(content),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["sharedPrompt"] });
    },
  });
};
