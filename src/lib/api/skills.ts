import { invoke } from "@tauri-apps/api/core";

export interface Skill {
  key: string;
  name: string;
  description: string;
  directory: string;
  readmeUrl?: string;
  installed: boolean;
  repoOwner?: string;
  repoName?: string;
  repoBranch?: string;
}

export interface SkillRepo {
  owner: string;
  name: string;
  branch: string;
  enabled: boolean;
  // 私有仓库字段（可选）
  base_url?: string;
  access_token?: string;
  auth_header?: string;
}

export type AppType = "claude" | "codex" | "gemini";

/**
 * 仓库加载状态
 * 用于渐进式加载时跟踪每个仓库的加载进度
 */
export interface RepoLoadingState {
  /** 加载状态: pending-等待中, loading-加载中, success-成功, error-失败 */
  status: "pending" | "loading" | "success" | "error";
  /** 错误信息（仅在 status 为 error 时有值） */
  error?: string;
  /** 该仓库的技能数量（仅在 status 为 success 时有值） */
  skillCount?: number;
}

export const skillsApi = {
  async getAll(app: AppType = "claude"): Promise<Skill[]> {
    if (app === "claude") {
      return await invoke("get_skills");
    }
    return await invoke("get_skills_for_app", { app });
  },

  async install(directory: string, app: AppType = "claude"): Promise<boolean> {
    if (app === "claude") {
      return await invoke("install_skill", { directory });
    }
    return await invoke("install_skill_for_app", { app, directory });
  },

  async uninstall(
    directory: string,
    app: AppType = "claude",
  ): Promise<boolean> {
    if (app === "claude") {
      return await invoke("uninstall_skill", { directory });
    }
    return await invoke("uninstall_skill_for_app", { app, directory });
  },

  async getRepos(): Promise<SkillRepo[]> {
    return await invoke("get_skill_repos");
  },

  async addRepo(repo: SkillRepo): Promise<boolean> {
    return await invoke("add_skill_repo", { repo });
  },

  async removeRepo(owner: string, name: string): Promise<boolean> {
    return await invoke("remove_skill_repo", { owner, name });
  },

  /**
   * 切换仓库的启用状态
   * 用于控制仓库是否在 Skills 页面中显示
   * @param owner 仓库所有者
   * @param name 仓库名称
   * @param enabled 是否启用
   */
  async toggleRepoEnabled(owner: string, name: string, enabled: boolean): Promise<boolean> {
    return await invoke("toggle_repo_enabled", { owner, name, enabled });
  },

  /**
   * 测试私有仓库连接
   * 依次尝试多种认证头，返回成功的认证头名称
   * @param url 仓库 URL
   * @param accessToken 访问令牌
   * @returns 成功的认证头名称（如 "Authorization" 或 "PRIVATE-TOKEN"）
   */
  async testRepoConnection(url: string, accessToken: string): Promise<string> {
    return await invoke("test_repo_connection", { url, accessToken });
  },

  /**
   * 获取单个仓库的技能列表
   * 用于渐进式加载，每个仓库独立加载其技能
   * @param app 应用类型
   * @param repoOwner 仓库所有者
   * @param repoName 仓库名称
   * @returns 该仓库的技能列表
   */
  async getSkillsForRepo(
    app: AppType,
    repoOwner: string,
    repoName: string
  ): Promise<Skill[]> {
    return await invoke("get_skills_for_repo", { app, repoOwner, repoName });
  },

  /**
   * 获取本地独有的技能列表
   * 返回所有本地安装的技能中，不属于任何远程仓库的技能
   * @param app 应用类型
   * @param remoteSkills 已加载的远程技能列表
   * @returns 本地独有的技能列表
   */
  async getLocalSkills(
    app: AppType,
    remoteSkills: Skill[]
  ): Promise<Skill[]> {
    return await invoke("get_local_skills", { app, remoteSkills });
  },
};
