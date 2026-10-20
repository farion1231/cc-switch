import type { DeepLinkImportRequest } from "@/lib/api/deeplink";
import type { CodexCatalogModel } from "@/types";
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

export function applyCodexImportTemplate(
  request: DeepLinkImportRequest,
): DeepLinkImportRequest {
  if (request.resource !== "provider" || request.app !== "codex")
    return request;
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
  if (!request.endpoint?.trim()) return "请填写 API 地址";
  if (!request.apiKey?.trim()) return "请填写 API Key";
  const models = request.modelCatalog?.models ?? [];
  if (!models.length) return "请添加模型列表";
  const names = models.map((row) => row.model.trim());
  if (names.some((name) => !name)) return "请填写每个模型的名称";
  if (new Set(names).size !== names.length) return "模型名称不能重复";
  if (!request.model || !names.includes(request.model))
    return "请选择列表中的默认模型";
  if (
    models.some(
      (row) =>
        !row.reasoningLevels?.length ||
        row.reasoningLevels.some((level) => !reasoningLevels.includes(level)),
    )
  ) {
    return "请逐个确认模型支持的思考等级";
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
