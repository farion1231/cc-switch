import { invoke } from "@tauri-apps/api/core";

import type { AppId } from "@/lib/api/types";

export type AppType = "claude" | "codex" | "gemini" | "opencode" | "openclaw";

/** Rule 应用启用状态 */
export interface RuleApps {
  claude: boolean;
  codex: boolean;
  gemini: boolean;
  opencode: boolean;
  openclaw: boolean;
}

/** 已安装的 Rule（统一结构） */
export interface InstalledRule {
  id: string;
  name: string;
  description?: string;
  directory: string;
  repoOwner?: string;
  repoName?: string;
  repoBranch?: string;
  readmeUrl?: string;
  apps: RuleApps;
  installedAt: number;
}

export interface RuleUninstallResult {
  backupPath?: string;
}

export interface RuleBackupEntry {
  backupId: string;
  backupPath: string;
  createdAt: number;
  rule: InstalledRule;
}

/** 可发现的 Rule（来自仓库） */
export interface DiscoverableRule {
  key: string;
  name: string;
  description: string;
  directory: string;
  readmeUrl?: string;
  repoOwner: string;
  repoName: string;
  repoBranch: string;
}

/** 未管理的 Rule（用于导入） */
export interface UnmanagedRule {
  directory: string;
  name: string;
  description?: string;
  foundIn: string[];
  path: string;
}

/** 导入已有 Rule 时提交的应用启用状态 */
export interface ImportRuleSelection {
  directory: string;
  apps: RuleApps;
}

/** 规则对象（兼容旧 API） */
export interface Rule {
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
export interface RuleRepo {
  owner: string;
  name: string;
  branch: string;
  enabled: boolean;
}

// ========== API ==========

export const rulesApi = {
  // ========== 统一管理 API ==========

  /** 获取所有已安装的 Rules */
  async getInstalled(): Promise<InstalledRule[]> {
    return await invoke("get_installed_rules");
  },

  /** 获取可恢复的 Rule 备份列表 */
  async getBackups(): Promise<RuleBackupEntry[]> {
    return await invoke("get_rule_backups");
  },

  /** 删除 Rule 备份 */
  async deleteBackup(backupId: string): Promise<boolean> {
    return await invoke("delete_rule_backup", { backupId });
  },

  /** 安装 Rule（统一安装） */
  async installUnified(
    rule: DiscoverableRule,
    currentApp: AppId,
  ): Promise<InstalledRule> {
    return await invoke("install_rule_unified", { rule, currentApp });
  },

  /** 卸载 Rule（统一卸载） */
  async uninstallUnified(id: string): Promise<RuleUninstallResult> {
    return await invoke("uninstall_rule_unified", { id });
  },

  /** 从备份恢复 Rule */
  async restoreBackup(
    backupId: string,
    currentApp: AppId,
  ): Promise<InstalledRule> {
    return await invoke("restore_rule_backup", { backupId, currentApp });
  },

  /** 切换 Rule 的应用启用状态 */
  async toggleApp(id: string, app: AppId, enabled: boolean): Promise<boolean> {
    return await invoke("toggle_rule_app", { id, app, enabled });
  },

  /** 扫描未管理的 Rules */
  async scanUnmanaged(): Promise<UnmanagedRule[]> {
    return await invoke("scan_unmanaged_rules");
  },

  /** 从应用目录导入 Rules */
  async importFromApps(
    imports: ImportRuleSelection[],
  ): Promise<InstalledRule[]> {
    return await invoke("import_rules_from_apps", { imports });
  },

  /** 发现可安装的 Rules（从仓库获取） */
  async discoverAvailable(): Promise<DiscoverableRule[]> {
    return await invoke("discover_available_rules");
  },

  // ========== 兼容旧 API ==========

  /** 获取规则列表（兼容旧 API） */
  async getAll(app: AppId = "claude"): Promise<Rule[]> {
    if (app === "claude") {
      return await invoke("get_rules");
    }
    return await invoke("get_rules_for_app", { app });
  },

  /** 安装规则（兼容旧 API） */
  async install(directory: string, app: AppId = "claude"): Promise<boolean> {
    if (app === "claude") {
      return await invoke("install_rule", { directory });
    }
    return await invoke("install_rule_for_app", { app, directory });
  },

  /** 卸载规则（兼容旧 API） */
  async uninstall(
    directory: string,
    app: AppId = "claude",
  ): Promise<RuleUninstallResult> {
    if (app === "claude") {
      return await invoke("uninstall_rule", { directory });
    }
    return await invoke("uninstall_rule_for_app", { app, directory });
  },

  // ========== 仓库管理 ==========

  /** 获取仓库列表 */
  async getRepos(): Promise<RuleRepo[]> {
    return await invoke("get_rule_repos");
  },

  /** 添加仓库 */
  async addRepo(repo: RuleRepo): Promise<boolean> {
    return await invoke("add_rule_repo", { repo });
  },

  /** 删除仓库 */
  async removeRepo(owner: string, name: string): Promise<boolean> {
    return await invoke("remove_rule_repo", { owner, name });
  },

  // ========== ZIP 安装 ==========

  /** 打开 ZIP 文件选择对话框 */
  async openZipFileDialog(): Promise<string | null> {
    return await invoke("open_zip_file_dialog");
  },

  /** 从 ZIP 文件安装 Rules */
  async installFromZip(
    filePath: string,
    currentApp: AppId,
  ): Promise<InstalledRule[]> {
    return await invoke("install_rules_from_zip", { filePath, currentApp });
  },
};
