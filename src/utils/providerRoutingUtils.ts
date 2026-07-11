type ProviderLike = {
  settingsConfig?: unknown;
  meta?: {
    providerType?: string;
    usage_script?: {
      templateType?: string;
      template_type?: string;
    };
  } | null;
};

function parseSettingsConfig(raw: unknown): Record<string, any> | null {
  if (raw && typeof raw === "object") {
    return raw as Record<string, any>;
  }

  if (typeof raw === "string") {
    try {
      const parsed = JSON.parse(raw) as unknown;
      if (parsed && typeof parsed === "object") {
        return parsed as Record<string, any>;
      }
    } catch {
      return null;
    }
  }

  return null;
}

export function extractProviderBaseUrl(settingsConfig: unknown): string {
  const config = parseSettingsConfig(settingsConfig);
  const candidates = [
    config?.options?.baseURL,
    config?.options?.baseUrl,
    config?.baseURL,
    config?.base_url,
  ];
  const first = candidates.find((value) => typeof value === "string");
  return typeof first === "string" ? first : "";
}

export function isCopilotProvider(provider: ProviderLike): boolean {
  if (provider.meta?.providerType === "github_copilot") {
    return true;
  }

  const templateType =
    provider.meta?.usage_script?.templateType ??
    provider.meta?.usage_script?.template_type;
  if (templateType === "github_copilot") {
    return true;
  }

  return extractProviderBaseUrl(provider.settingsConfig)
    .toLowerCase()
    .includes("githubcopilot.com");
}
