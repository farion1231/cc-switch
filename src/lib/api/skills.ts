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
}

export const skillsApi = {
  async getAll(): Promise<Skill[]> {
    return await invoke("get_skills");
  },

  async install(directory: string): Promise<boolean> {
    return await invoke("install_skill", { directory });
  },

  async uninstall(directory: string): Promise<boolean> {
    return await invoke("uninstall_skill", { directory });
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
};
