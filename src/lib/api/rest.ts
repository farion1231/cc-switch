/**
 * REST 控制面配置 API
 *
 * 仅 `rest_api` feature on 时后端命令存在；前端运行期探测：invoke 失败（命令不存在）
 * 即 feature off，UI 隐藏面板。
 */

import { invoke } from "@tauri-apps/api/core";

/** REST 配置（镜像 Rust `RestConfig`，camelCase） */
export interface RestConfig {
  enabled: boolean;
  listenAddress: string;
  listenPort: number;
  token: string;
}

/** 获取 REST 配置 */
export async function getRestConfig(): Promise<RestConfig> {
  return invoke<RestConfig>("get_rest_config");
}

/** 设置 REST 配置（set 内可能生成 token，返回值含生成 token） */
export async function setRestConfig(config: RestConfig): Promise<RestConfig> {
  return invoke<RestConfig>("set_rest_config", { config });
}

/** 重新生成 token（保持其余字段不变） */
export async function regenerateRestToken(): Promise<RestConfig> {
  return invoke<RestConfig>("regenerate_rest_token");
}

/**
 * 探测 REST 控制面是否可用（feature on）。
 * 调 get_rest_config；命令不存在即 feature off。
 */
export async function probeRestAvailable(): Promise<boolean> {
  try {
    await getRestConfig();
    return true;
  } catch {
    return false;
  }
}
