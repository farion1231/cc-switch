export type ProviderCategory =
  | "official" // 官方
  | "cn_official" // 开源官方（原"国产官方"）
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
  // 备注信息
  notes?: string;
  // 新增：是否为商业合作伙伴
  isPartner?: boolean;
  // 可选：供应商元数据（仅存于 ~/.cc-switch/config.json，不写入 live 配置）
  meta?: ProviderMeta;
  // 图标配置
  icon?: string; // 图标名称（如 "openai", "anthropic"）
  iconColor?: string; // 图标颜色（Hex 格式，如 "#00A67E"）
  // 是否加入故障转移队列
  inFailoverQueue?: boolean;
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

// 端点候选项（用于端点测速弹窗）
export interface EndpointCandidate {
  id?: string;
  url: string;
  isCustom?: boolean;
}

// 用量查询脚本配置
export interface UsageScript {
  enabled: boolean; // 是否启用用量查询
  language: "javascript"; // 脚本语言
  code: string; // 脚本代码（JSON 格式配置）
  timeout?: number; // 超时时间（秒，默认 10）
  templateType?: "custom" | "general" | "newapi"; // 模板类型（用于后端判断验证规则）
  apiKey?: string; // 用量查询专用的 API Key（通用模板使用）
  baseUrl?: string; // 用量查询专用的 Base URL（通用和 NewAPI 模板使用）
  accessToken?: string; // 访问令牌（NewAPI 模板使用）
  userId?: string; // 用户ID（NewAPI 模板使用）
  autoQueryInterval?: number; // 自动查询间隔（单位：分钟，0 表示禁用）
  autoIntervalMinutes?: number; // 自动查询间隔（分钟）- 别名字段
  request?: {
    // 请求配置
    url?: string; // 请求 URL
    method?: string; // HTTP 方法
    headers?: Record<string, string>; // 请求头
    body?: any; // 请求体
  };
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
  // 请求地址管理：测速后自动选择最佳端点
  endpointAutoSelect?: boolean;
  // 是否为官方合作伙伴
  isPartner?: boolean;
  // 合作伙伴促销 key（用于后端识别 PackyCode 等）
  partnerPromotionKey?: string;
}

// 应用设置类型（用于设置对话框与 Tauri API）
// 存储在本地 ~/.cc-switch/settings.json，不随数据库同步
export interface Settings {
  // ===== 设备级 UI 设置 =====
  // 是否在系统托盘（macOS 菜单栏）显示图标
  showInTray: boolean;
  // 点击关闭按钮时是否最小化到托盘而不是关闭应用
  minimizeToTrayOnClose: boolean;
  // 启用 Claude 插件联动（写入 ~/.claude/config.json 的 primaryApiKey）
  enableClaudePluginIntegration?: boolean;
  // 跳过 Claude Code 初次安装确认（写入 ~/.claude.json 的 hasCompletedOnboarding）
  skipClaudeOnboarding?: boolean;
  // 是否开机自启
  launchOnStartup?: boolean;
  // 首选语言（可选，默认中文）
  language?: "en" | "zh" | "ja";

  // ===== 设备级目录覆盖 =====
  // 覆盖 Claude Code 配置目录（可选）
  claudeConfigDir?: string;
  // 覆盖 Codex 配置目录（可选）
  codexConfigDir?: string;
  // 覆盖 Gemini 配置目录（可选）
  geminiConfigDir?: string;

  // ===== 当前供应商 ID（设备级）=====
  // 当前 Claude 供应商 ID（优先于数据库 is_current）
  currentProviderClaude?: string;
  // 当前 Codex 供应商 ID（优先于数据库 is_current）
  currentProviderCodex?: string;
  // 当前 Gemini 供应商 ID（优先于数据库 is_current）
  currentProviderGemini?: string;
}

// MCP 服务器连接参数（宽松：允许扩展字段）
export interface McpServerSpec {
  // 可选：社区常见 .mcp.json 中 stdio 配置可不写 type
  type?: "stdio" | "http" | "sse";
  // stdio 字段
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  cwd?: string;
  // http 和 sse 字段
  url?: string;
  headers?: Record<string, string>;
  // 通用字段
  [key: string]: any;
}

// v3.7.0: MCP 服务器应用启用状态
export interface McpApps {
  claude: boolean;
  codex: boolean;
  gemini: boolean;
}

// MCP 服务器条目（v3.7.0 统一结构）
export interface McpServer {
  id: string;
  name: string;
  server: McpServerSpec;
  apps: McpApps; // v3.7.0: 标记应用到哪些客户端
  description?: string;
  tags?: string[];
  homepage?: string;
  docs?: string;
  // 兼容旧字段（v3.6.x 及以前）
  enabled?: boolean; // 已废弃，v3.7.0 使用 apps 字段
  source?: string;
  [key: string]: any;
}

// MCP 服务器映射（id -> McpServer）
export type McpServersMap = Record<string, McpServer>;

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

// ============================================================================
// 统一供应商（Universal Provider）- 跨应用共享配置
// ============================================================================

// 统一供应商的应用启用状态
export interface UniversalProviderApps {
  claude: boolean;
  codex: boolean;
  gemini: boolean;
}

// Claude 模型配置
export interface ClaudeModelConfig {
  model?: string;
  haikuModel?: string;
  sonnetModel?: string;
  opusModel?: string;
}

// Codex 模型配置
export interface CodexModelConfig {
  model?: string;
  reasoningEffort?: string;
}

// Gemini 模型配置
export interface GeminiModelConfig {
  model?: string;
}

// 各应用的模型配置
export interface UniversalProviderModels {
  claude?: ClaudeModelConfig;
  codex?: CodexModelConfig;
  gemini?: GeminiModelConfig;
}

// 统一供应商（跨应用共享配置）
export interface UniversalProvider {
  id: string;
  name: string;
  providerType: string; // "newapi" | "custom" 等
  apps: UniversalProviderApps;
  baseUrl: string;
  apiKey: string;
  models: UniversalProviderModels;
  websiteUrl?: string;
  notes?: string;
  icon?: string;
  iconColor?: string;
  meta?: ProviderMeta;
  createdAt?: number;
  sortIndex?: number;
}

// 统一供应商映射（id -> UniversalProvider）
export type UniversalProvidersMap = Record<string, UniversalProvider>;
