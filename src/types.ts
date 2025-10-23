export type ProviderCategory =
  | "official" // 官方
  | "cn_official" // 国产官方
  | "aggregator" // 聚合网站
  | "third_party" // 第三方供应商
  | "custom"; // 自定义

export interface Provider {
  id: string;
  name: string;
  settingsConfig: Record<string, any>; // 应用配置对象：Claude 为 settings.json；Codex 为 { auth, config }
  websiteUrl?: string;
  // 新增：供应商分类（用于差异化提示/能力开关）
  category?: ProviderCategory;
  createdAt?: number; // 添加时间戳（毫秒）
  sortIndex?: number; // 排序索引（用于自定义拖拽排序）
  // 可选：供应商元数据（仅存于 ~/.cc-switch/config.json，不写入 live 配置）
  meta?: ProviderMeta;
}

export interface AppConfig {
  providers: Record<string, Provider>;
  current: string;
}

// 自定义端点配置
export interface CustomEndpoint {
  url: string;
  addedAt: number;
  lastUsed?: number;
}

// 用量查询脚本配置
export interface UsageScript {
  enabled: boolean; // 是否启用用量查询
  language: "javascript"; // 脚本语言
  code: string; // 脚本代码（JSON 格式配置）
  timeout?: number; // 超时时间（秒，默认 10）
}

// 单个套餐用量数据
export interface UsageData {
  planName?: string; // 套餐名称（可选）
  extra?: string; // 扩展字段，可自由补充需要展示的文本（可选）
  isValid?: boolean; // 套餐是否有效（可选）
  invalidMessage?: string; // 失效原因说明（可选，当 isValid 为 false 时显示）
  total?: number; // 总额度（可选）
  used?: number; // 已用额度（可选）
  remaining?: number; // 剩余额度（可选）
  unit?: string; // 单位（可选）
}

// 用量查询结果（支持多套餐）
export interface UsageResult {
  success: boolean;
  data?: UsageData[]; // 改为数组，支持返回多个套餐
  error?: string;
}

// 供应商元数据（字段名与后端一致，保持 snake_case）
export interface ProviderMeta {
  // 自定义端点：以 URL 为键，值为端点信息
  custom_endpoints?: Record<string, CustomEndpoint>;
  // 用量查询脚本配置
  usage_script?: UsageScript;
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
}

// MCP 服务器连接参数（宽松：允许扩展字段）
export interface McpServerSpec {
  // 可选：社区常见 .mcp.json 中 stdio 配置可不写 type
  type?: "stdio" | "http";
  // stdio 字段
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  cwd?: string;
  startup_timeout_ms?: number;
  // http 字段
  url?: string;
  headers?: Record<string, string>;
  // 通用字段
  [key: string]: any;
}

// MCP 服务器条目（含元信息）
export interface McpServer {
  id: string;
  name?: string;
  description?: string;
  tags?: string[];
  homepage?: string;
  docs?: string;
  enabled?: boolean;
  server: McpServerSpec;
  source?: string;
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
