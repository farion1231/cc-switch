import { get, post, put, del } from "../web-client";
import type { Prompt } from "../prompts";
import type { AppId } from "../types";

export const promptsApi = {
  async getPrompts(app: AppId): Promise<Record<string, Prompt>> {
    const prompts = await get<Prompt[]>(`/prompts?app=${app}`);
    return Object.fromEntries(prompts.map((prompt) => [prompt.id, prompt]));
  },

  async getPrompt(id: string): Promise<Prompt | null> {
    return get(`/prompts/${id}`);
  },

  async upsertPrompt(_app: AppId, id: string, prompt: Prompt): Promise<void> {
    if (id) {
      await put(`/prompts/${encodeURIComponent(id)}`, prompt);
      return;
    }
    await post("/prompts", prompt);
  },

  async deletePrompt(_app: AppId, id: string): Promise<void> {
    await del(`/prompts/${encodeURIComponent(id)}`);
  },

  async enablePrompt(_app: AppId, id: string): Promise<void> {
    await post(`/prompts/${encodeURIComponent(id)}/activate`, {});
  },

  async importFromFile(app: AppId): Promise<string> {
    return post("/prompts/import", { app });
  },

  async getCurrentFileContent(app: AppId): Promise<string | null> {
    const response = await get<{ content: string }>(
      `/prompts/current-content?app=${app}`,
    );
    return response.content ?? null;
  },
};
