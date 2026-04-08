import { invoke } from "@tauri-apps/api/core";

import type { AppId } from "@/lib/api/types";

export type AppType = "claude" | "codex" | "gemini" | "opencode" | "openclaw";

/** Agent 应用启用状态 */
export interface AgentApps {
  claude: boolean;
  codex: boolean;
  gemini: boolean;
  opencode: boolean;
  openclaw: boolean;
}

/** 已安装的 Agent（统一结构） */
export interface InstalledAgent {
  id: string;
  name: string;
  description?: string;
  directory: string;
  repoOwner?: string;
  repoName?: string;
  repoBranch?: string;
  readmeUrl?: string;
  apps: AgentApps;
  installedAt: number;
}

export interface AgentUninstallResult {
  backupPath?: string;
}

export interface AgentBackupEntry {
  backupId: string;
  backupPath: string;
  createdAt: number;
  agent: InstalledAgent;
}

/** 可发现的 Agent（来自仓库） */
export interface DiscoverableAgent {
  key: string;
  name: string;
  description: string;
  directory: string;
  readmeUrl?: string;
  repoOwner: string;
  repoName: string;
  repoBranch: string;
}

/** 未管理的 Agent（用于导入） */
export interface UnmanagedAgent {
  directory: string;
  name: string;
  description?: string;
  foundIn: string[];
  path: string;
}

/** 导入已有 Agent 时提交的应用启用状态 */
export interface ImportAgentSelection {
  directory: string;
  apps: AgentApps;
}

/** Agent 对象（兼容旧 API） */
export interface Agent {
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
export interface AgentRepo {
  owner: string;
  name: string;
  branch: string;
  enabled: boolean;
}

// ========== API ==========

export const agentsApi = {
  // ========== 统一管理 API ==========

  /** 获取所有已安装的 Agents */
  async getInstalled(): Promise<InstalledAgent[]> {
    return await invoke("get_installed_agents");
  },

  /** 获取可恢复的 Agent 备份列表 */
  async getBackups(): Promise<AgentBackupEntry[]> {
    return await invoke("get_agent_backups");
  },

  /** 删除 Agent 备份 */
  async deleteBackup(backupId: string): Promise<boolean> {
    return await invoke("delete_agent_backup", { backupId });
  },

  /** 安装 Agent（统一安装） */
  async installUnified(
    agent: DiscoverableAgent,
    currentApp: AppId,
  ): Promise<InstalledAgent> {
    return await invoke("install_agent_unified", { agent, currentApp });
  },

  /** 卸载 Agent（统一卸载） */
  async uninstallUnified(id: string): Promise<AgentUninstallResult> {
    return await invoke("uninstall_agent_unified", { id });
  },

  /** 从备份恢复 Agent */
  async restoreBackup(
    backupId: string,
    currentApp: AppId,
  ): Promise<InstalledAgent> {
    return await invoke("restore_agent_backup", { backupId, currentApp });
  },

  /** 切换 Agent 的应用启用状态 */
  async toggleApp(id: string, app: AppId, enabled: boolean): Promise<boolean> {
    return await invoke("toggle_agent_app", { id, app, enabled });
  },

  /** 扫描未管理的 Agents */
  async scanUnmanaged(): Promise<UnmanagedAgent[]> {
    return await invoke("scan_unmanaged_agents");
  },

  /** 从应用目录导入 Agents */
  async importFromApps(
    imports: ImportAgentSelection[],
  ): Promise<InstalledAgent[]> {
    return await invoke("import_agents_from_apps", { imports });
  },

  /** 发现可安装的 Agents（从仓库获取） */
  async discoverAvailable(): Promise<DiscoverableAgent[]> {
    return await invoke("discover_available_agents");
  },

  // ========== 兼容旧 API ==========

  /** 获取 Agent 列表（兼容旧 API） */
  async getAll(app: AppId = "claude"): Promise<Agent[]> {
    if (app === "claude") {
      return await invoke("get_agents");
    }
    return await invoke("get_agents_for_app", { app });
  },

  /** 安装 Agent（兼容旧 API） */
  async install(directory: string, app: AppId = "claude"): Promise<boolean> {
    if (app === "claude") {
      return await invoke("install_agent", { directory });
    }
    return await invoke("install_agent_for_app", { app, directory });
  },

  /** 卸载 Agent（兼容旧 API） */
  async uninstall(
    directory: string,
    app: AppId = "claude",
  ): Promise<AgentUninstallResult> {
    if (app === "claude") {
      return await invoke("uninstall_agent", { directory });
    }
    return await invoke("uninstall_agent_for_app", { app, directory });
  },

  // ========== 仓库管理 ==========

  /** 获取仓库列表 */
  async getRepos(): Promise<AgentRepo[]> {
    return await invoke("get_agent_repos");
  },

  /** 添加仓库 */
  async addRepo(repo: AgentRepo): Promise<boolean> {
    return await invoke("add_agent_repo", { repo });
  },

  /** 删除仓库 */
  async removeRepo(owner: string, name: string): Promise<boolean> {
    return await invoke("remove_agent_repo", { owner, name });
  },

  // ========== ZIP 安装 ==========

  /** 打开 ZIP 文件选择对话框 */
  async openZipFileDialog(): Promise<string | null> {
    return await invoke("open_zip_file_dialog");
  },

  /** 从 ZIP 文件安装 Agents */
  async installFromZip(
    filePath: string,
    currentApp: AppId,
  ): Promise<InstalledAgent[]> {
    return await invoke("install_agents_from_zip", { filePath, currentApp });
  },
};
