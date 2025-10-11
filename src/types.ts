export type ProviderCategory =
  | "official" // 官方
  | "cn_official" // 国产官方
  | "aggregator" // 聚合网站
  | "third_party" // 第三方供应商
  | "custom"; // 自定义

// 排序维度
export type SortField =
  | "name"           // 名称 A-Z
  | "id"             // 供应商ID
  | "createdAt"      // 创建时间
  | "lastUsed"       // 最近使用时间
  | "priority"       // 优先级
  | "contractExpiry" // 合同到期日
  | "custom";        // 自定义顺序

export type SortOrder = "asc" | "desc";

// 排序配置
export interface SortConfig {
  field: SortField;
  order: SortOrder;
}

// 供应商分组
export interface ProviderGroup {
  id: string;                    // 分组唯一ID
  name: string;                  // 分组名称
  color?: string;                // 分组颜色标记
  icon?: string;                 // 分组图标
  parentId?: string;             // 父分组ID（支持嵌套）
  providerIds: string[];         // 包含的供应商ID列表
  collapsed?: boolean;           // 是否折叠
  sortConfig?: SortConfig;       // 分组内独立排序配置
  order?: number;                // 分组显示顺序
  createdAt: number;             // 创建时间
  updatedAt: number;             // 更新时间
}

export interface Provider {
  id: string;
  name: string;
  settingsConfig: Record<string, any>; // 应用配置对象：Claude 为 settings.json；Codex 为 { auth, config }
  websiteUrl?: string;
  // 新增：供应商分类（用于差异化提示/能力开关）
  category?: ProviderCategory;
  createdAt?: number; // 添加时间戳（毫秒）
  // 可选：供应商元数据（仅存于 ~/.cc-switch/config.json，不写入 live 配置）
  meta?: ProviderMeta;

  // 分组和排序相关字段
  groupId?: string;              // 所属分组ID
  priority?: number;             // 优先级评分 (0-100)
  contractExpiry?: number;       // 合同到期时间戳
  lastUsedAt?: number;           // 最后使用时间
  tags?: string[];               // 标签
  customOrder?: number;          // 自定义排序顺序
}

export interface AppConfig {
  providers: Record<string, Provider>;
  current: string;
  groups?: Record<string, ProviderGroup>;     // 分组配置
  globalSortConfig?: SortConfig;              // 全局排序配置
  groupsOrder?: string[];                     // 分组显示顺序
}

// 自定义端点配置
export interface CustomEndpoint {
  url: string;
  addedAt: number;
  lastUsed?: number;
}

// 供应商元数据（字段名与后端一致，保持 snake_case）
export interface ProviderMeta {
  // 自定义端点：以 URL 为键，值为端点信息
  custom_endpoints?: Record<string, CustomEndpoint>;
}

// 应用设置类型（用于 SettingsModal 与 Tauri API）
export interface Settings {
  // 是否在系统托盘（macOS 菜单栏）显示图标
  showInTray: boolean;
  // 点击关闭按钮时是否最小化到托盘而不是关闭应用
  minimizeToTrayOnClose: boolean;
  // 启用 Claude 插件联动（写入 ~/.claude/config.json 的 primaryApiKey）
  enableClaudePluginIntegration?: boolean;
  // 覆盖 Claude Code 配置目录（可选）
  claudeConfigDir?: string;
  // 覆盖 Codex 配置目录（可选）
  codexConfigDir?: string;
  // 首选语言（可选，默认中文）
  language?: "en" | "zh";
  // Claude 自定义端点列表
  customEndpointsClaude?: Record<string, CustomEndpoint>;
  // Codex 自定义端点列表
  customEndpointsCodex?: Record<string, CustomEndpoint>;

  // 分组和排序偏好设置
  defaultSortConfig?: SortConfig;             // 默认排序规则
  showUngroupedProviders?: boolean;           // 是否显示未分组供应商
  rememberGroupCollapseState?: boolean;       // 记住分组折叠状态
}

// MCP 服务器定义（宽松：允许扩展字段）
export interface McpServer {
  // 可选：社区常见 .mcp.json 中 stdio 配置可不写 type
  type?: "stdio" | "http";
  // stdio 字段
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  cwd?: string;
  // http 字段
  url?: string;
  headers?: Record<string, string>;
  // 通用字段
  enabled?: boolean; // 是否启用该 MCP 服务器，默认 true
  [key: string]: any;
}

// MCP 配置状态
export interface McpStatus {
  userConfigPath: string;
  userConfigExists: boolean;
  serverCount: number;
}

// 新：来自 config.json 的 MCP 列表响应
export interface McpConfigResponse {
  configPath: string;
  servers: Record<string, McpServer>;
}

export interface ProviderTestResult {
  success: boolean;
  status?: number;
  latencyMs?: number;
  message: string;
  detail?: string;
}
