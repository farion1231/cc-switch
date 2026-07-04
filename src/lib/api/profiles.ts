import { invoke } from "@tauri-apps/api/core";

/**
 * 按 app 分槽的载荷容器（与后端 services/profile.rs 的 PerApp<T> 严格对应）
 */
export interface PerApp<T> {
  claude: T;
  "claude-desktop": T;
  codex: T;
}

/**
 * 项目 Profile 的配置快照（与后端 ProfilePayload 严格对应）
 */
export interface ProfilePayload {
  providers: PerApp<string | null>;
  mcp: PerApp<string[]>;
  skills: PerApp<string[]>;
  prompts: PerApp<string | null>;
}

export interface Profile {
  id: string;
  name: string;
  payload: ProfilePayload;
  createdAt?: number;
  updatedAt?: number;
}

export interface ProfilesResponse {
  profiles: Profile[];
  currentId: string | null;
}

export const profilesApi = {
  /**
   * 获取所有项目及当前激活项目 id
   */
  async list(): Promise<ProfilesResponse> {
    return await invoke("list_profiles");
  },

  /**
   * 以当前配置状态创建新项目
   */
  async create(name: string): Promise<Profile> {
    return await invoke("create_profile", { name });
  },

  /**
   * 更新项目（重命名和/或以当前状态重拍快照）
   */
  async update(
    id: string,
    options: { name?: string; resnapshot?: boolean },
  ): Promise<Profile> {
    return await invoke("update_profile", {
      id,
      name: options.name,
      resnapshot: options.resnapshot,
    });
  },

  /**
   * 删除项目
   */
  async delete(id: string): Promise<void> {
    return await invoke("delete_profile", { id });
  },

  /**
   * 应用项目快照，返回 warnings（best-effort，部分失败不中断）
   */
  async apply(id: string): Promise<string[]> {
    return await invoke("apply_profile", { id });
  },

  /**
   * 不使用项目：仅清除激活标记，不改动任何配置
   */
  async clearCurrent(): Promise<void> {
    return await invoke("clear_current_profile");
  },
};
