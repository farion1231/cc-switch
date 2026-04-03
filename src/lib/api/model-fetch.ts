import { invoke } from "@tauri-apps/api/core";

export interface FetchedModel {
  id: string;
  ownedBy: string | null;
}

/**
 * 从供应商获取可用模型列表
 *
 * 使用 OpenAI 兼容的 GET /v1/models 端点。
 * 主要面向第三方聚合站（硅基流动、OpenRouter 等）。
 */
export async function fetchModelsForConfig(
  baseUrl: string,
  apiKey: string,
  isFullUrl?: boolean,
): Promise<FetchedModel[]> {
  return invoke("fetch_models_for_config", { baseUrl, apiKey, isFullUrl });
}
