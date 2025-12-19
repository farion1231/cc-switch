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
   * 测试私有仓库连接
   * 依次尝试多种认证头，返回成功的认证头名称
   * @param url 仓库 URL
   * @param accessToken 访问令牌
   * @returns 成功的认证头名称（如 "Authorization" 或 "PRIVATE-TOKEN"）
   */
  async testRepoConnection(url: string, accessToken: string): Promise<string> {
    return await invoke("test_repo_connection", { url, accessToken });
  },
};
