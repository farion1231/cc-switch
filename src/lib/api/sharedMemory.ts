import { invoke } from "@tauri-apps/api/core";
import type { SharedMemorySettings } from "@/types";

/**
 * 共享记忆 API —— cc-switch 作为跨平台共享记忆的统一读写通道。
 *
 * 云端协议（Cloudflare Worker）：
 *  - GET {url}/api  → { ok, updatedAt, bytes, content }（读取）
 *  - PUT {url}/api  → 请求头 X-Auth-Token + text/plain body（写入）
 */
export interface SharedMemorySnapshot {
  ok: boolean;
  updatedAt: string | null;
  bytes: number;
  content: string;
}

export const sharedMemoryApi = {
  /** 读取共享记忆设置（含令牌，仅供本机 UI 使用）。 */
  async getSettings(): Promise<SharedMemorySettings> {
    return await invoke("shared_memory_get_settings");
  },

  /** 保存共享记忆设置；令牌留空时后端保留已有令牌。 */
  async saveSettings(settings: SharedMemorySettings): Promise<{ success: boolean }> {
    return await invoke("shared_memory_save_settings", { incoming: settings });
  },

  /** 从云端拉取共享记忆。 */
  async fetch(): Promise<SharedMemorySnapshot> {
    return await invoke("shared_memory_fetch");
  },

  /** 推送内容到云端（覆盖）。 */
  async push(content: string): Promise<SharedMemorySnapshot> {
    return await invoke("shared_memory_push", { content });
  },
};