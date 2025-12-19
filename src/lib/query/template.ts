import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { templateApi } from "@/lib/api/template";
import type { ComponentFilter, ComponentType } from "@/types/template";
import type { AppType } from "@/lib/api/config";

// Query keys
export const templateKeys = {
  all: ["templates"] as const,
  repos: () => [...templateKeys.all, "repos"] as const,
  components: (filter: ComponentFilter) =>
    [...templateKeys.all, "components", filter] as const,
  component: (id: number) => [...templateKeys.all, "component", id] as const,
  categories: (type?: ComponentType) =>
    [...templateKeys.all, "categories", type] as const,
  installed: (appType?: AppType, componentType?: ComponentType) =>
    [...templateKeys.all, "installed", appType, componentType] as const,
  preview: (id: number) => [...templateKeys.all, "preview", id] as const,
  bundles: () => [...templateKeys.all, "bundles"] as const,
};

// Hooks - 模板仓库
export function useTemplateRepos() {
  return useQuery({
    queryKey: templateKeys.repos(),
    queryFn: templateApi.listTemplateRepos,
  });
}

export function useAddTemplateRepo() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (params: { owner: string; name: string; branch: string }) =>
      templateApi.addTemplateRepo(params.owner, params.name, params.branch),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: templateKeys.repos() });
    },
  });
}

export function useRemoveTemplateRepo() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (id: number) => templateApi.removeTemplateRepo(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: templateKeys.repos() });
      queryClient.invalidateQueries({ queryKey: templateKeys.components({}) });
    },
  });
}

export function useToggleTemplateRepo() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (params: { id: number; enabled: boolean }) =>
      templateApi.toggleTemplateRepo(params.id, params.enabled),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: templateKeys.repos() });
    },
  });
}

// Hooks - 模板索引
export function useRefreshTemplateIndex() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: templateApi.refreshTemplateIndex,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: templateKeys.all });
    },
  });
}

// Hooks - 模板组件查询
export function useTemplateComponents(filter: ComponentFilter) {
  return useQuery({
    queryKey: templateKeys.components(filter),
    queryFn: () => templateApi.listTemplateComponents(filter),
  });
}

export function useTemplateComponent(id: number) {
  return useQuery({
    queryKey: templateKeys.component(id),
    queryFn: () => templateApi.getTemplateComponent(id),
    enabled: id > 0,
  });
}

export function useComponentCategories(componentType?: ComponentType) {
  return useQuery({
    queryKey: templateKeys.categories(componentType),
    queryFn: () => templateApi.getComponentCategories(componentType),
  });
}

// Hooks - 组件安装
export function useInstallTemplateComponent() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (params: { id: number; appType: AppType }) =>
      templateApi.installTemplateComponent(params.id, params.appType),
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: templateKeys.components({}),
      });
      queryClient.invalidateQueries({
        queryKey: templateKeys.installed(),
      });
    },
  });
}

export function useUninstallTemplateComponent() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (params: { id: number; appType: AppType }) =>
      templateApi.uninstallTemplateComponent(params.id, params.appType),
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: templateKeys.components({}),
      });
      queryClient.invalidateQueries({
        queryKey: templateKeys.installed(),
      });
    },
  });
}

export function useBatchInstallComponents() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (params: { ids: number[]; appType: AppType }) =>
      templateApi.batchInstallComponents(params.ids, params.appType),
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: templateKeys.components({}),
      });
      queryClient.invalidateQueries({
        queryKey: templateKeys.installed(),
      });
    },
  });
}

export function useInstalledComponents(
  appType?: AppType,
  componentType?: ComponentType,
) {
  return useQuery({
    queryKey: templateKeys.installed(appType, componentType),
    queryFn: () => templateApi.listInstalledComponents(appType, componentType),
  });
}

// Hooks - 组件预览
export function useComponentPreview(id: number) {
  return useQuery({
    queryKey: templateKeys.preview(id),
    queryFn: () => templateApi.previewComponentContent(id),
    enabled: id > 0,
  });
}

// Hooks - 市场组合
export function useMarketplaceBundles() {
  return useQuery({
    queryKey: templateKeys.bundles(),
    queryFn: templateApi.listMarketplaceBundles,
  });
}
