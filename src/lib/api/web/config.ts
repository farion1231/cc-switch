import { get, post, put } from "../web-client";

export type AppType = "claude" | "codex" | "gemini" | "omo" | "omo_slim";

export async function getClaudeCommonConfigSnippet(): Promise<string | null> {
  return getCommonConfigSnippet("claude");
}

export async function setClaudeCommonConfigSnippet(
  snippet: string,
): Promise<void> {
  await setCommonConfigSnippet("claude", snippet);
}

export async function getCommonConfigSnippet(
  appType: AppType,
): Promise<string | null> {
  return get(`/settings/common-config/${encodeURIComponent(appType)}`);
}

export async function setCommonConfigSnippet(
  appType: AppType,
  snippet: string,
): Promise<void> {
  await put(`/settings/common-config/${encodeURIComponent(appType)}`, {
    snippet,
  });
}

export type ExtractCommonConfigSnippetOptions = {
  settingsConfig?: string;
};

export async function extractCommonConfigSnippet(
  appType: Exclude<AppType, "omo">,
  options?: ExtractCommonConfigSnippetOptions,
): Promise<string> {
  return post("/settings/common-config/extract", {
    appType,
    settingsConfig: options?.settingsConfig,
  });
}
