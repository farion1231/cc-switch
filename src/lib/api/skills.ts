import { invoke } from "@tauri-apps/api/core";

import type { AppId } from "@/lib/api/types";

export type AppType =
  | "claude"
  | "codex"
  | "gemini"
  | "opencode"
  | "openclaw"
  | "hermes";

/** Skill 应用启用状态 */
export interface SkillApps {
  claude: boolean;
  codex: boolean;
  gemini: boolean;
  opencode: boolean;
  openclaw: boolean;
  hermes: boolean;
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
  contentHash?: string;
  updatedAt: number;
}

export interface SkillUninstallResult {
  backupPath?: string;
}

export interface SkillBackupEntry {
  backupId: string;
  backupPath: string;
  createdAt: number;
  skill: InstalledSkill;
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
  path: string;
}

/** 导入已有 Skill 时提交的应用启用状态 */
export interface ImportSkillSelection {
  directory: string;
  apps: SkillApps;
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

/** Skill 更新信息 */
export interface SkillUpdateInfo {
  id: string;
  name: string;
  currentHash?: string;
  remoteHash: string;
}

/** 存储位置迁移结果 */
export interface MigrationResult {
  migratedCount: number;
  skippedCount: number;
  errors: string[];
}

/** skills.sh 可发现的技能 */
export interface SkillsShDiscoverableSkill {
  key: string;
  name: string;
  directory: string;
  repoOwner: string;
  repoName: string;
  repoBranch: string;
  installs: number;
  readmeUrl?: string;
}

/** skills.sh 搜索结果 */
export interface SkillsShSearchResult {
  skills: SkillsShDiscoverableSkill[];
  totalCount: number;
  query: string;
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

  /** 获取可恢复的 Skill 备份列表 */
  async getBackups(): Promise<SkillBackupEntry[]> {
    return await invoke("get_skill_backups");
  },

  /** 删除 Skill 备份 */
  async deleteBackup(backupId: string): Promise<boolean> {
    return await invoke("delete_skill_backup", { backupId });
  },

  /** 安装 Skill（统一安装） */
  async installUnified(
    skill: DiscoverableSkill,
    currentApp: AppId,
  ): Promise<InstalledSkill> {
    return await invoke("install_skill_unified", { skill, currentApp });
  },

  /** 卸载 Skill（统一卸载） */
  async uninstallUnified(id: string): Promise<SkillUninstallResult> {
    return await invoke("uninstall_skill_unified", { id });
  },

  /** 从备份恢复 Skill */
  async restoreBackup(
    backupId: string,
    currentApp: AppId,
  ): Promise<InstalledSkill> {
    return await invoke("restore_skill_backup", { backupId, currentApp });
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
  async importFromApps(
    imports: ImportSkillSelection[],
  ): Promise<InstalledSkill[]> {
    return await invoke("import_skills_from_apps", { imports });
  },

  /** 发现可安装的 Skills（从仓库获取） */
  async discoverAvailable(): Promise<DiscoverableSkill[]> {
    return await invoke("discover_available_skills");
  },

  /** 检查 Skills 更新 */
  async checkUpdates(): Promise<SkillUpdateInfo[]> {
    return await invoke("check_skill_updates");
  },

  /** 更新单个 Skill */
  async updateSkill(id: string): Promise<InstalledSkill> {
    return await invoke("update_skill", { id });
  },

  /** 迁移 Skill 存储位置 */
  async migrateStorage(
    target: "cc_switch" | "unified",
  ): Promise<MigrationResult> {
    return await invoke("migrate_skill_storage", { target });
  },

  /** 搜索 skills.sh 公共目录 */
  async searchSkillsSh(
    query: string,
    limit: number,
    offset: number,
  ): Promise<SkillsShSearchResult> {
    return await invoke("search_skills_sh", { query, limit, offset });
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
  async uninstall(
    directory: string,
    app: AppId = "claude",
  ): Promise<SkillUninstallResult> {
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
