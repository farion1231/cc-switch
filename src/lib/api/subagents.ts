import { invoke } from "@tauri-apps/api/core";

/**
 * Claude Code 用户级 subagent（~/.claude/agents/*.md）。
 * 库里存整份 markdown 原文，启用时写进目录；status 是库与磁盘文件的对照结果。
 */
export type SubagentStatus = "synced" | "drifted" | "missing" | "disabled";

export interface Subagent {
  id: string;
  name: string;
  fileName: string;
  description?: string;
  content: string;
  enabled: boolean;
  source: "local" | "imported" | "repo";
  status: SubagentStatus;
  filePath: string;
  createdAt?: number;
  updatedAt?: number;
}

export interface SubagentInput {
  id?: string;
  fileName?: string;
  content: string;
  enabled: boolean;
}

/** agents 目录里有、库里没有的文件（可导入）；frontmatter 解析失败时带 parseError */
export interface UnmanagedSubagent {
  fileName: string;
  path: string;
  name?: string;
  description?: string;
  content: string;
  parseError?: string;
}

/** 插件自带的 subagent：只读展示，由插件自己管理 */
export interface PluginSubagent {
  pluginName: string;
  name: string;
  description?: string;
  model?: string;
  path: string;
}

/** path 是完整路径（复制用），displayPath 把主目录写成 ~（显示用） */
export interface SubagentsDirInfo {
  path: string;
  displayPath: string;
  exists: boolean;
}

export type SubagentDriftStrategy = "adopt_disk" | "overwrite_disk";

export type SubagentRoutingMode = "direct" | "route" | "stack";

export interface SubagentModelOption {
  value: string;
  kind: "inherit" | "alias" | "model";
  label: string;
  resolvesTo?: string;
  providerName?: string;
  description?: string;
  warning?: "alias_unmapped";
}

export type SubagentModelNotice =
  | "route_aliases_only"
  | "stack_aliases_share_default"
  | "no_provider"
  | "subagent_model_override";

export interface SubagentModelOptions {
  mode: SubagentRoutingMode;
  providerId?: string;
  providerName?: string;
  isOfficial: boolean;
  options: SubagentModelOption[];
  subagentModelOverride?: string;
  notices: SubagentModelNotice[];
}

/** 后端冲突类错误的前缀（错误是 `CODE: 详情` 形式的字符串） */
export type SubagentErrorCode =
  | "SUBAGENT_DRIFTED"
  | "SUBAGENT_FILE_EXISTS"
  | "SUBAGENT_INVALID"
  | "SUBAGENT_NAME_TAKEN"
  | "SUBAGENT_NOT_FOUND";

export const subagentsApi = {
  async list(): Promise<Subagent[]> {
    return await invoke("list_subagents");
  },

  async getDir(): Promise<SubagentsDirInfo> {
    return await invoke("get_subagents_dir");
  },

  async upsert(input: SubagentInput, force = false): Promise<Subagent> {
    return await invoke("upsert_subagent", { input, force });
  },

  async delete(id: string, force = false): Promise<void> {
    return await invoke("delete_subagent", { id, force });
  },

  async toggle(id: string, enabled: boolean, force = false): Promise<Subagent> {
    return await invoke("toggle_subagent", { id, enabled, force });
  },

  async scanUnmanaged(): Promise<UnmanagedSubagent[]> {
    return await invoke("scan_unmanaged_subagents");
  },

  async import(fileNames: string[]): Promise<Subagent[]> {
    return await invoke("import_subagents", { fileNames });
  },

  async resolveDrift(
    id: string,
    strategy: SubagentDriftStrategy,
  ): Promise<Subagent> {
    return await invoke("resolve_subagent_drift", { id, strategy });
  },

  async getDiskContent(id: string): Promise<string | null> {
    return await invoke("get_subagent_disk_content", { id });
  },

  async listPlugin(): Promise<PluginSubagent[]> {
    return await invoke("list_plugin_subagents");
  },

  async getModelOptions(): Promise<SubagentModelOptions> {
    return await invoke("get_subagent_model_options");
  },
};
