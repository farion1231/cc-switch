import { invoke } from "@tauri-apps/api/core";

import type { AppId } from "@/lib/api/types";

// ========== 类型定义 ==========

/** Skill 应用启用状态 */
export interface SkillApps {
  claude: boolean;
  codex: boolean;
  gemini: boolean;
  opencode: boolean;
}

/** 已安装的 Skill（v3.10.0+ 统一结构） */
export interface InstalledSkill {
  id: string;
  name: string;
  description?: string;
  directory: string;
  repoOwner?: string;
  repoName?: string;
  repoBranch?: string;
  readmeUrl?: string;
  apps: SkillApps;
  installedAt: number;
}

/** 可发现的 Skill（来自仓库） */
export interface DiscoverableSkill {
  key: string;
  name: string;
  description: string;
  directory: string;
  readmeUrl?: string;
  repoOwner: string;
  repoName: string;
  repoBranch: string;
}

/** 未管理的 Skill（用于导入） */
export interface UnmanagedSkill {
  directory: string;
  name: string;
  description?: string;
  foundIn: string[];
}

/** 技能对象（兼容旧 API） */
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

/** 仓库配置 */
export interface SkillRepo {
  owner: string;
  name: string;
  branch: string;
  enabled: boolean;
}

// ========== API ==========

export const skillsApi = {
  // ========== 统一管理 API (v3.10.0+) ==========

  /** 获取所有已安装的 Skills */
  async getInstalled(): Promise<InstalledSkill[]> {
    return await invoke("get_installed_skills");
  },

  /** 安装 Skill（统一安装） */
  async installUnified(
    skill: DiscoverableSkill,
    currentApp: AppId,
  ): Promise<InstalledSkill> {
    return await invoke("install_skill_unified", { skill, currentApp });
  },

  /** 卸载 Skill（统一卸载） */
  async uninstallUnified(id: string): Promise<boolean> {
    return await invoke("uninstall_skill_unified", { id });
  },

  /** 切换 Skill 的应用启用状态 */
  async toggleApp(id: string, app: AppId, enabled: boolean): Promise<boolean> {
    return await invoke("toggle_skill_app", { id, app, enabled });
  },

  /** 扫描未管理的 Skills */
  async scanUnmanaged(): Promise<UnmanagedSkill[]> {
    return await invoke("scan_unmanaged_skills");
  },

  /** 从应用目录导入 Skills */
  async importFromApps(directories: string[]): Promise<InstalledSkill[]> {
    return await invoke("import_skills_from_apps", { directories });
  },

  /** 发现可安装的 Skills（从仓库获取） */
  async discoverAvailable(): Promise<DiscoverableSkill[]> {
    return await invoke("discover_available_skills");
  },

  // ========== 兼容旧 API ==========

  /** 获取技能列表（兼容旧 API） */
  async getAll(app: AppId = "claude"): Promise<Skill[]> {
    if (app === "claude") {
      return await invoke("get_skills");
    }
    return await invoke("get_skills_for_app", { app });
  },

  /** 安装技能（兼容旧 API） */
  async install(directory: string, app: AppId = "claude"): Promise<boolean> {
    if (app === "claude") {
      return await invoke("install_skill", { directory });
    }
    return await invoke("install_skill_for_app", { app, directory });
  },

  /** 卸载技能（兼容旧 API） */
  async uninstall(directory: string, app: AppId = "claude"): Promise<boolean> {
    if (app === "claude") {
      return await invoke("uninstall_skill", { directory });
    }
    return await invoke("uninstall_skill_for_app", { app, directory });
  },

  // ========== 仓库管理 ==========

  /** 获取仓库列表 */
  async getRepos(): Promise<SkillRepo[]> {
    return await invoke("get_skill_repos");
  },

  /** 添加仓库 */
  async addRepo(repo: SkillRepo): Promise<boolean> {
    return await invoke("add_skill_repo", { repo });
  },

  /** 删除仓库 */
  async removeRepo(owner: string, name: string): Promise<boolean> {
    return await invoke("remove_skill_repo", { owner, name });
  },

  // ========== ZIP 安装 ==========

  /** 打开 ZIP 文件选择对话框 */
  async openZipFileDialog(): Promise<string | null> {
    return await invoke("open_zip_file_dialog");
  },

  /** 从 ZIP 文件安装 Skills */
  async installFromZip(
    filePath: string,
    currentApp: AppId,
  ): Promise<InstalledSkill[]> {
    return await invoke("install_skills_from_zip", { filePath, currentApp });
  },
};
