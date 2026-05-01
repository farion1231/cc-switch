import type { AppId } from "@/lib/api";
import type { ProviderCategory } from "@/types";

const SUPPORTED_OPENCODE_NPM_PACKAGES = new Set([
  "@ai-sdk/openai-compatible",
  "@ai-sdk/openai",
  "@ai-sdk/anthropic",
]);

const SUPPORTED_OPENCLAW_PROTOCOLS = new Set([
  "openai-completions",
  "openai-responses",
  "anthropic-messages",
]);

interface FullUrlSupportInput {
  appId: AppId;
  category?: ProviderCategory;
  opencodeNpm?: string;
  openclawApi?: string;
}

interface PersistFullUrlInput extends FullUrlSupportInput {
  isFullUrl?: boolean;
}

export function supportsFullUrlMode({
  appId,
  category,
  opencodeNpm,
  openclawApi,
}: FullUrlSupportInput): boolean {
  if (
    category === "official" ||
    category === "omo" ||
    category === "omo-slim"
  ) {
    return false;
  }

  if (appId === "claude" || appId === "codex") {
    return true;
  }

  if (appId === "opencode") {
    return opencodeNpm
      ? SUPPORTED_OPENCODE_NPM_PACKAGES.has(opencodeNpm)
      : false;
  }

  if (appId === "openclaw") {
    return openclawApi
      ? SUPPORTED_OPENCLAW_PROTOCOLS.has(openclawApi)
      : false;
  }

  return false;
}

export function shouldPersistFullUrl({
  isFullUrl,
  ...input
}: PersistFullUrlInput): boolean {
  return isFullUrl === true && supportsFullUrlMode(input);
}
