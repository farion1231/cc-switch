import { invoke } from "@tauri-apps/api/core";

import type { AppId } from "@/lib/api/types";

export type AppType =
  | "claude"
  | "claude-desktop"
  | "codex"
  | "gemini"
  | "grokbuild"
  | "opencode"
  | "openclaw"
  | "hermes"
  | "pi"
  | "mcode"
  | "kimi"
  | "dsh"
  | "agy"
  | "omp"
  | "copilot"
  | "codebuddy";

/** Skill 应用启用状态 */
export interface SkillApps {
  claude: boolean;
  "claude-desktop"?: boolean;
  codex: boolean;
  gemini: boolean;
  grokbuild?: boolean;
  opencode: boolean;
  openclaw: boolean;
  hermes: boolean;
  pi: boolean;
  mcode?: boolean;
  kimi?: boolean;
  dsh?: boolean;
  agy?: boolean;
  omp?: boolean;
  copilot?: boolean;
  workbuddy?: boolean;
  codebuddy?: boolean;
}

/** 设置了访问令牌的主机（读私有仓库用，令牌本身不经前端读回） */
export interface SkillRepoTokens {
  hosts: string[];
  /** 令牌存在系统凭据存储里；false 时存在本机仅自己可读的文件里 */
  secureStorage: boolean;
}

/** 已安装的 Skill（v3.10.0+ 统一结构） */
export interface InstalledSkill {
  id: string;
  name: string;
  description?: string;
  directory: string;
  /** 仓库所在主机；不写即 github.com，GitLab 仓库是实例的主机名 */
  repoHost?: string;
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
  preservedPiPath?: string;
  piCleanupIncomplete?: boolean;
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
  /** 仓库所在主机；不写即 github.com（后端总会带上，skills.sh 的结果不带） */
  repoHost?: string;
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

/** Skill 更新信息 */
export interface SkillUpdateInfo {
  id: string;
  name: string;
  currentHash?: string;
  remoteHash: string;
}

/** 没有读到的仓库（发现、检查更新时逐仓库报告） */
export interface SkillRepoFailure {
  owner: string;
  name: string;
  branch: string;
  /** 原始错误；下载类错误是结构化 JSON，用 formatSkillError / skillErrorReason 解析 */
  error: string;
}

/** 发现结果：读到的 Skill + 没读到的仓库 */
export interface SkillDiscoveryResult {
  skills: DiscoverableSkill[];
  failures: SkillRepoFailure[];
}

/** 检查更新结果：可更新的 Skill + 没读到的仓库 */
export interface SkillUpdateCheckResult {
  updates: SkillUpdateInfo[];
  failures: SkillRepoFailure[];
}

/** ZIP 里因目录名已被占用而跳过的 Skill（#3749） */
export interface ZipSkippedSkill {
  directory: string;
  existingId: string;
  existingName: string;
}

/** 从 ZIP 安装的结果 */
export interface ZipInstallResult {
  installed: InstalledSkill[];
  skipped: ZipSkippedSkill[];
}

/** 「立即重新同步」里单个应用的结果 */
export interface SkillAppSyncOutcome {
  app: AppType;
  ok: boolean;
  /** 整个应用没同步 */
  error?: string;
  /** 应用同步了，但这些 Skill 失败 */
  failedSkills: Array<{ directory: string; error: string }>;
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
  /**
   * 仓库所在主机；不写即 github.com，其余按 GitLab 处理（自建实例可带端口）。
   * 只是属性：仓库仍按 owner + name 定位，GitLab 的 owner 是分组路径，可含 `/`。
   */
  host?: string;
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

  /** 刷新本地 Skill 的过期元数据，返回实际更新的数量 */
  async refreshLocalMetadata(): Promise<number> {
    return await invoke("refresh_local_skill_metadata");
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

  /**
   * 发现可安装的 Skills（从仓库获取），附带没读到的仓库。
   * 后端按仓库缓存发现结果，forceRefresh 绕过缓存（手动刷新用）。
   */
  async discoverAvailable(forceRefresh = false): Promise<SkillDiscoveryResult> {
    return await invoke("discover_available_skills", { forceRefresh });
  },

  /** 只发现一个已保存的仓库；仓库不存在或已停用时结果为空 */
  async discoverRepo(
    owner: string,
    name: string,
  ): Promise<SkillDiscoveryResult> {
    return await invoke("discover_repo_skills", { owner, name });
  },

  /** 检查 Skills 更新，附带没读到的仓库 */
  async checkUpdates(): Promise<SkillUpdateCheckResult> {
    return await invoke("check_skill_updates");
  },

  /** 立即重新同步：按开关和当前同步方式把 Skill 重新投影到各应用目录 */
  async resyncToApps(): Promise<SkillAppSyncOutcome[]> {
    return await invoke("resync_skills_to_apps");
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

  /** CC Switch 目录下放 Skill 主副本的路径（改过配置目录就是改后的） */
  async getCcSwitchSkillsDir(): Promise<string> {
    return await invoke("get_cc_switch_skills_dir");
  },

  /** 在文件管理器里打开 CC Switch 目录下的 Skills 目录 */
  async openCcSwitchSkillsDir(): Promise<void> {
    await invoke("open_cc_switch_skills_dir");
  },

  /** 在文件管理器里打开某个已安装 Skill 的目录 */
  async openSkillDir(directory: string): Promise<void> {
    await invoke("open_skill_dir", { directory });
  },

  /** 搜索 skills.sh 公共目录 */
  async searchSkillsSh(
    query: string,
    limit: number,
    offset: number,
  ): Promise<SkillsShSearchResult> {
    return await invoke("search_skills_sh", { query, limit, offset });
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

  // ========== 仓库访问令牌（按主机，只在本机） ==========

  /** 设置了访问令牌的主机（不含令牌本身） */
  async getRepoTokens(): Promise<SkillRepoTokens> {
    return await invoke("get_skill_repo_tokens");
  },

  /** 保存（覆盖）一台主机的访问令牌，返回规范化后的主机名 */
  async setRepoToken(host: string, token: string): Promise<string> {
    return await invoke("set_skill_repo_token", { host, token });
  },

  /** 删除一台主机的访问令牌，返回规范化后的主机名 */
  async deleteRepoToken(host: string): Promise<string> {
    return await invoke("delete_skill_repo_token", { host });
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
  ): Promise<ZipInstallResult> {
    return await invoke("install_skills_from_zip", { filePath, currentApp });
  },
};
