import type { DeepLinkImportRequest } from "@/lib/api/deeplink";
import type { CodexCatalogModel, Provider } from "@/types";
import { isOfficialAccount } from "@/utils/providerCapabilities";
import usageTemplate from "../../resources/custom-codex-usage.js?raw";

export const reasoningLevels = [
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
  "ultra",
];

export function isCustomCodexImport(request: DeepLinkImportRequest): boolean {
  return (
    request.resource === "provider" &&
    request.app === "codex" &&
    request.modelCatalog != null
  );
}

export function requiresCodexReview(provider: Provider | undefined): boolean {
  return (
    !!provider &&
    !isOfficialAccount("codex", provider) &&
    provider.settingsConfig?.modelCatalog?.respectDeclaredReasoning === true
  );
}

export function applyCodexImportTemplate(
  request: DeepLinkImportRequest,
): DeepLinkImportRequest {
  if (!isCustomCodexImport(request)) return request;
  const rawModels = request.modelCatalog?.models;
  const models: CodexCatalogModel[] = Array.isArray(rawModels)
    ? rawModels.map((row) => ({
        ...(row && typeof row === "object" ? row : {}),
        model: typeof row?.model === "string" ? row.model : "",
        reasoningLevels:
          Array.isArray(row?.reasoningLevels) &&
          row.reasoningLevels.every((level) => reasoningLevels.includes(level))
            ? row.reasoningLevels
            : [],
      }))
    : request.model
      ? [{ model: request.model }]
      : [];
  return {
    ...request,
    enabled: false,
    usageScript: request.usageScript?.trim()
      ? request.usageScript
      : btoa(usageTemplate),
    modelCatalog: { ...request.modelCatalog, models },
  };
}

export function codexImportError(
  request: DeepLinkImportRequest,
): string | undefined {
  if (!request.endpoint?.trim()) return "codexCustomization.error.endpoint";
  if (!request.apiKey?.trim()) return "codexCustomization.error.apiKey";
  const models = request.modelCatalog?.models ?? [];
  if (!models.length) return "codexCustomization.error.models";
  const names = models.map((row) => row.model.trim());
  if (names.some((name) => !name)) return "codexCustomization.error.modelName";
  if (new Set(names).size !== names.length)
    return "codexCustomization.error.duplicate";
  if (!request.model || !names.includes(request.model))
    return "codexCustomization.error.defaultModel";
  if (
    models.some(
      (row) =>
        !row.reasoningLevels?.length ||
        row.reasoningLevels.some((level) => !reasoningLevels.includes(level)),
    )
  ) {
    return "codexCustomization.error.levels";
  }
}

export function updateImportModel(
  request: DeepLinkImportRequest,
  index: number,
  row: CodexCatalogModel,
) {
  return {
    ...request,
    modelCatalog: {
      ...request.modelCatalog,
      models: request.modelCatalog!.models.map((model, i) =>
        i === index ? row : model,
      ),
    },
  };
}
