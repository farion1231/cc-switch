// Template 功能相关类型定义

// 组件类型
export type ComponentType =
  | "agent"
  | "command"
  | "mcp"
  | "setting"
  | "hook"
  | "skill";

// 模板仓库
export interface TemplateRepo {
  id: number | null;
  owner: string;
  name: string;
  branch: string;
  enabled: boolean;
  createdAt?: string;
  updatedAt?: string;
}

// 模板组件
export interface TemplateComponent {
  id: number | null;
  repoId: number;
  componentType: ComponentType;
  category: string | null;
  name: string;
  path: string;
  description: string | null;
  contentHash: string | null;
  installed: boolean;
  createdAt?: string;
  updatedAt?: string;
}

// 组件详情（含完整内容）
export interface ComponentDetail extends TemplateComponent {
  content: string;
  repoOwner: string;
  repoName: string;
  repoBranch: string;
  readmeUrl: string;
}

// 已安装组件
export interface InstalledComponent {
  id: number | null;
  componentId: number | null;
  componentType: ComponentType;
  name: string;
  path: string;
  appType: string;
  installedAt: string;
}

// 分页结果
export interface PaginatedResult<T> {
  items: T[];
  total: number;
  page: number;
  pageSize: number;
}

// 组件过滤选项
export interface ComponentFilter {
  componentType?: ComponentType;
  category?: string;
  search?: string;
  page?: number;
  pageSize?: number;
  appType?: string;
}

// 批量安装结果
export interface BatchInstallResult {
  success: number[];
  failed: Array<[number, string]>;
}

// 组件元数据（从 YAML front matter 解析）
export interface ComponentMetadata {
  name?: string;
  description?: string;
  tools?: string; // Agent 专用
  model?: string; // Agent 专用
}

// 市场组合项（plugin 中的单个组件）
export interface MarketplaceBundleItem {
  name: string;
  path: string;
  componentType: ComponentType;
}

// 市场组合（预设组件集合）
export interface MarketplaceBundle {
  id: string;
  name: string;
  description: string;
  category: string;
  components: MarketplaceBundleItem[];
}
