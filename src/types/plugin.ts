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
  /** 非空 = 加载失败的占位条目，只读展示错误 */
  error: string | null;
}

export interface PluginReloadResult {
  loaded: number;
  errors: string[];
}
