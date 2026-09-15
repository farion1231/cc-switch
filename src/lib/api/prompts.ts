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

export type OhMyPiPromptFileKind =
  | "agents"
  | "system_override"
  | "system_append";

export interface OhMyPiPromptFileSnapshot {
  exists: boolean;
  revision: string;
  content: string;
}

export interface OhMyPiPromptTemplate {
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

  async getOhMyPiPromptFile(
    kind: OhMyPiPromptFileKind,
  ): Promise<OhMyPiPromptFileSnapshot> {
    return await invoke("get_ohmypi_prompt_file", { kind });
  },

  async replaceOhMyPiPromptFile(
    kind: OhMyPiPromptFileKind,
    expectedRevision: string,
    content: string,
  ): Promise<OhMyPiPromptFileSnapshot> {
    return await invoke("replace_ohmypi_prompt_file", {
      kind,
      expectedRevision,
      content,
    });
  },

  async deleteOhMyPiPromptFile(
    kind: OhMyPiPromptFileKind,
    expectedRevision: string,
  ): Promise<boolean> {
    return await invoke("delete_ohmypi_prompt_file", {
      kind,
      expectedRevision,
    });
  },

  async listOhMyPiPromptTemplates(): Promise<OhMyPiPromptTemplate[]> {
    return await invoke("list_ohmypi_prompt_templates");
  },

  async upsertOhMyPiPromptTemplate(
    slug: string,
    expectedRevision: string,
    content: string,
    originalSlug?: string,
  ): Promise<OhMyPiPromptTemplate> {
    return await invoke("upsert_ohmypi_prompt_template", {
      slug,
      originalSlug: originalSlug ?? null,
      expectedRevision,
      content,
    });
  },

  async deleteOhMyPiPromptTemplate(
    slug: string,
    expectedRevision: string,
  ): Promise<boolean> {
    return await invoke("delete_ohmypi_prompt_template", {
      slug,
      expectedRevision,
    });
  },
};
