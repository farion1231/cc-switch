import { invoke } from "@tauri-apps/api/core";
import type {
  TemplateRepo,
  TemplateComponent,
  ComponentDetail,
  PaginatedResult,
  ComponentFilter,
  BatchInstallResult,
  InstalledComponent,
  ComponentType,
} from "@/types/template";
import type { AppType } from "./config";

export const templateApi = {
  // 模板仓库管理
  async listTemplateRepos(): Promise<TemplateRepo[]> {
    return invoke("list_template_repos");
  },

  async addTemplateRepo(
    owner: string,
    name: string,
    branch: string,
  ): Promise<void> {
    return invoke("add_template_repo", { owner, name, branch });
  },

  async removeTemplateRepo(id: number): Promise<void> {
    return invoke("remove_template_repo", { id });
  },

  async toggleTemplateRepo(id: number, enabled: boolean): Promise<void> {
    return invoke("toggle_template_repo", { id, enabled });
  },

  // 模板索引刷新
  async refreshTemplateIndex(): Promise<void> {
    return invoke("refresh_template_index");
  },

  // 模板组件查询
  async listTemplateComponents(
    filter: ComponentFilter,
  ): Promise<PaginatedResult<TemplateComponent>> {
    return invoke("list_template_components", {
      componentType: filter.componentType,
      category: filter.category,
      search: filter.search,
      page: filter.page ?? 1,
      pageSize: filter.pageSize ?? 20,
      appType: filter.appType,
    });
  },

  async getTemplateComponent(id: number): Promise<ComponentDetail> {
    return invoke("get_template_component", { id });
  },

  async getComponentCategories(
    componentType?: ComponentType,
  ): Promise<string[]> {
    return invoke("list_template_categories", { componentType });
  },

  // 组件安装管理
  async installTemplateComponent(id: number, appType: AppType): Promise<void> {
    return invoke("install_template_component", { id, appType });
  },

  async uninstallTemplateComponent(
    id: number,
    appType: AppType,
  ): Promise<void> {
    return invoke("uninstall_template_component", { id, appType });
  },

  async batchInstallComponents(
    ids: number[],
    appType: AppType,
  ): Promise<BatchInstallResult> {
    return invoke("batch_install_template_components", { ids, appType });
  },

  async listInstalledComponents(
    appType?: AppType,
    componentType?: ComponentType,
  ): Promise<InstalledComponent[]> {
    return invoke("list_installed_components", { appType, componentType });
  },

  // 组件内容预览
  async previewComponentContent(id: number): Promise<string> {
    return invoke("preview_component_content", { id });
  },

  // 市场组合
  async listMarketplaceBundles(): Promise<
    import("@/types/template").MarketplaceBundle[]
  > {
    return invoke("list_marketplace_bundles");
  },
};
