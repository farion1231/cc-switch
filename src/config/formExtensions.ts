import React from "react";
import type { ProviderMeta } from "@/types";
import type { AppId } from "@/lib/api/types";

export interface ProviderFormSaveContext {
  appId: AppId;
  presetAppTarget?: string;
  selectedAccountId?: string | null;
}

export interface ProviderFormExtension {
  providerType: string;
  skipApiKeyRequired?: boolean;
  defaultMetaFields?: Record<string, unknown>;
  AuthSection?: React.LazyExoticComponent<
    React.ComponentType<Record<string, unknown>>
  >;
  fetchModels?: (
    accountId?: string | null,
  ) => Promise<{ id: string; ownedBy: string | null }[]>;
  forceDesktopProxy?: boolean;
  onSaveMeta?: (context: ProviderFormSaveContext) => Partial<ProviderMeta>;
}

const extensions = new Map<string, ProviderFormExtension>();

export function registerFormExtension(ext: ProviderFormExtension): void {
  extensions.set(ext.providerType, ext);
}

export function getFormExtension(
  providerType: string,
): ProviderFormExtension | undefined {
  return extensions.get(providerType);
}

export function getFormExtensionSaveMeta(
  providerType: string | undefined,
  context: ProviderFormSaveContext,
): Partial<ProviderMeta> {
  return getFormExtension(providerType ?? "")?.onSaveMeta?.(context) ?? {};
}
