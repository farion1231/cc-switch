import { invoke } from "@tauri-apps/api/core";
import type { AppId } from "./types";

export interface InjectionToggle {
  enabled: boolean;
  receiveShared: boolean;
  customFilePath?: string | null;
}

export const systemPromptApi = {
  /** 读取指定应用的全局系统提示文件内容 */
  async getFile(appId: AppId): Promise<string> {
    return await invoke("get_system_prompt_file", { app: appId });
  },

  /** 保存指定应用的全局系统提示文件内容 */
  async saveFile(appId: AppId, content: string): Promise<void> {
    return await invoke("save_system_prompt_file", {
      app: appId,
      content,
    });
  },

  /** 获取指定应用的注入开关状态 */
  async getToggle(appId: AppId): Promise<InjectionToggle> {
    return await invoke("get_injection_toggle", { app: appId });
  },

  /** 设置指定应用的注入开关状态 */
  async setToggle(appId: AppId, toggle: InjectionToggle): Promise<void> {
    return await invoke("set_injection_toggle", {
      app: appId,
      toggle,
    });
  },

  /** 获取统一共享规则内容 */
  async getShared(): Promise<string> {
    return await invoke("get_shared_prompt");
  },

  /** 保存统一共享规则内容 */
  async saveShared(content: string): Promise<void> {
    return await invoke("save_shared_prompt", { content });
  },

  /** 打开文件选择对话框，选择 .md 文件 */
  async pickFile(): Promise<string | null> {
    return await invoke("pick_system_prompt_file");
  },
};
