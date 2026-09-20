// 插件系统前端类型（与 src-tauri PluginInfo 序列化结构对应，serde camelCase）

export type PluginStage =
  | "pre_request"
  | "pre_send"
  | "post_response"
  | "sse_chunk";

export interface PluginInfo {
  /** "builtin:cache-injector" | "user:my-plugin"；加载失败条目可能是 "user:<目录名>" */
  id: string;
  displayName: string;
  description: string;
  isBuiltin: boolean;
  stages: PluginStage[];
  priority: number;
  enabled: boolean;
  version: string | null;
  /** 用户插件 manifest 路径；内置为 null */
  source: string | null;
  /** 插件声明了配置界面（config_schema 非空）→ 面板显示"设置"按钮 */
  hasConfig: boolean;
  /** 可选，插件自定义的「设置」对话框标题 */
  configTitle?: string;
  /** 非空 = 加载失败的占位条目，只读展示错误 */
  error: string | null;
}

export interface PluginReloadResult {
  loaded: number;
  errors: string[];
}

/** plugin_list 返回：插件列表 + 后端显式给出的全局开关（前端不得自行推导） */
export interface PluginListResult {
  plugins: PluginInfo[];
  globalEnabled: boolean;
}

// ---------------------------------------------------------------------------
// 声明式配置界面（plugin.json 的 config_schema；字段名与 Rust serde 序列化一致，
// ConfigSchemaItem/ConfigColumn 未加 camelCase 重命名 → snake_case）
// ---------------------------------------------------------------------------

export type ConfigFieldType =
  | "toggle"
  | "text"
  | "number"
  | "select"
  | "textarea"
  | "table";

export type ConfigColumnType = "text" | "number" | "toggle" | "textarea" | "select";

export interface ConfigColumn {
  key: string;
  type: ConfigColumnType;
  label: string;
  options?: string[];
}

export interface ConfigSchemaItem {
  type: ConfigFieldType;
  key: string;
  /** 插件目录内的目标配置文件（相对路径，允许子目录） */
  file: string;
  /** 可选，页签分组名（缺省按 file 分组） */
  tab?: string;
  /** 文档内的点路径（缺省 = key） */
  path?: string;
  label: string;
  description?: string;
  options?: string[];
  columns?: ConfigColumn[];
}

/** plugin_config_read 返回：{ 配置文件名: 文档 } */
export type PluginConfigDocs = Record<string, unknown>;
