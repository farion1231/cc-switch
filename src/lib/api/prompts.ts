import { invoke } from "@tauri-apps/api/core";
import type { AppId } from "./types";

export interface Prompt {
  id: string;
  name: string;
  content: string;
  description?: string;
  enabled: boolean;
  createdAt?: number;
  updatedAt?: number;
}

/**
 * 全局提示词：一份内容同时用到多个应用。用到某个应用时，该应用的列表里会出现
 * id 为 `global-<id>` 的一条（见 {@link linkedPromptId}），启用 / 写文件走该应用原有流程。
 */
export interface GlobalPrompt {
  id: string;
  name: string;
  content: string;
  description?: string;
  createdAt?: number;
  updatedAt?: number;
}

export interface GlobalPromptEntry extends GlobalPrompt {
  /** 正在使用这条提示词的应用 */
  apps: Partial<Record<AppId, boolean>>;
  /** 启用中、但目标文件已在别处改过的应用 */
  modifiedApps: AppId[];
}

export interface GlobalPromptSaveResult {
  id: string;
  /** 内容已保存，但同步到这些应用失败 */
  failures: Array<{ app: AppId; error: string }>;
}

export const GLOBAL_PROMPT_LINK_PREFIX = "global-";

/** 全局提示词在各应用列表里对应的那条记录的 id */
export function linkedPromptId(globalId: string): string {
  return `${GLOBAL_PROMPT_LINK_PREFIX}${globalId}`;
}

/** 应用列表里的一条如果是全局提示词的关联记录，返回全局 id */
export function globalIdOfLinked(promptId: string): string | null {
  return promptId.startsWith(GLOBAL_PROMPT_LINK_PREFIX)
    ? promptId.slice(GLOBAL_PROMPT_LINK_PREFIX.length)
    : null;
}

/** 提示词目标文件：path 是完整路径（复制用），displayPath 把主目录写成 ~（显示用）。 */
export interface PromptFileLocation {
  path: string;
  displayPath: string;
}

export type PiPromptFileKind = "system_override" | "system_append";

export interface PiPromptFileSnapshot {
  exists: boolean;
  revision: string;
  content: string;
}

export interface PiPromptTemplate {
  slug: string;
  content: string;
  revision: string;
}

export const promptsApi = {
  async getPrompts(app: AppId): Promise<Record<string, Prompt>> {
    return await invoke("get_prompts", { app });
  },

  async upsertPrompt(app: AppId, id: string, prompt: Prompt): Promise<void> {
    return await invoke("upsert_prompt", { app, id, prompt });
  },

  async deletePrompt(app: AppId, id: string): Promise<void> {
    return await invoke("delete_prompt", { app, id });
  },

  async enablePrompt(app: AppId, id: string): Promise<void> {
    return await invoke("enable_prompt", { app, id });
  },

  async importFromFile(app: AppId): Promise<string> {
    return await invoke("import_prompt_from_file", { app });
  },

  async getCurrentFileContent(app: AppId): Promise<string | null> {
    return await invoke("get_current_prompt_file_content", { app });
  },

  async getFileLocation(app: AppId): Promise<PromptFileLocation> {
    return await invoke("get_prompt_file_location", { app });
  },

  async getGlobalPrompts(): Promise<GlobalPromptEntry[]> {
    return await invoke("get_global_prompts");
  },

  /** id 传空串表示新建，后端生成 id */
  async upsertGlobalPrompt(
    prompt: GlobalPrompt,
  ): Promise<GlobalPromptSaveResult> {
    return await invoke("upsert_global_prompt", { prompt });
  },

  async deleteGlobalPrompt(id: string): Promise<void> {
    return await invoke("delete_global_prompt", { id });
  },

  async setGlobalPromptApp(
    id: string,
    app: AppId,
    enabled: boolean,
  ): Promise<void> {
    return await invoke("set_global_prompt_app", { id, app, enabled });
  },

  /** 把应用里的一条设为全局提示词，返回全局 id */
  async promoteToGlobal(app: AppId, id: string): Promise<string> {
    return await invoke("promote_prompt_to_global", { app, id });
  },

  async getPiPromptFile(kind: PiPromptFileKind): Promise<PiPromptFileSnapshot> {
    return await invoke("get_pi_prompt_file", { kind });
  },

  async replacePiPromptFile(
    kind: PiPromptFileKind,
    expectedRevision: string,
    content: string,
  ): Promise<PiPromptFileSnapshot> {
    return await invoke("replace_pi_prompt_file", {
      kind,
      expectedRevision,
      content,
    });
  },

  async deletePiPromptFile(
    kind: PiPromptFileKind,
    expectedRevision: string,
  ): Promise<boolean> {
    return await invoke("delete_pi_prompt_file", { kind, expectedRevision });
  },

  async listPiPromptTemplates(): Promise<PiPromptTemplate[]> {
    return await invoke("list_pi_prompt_templates");
  },

  async upsertPiPromptTemplate(
    slug: string,
    expectedRevision: string,
    content: string,
    originalSlug?: string,
  ): Promise<PiPromptTemplate> {
    return await invoke("upsert_pi_prompt_template", {
      slug,
      originalSlug: originalSlug ?? null,
      expectedRevision,
      content,
    });
  },

  async deletePiPromptTemplate(
    slug: string,
    expectedRevision: string,
  ): Promise<boolean> {
    return await invoke("delete_pi_prompt_template", {
      slug,
      expectedRevision,
    });
  },
};
