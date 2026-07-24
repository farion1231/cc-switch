import { invoke } from "@tauri-apps/api/core";
import type { TFunction } from "i18next";
import { toast } from "sonner";

export interface FetchedModel {
  id: string;
  ownedBy: string | null;
  contextWindow?: number | null;
  inputModalities?: string[] | null;
  supportsImage?: boolean | null;
}

export interface VolcengineModelListOptions {
  action: string;
  accessKeyId: string;
  secretAccessKey: string;
}

export interface CodexResponsesProbeResult {
  ok: boolean;
  status?: number | null;
  url: string;
  model: string;
  detail: string;
}

/**
 * 从供应商获取可用模型列表
 *
 * 使用 OpenAI 兼容的 GET /v1/models 端点。优先用 `modelsUrl` 精确覆写；
 * 否则后端会对 baseURL 生成候选列表并按序尝试（含"剥离 /anthropic 等兼容子路径"兜底）。
 */
export async function fetchModelsForConfig(
  baseUrl: string,
  apiKey: string,
  isFullUrl?: boolean,
  modelsUrl?: string,
  customUserAgent?: string,
  volcengineModelList?: VolcengineModelListOptions,
): Promise<FetchedModel[]> {
  return invoke("fetch_models_for_config", {
    request: {
      baseUrl,
      apiKey,
      isFullUrl,
      modelsUrl,
      customUserAgent,
      volcengineModelListAction: volcengineModelList?.action,
      volcengineAccessKeyId: volcengineModelList?.accessKeyId,
      volcengineSecretAccessKey: volcengineModelList?.secretAccessKey,
    },
  });
}

/**
 * 对指定 Codex 模型执行 `/v1/responses` 连通性探测。
 *
 * 这个探测会真实请求上游，`max_output_tokens` 为 1024；调用方应只在用户显式确认后触发。
 */
export async function probeCodexResponsesForConfig(
  baseUrl: string,
  apiKey: string,
  model: string,
  isFullUrl?: boolean,
  customUserAgent?: string,
): Promise<CodexResponsesProbeResult> {
  return invoke("probe_codex_responses_for_config", {
    baseUrl,
    apiKey,
    model,
    isFullUrl,
    customUserAgent,
  });
}

/**
 * 对指定 Codex 模型执行 `/v1/chat/completions` 连通性探测。
 *
 * 与 Responses 探测配对使用，用来区分“协议不支持”和“凭据/网络/模型本身不可用”。
 */
export async function probeCodexChatForConfig(
  baseUrl: string,
  apiKey: string,
  model: string,
  isFullUrl?: boolean,
  customUserAgent?: string,
): Promise<CodexResponsesProbeResult> {
  return invoke("probe_codex_chat_for_config", {
    baseUrl,
    apiKey,
    model,
    isFullUrl,
    customUserAgent,
  });
}

/**
 * 获取 Codex OAuth (ChatGPT Plus/Pro 反代) 可用模型列表
 *
 * Codex OAuth 使用 ChatGPT 的 backend-api/codex 端点，不兼容普通 /v1/models。
 */
export async function fetchCodexOauthModels(
  accountId?: string | null,
): Promise<FetchedModel[]> {
  return invoke("get_codex_oauth_models", {
    accountId: accountId || null,
  });
}

/**
 * 读取本地 Codex 官方模型缓存
 *
 * 只作为 ChatGPT OAuth 在线模型接口失败后的离线兜底，不触发登录态刷新或网络请求。
 */
export async function fetchCodexOauthCachedModels(): Promise<FetchedModel[]> {
  return invoke("get_codex_oauth_cached_models");
}

/**
 * 根据错误类型显示对应的 toast 提示
 */
export function showFetchModelsError(
  err: unknown,
  t: TFunction,
  opts?: { hasApiKey: boolean; hasBaseUrl: boolean },
): void {
  // 前端预检：缺少必填字段
  if (opts && !opts.hasBaseUrl && !opts.hasApiKey) {
    toast.error(t("providerForm.fetchModelsNeedConfig"));
    return;
  }
  if (opts && !opts.hasApiKey) {
    toast.error(t("providerForm.fetchModelsNeedApiKey"));
    return;
  }
  if (opts && !opts.hasBaseUrl) {
    toast.error(t("providerForm.fetchModelsNeedEndpoint"));
    return;
  }

  // 解析后端错误字符串
  const msg = String(err);

  if (msg.includes("HTTP 401") || msg.includes("HTTP 403")) {
    toast.error(t("providerForm.fetchModelsAuthFailed"));
    return;
  }
  // 所有候选端点均返回 404/405：供应商可能未开放 /models 接口，或 Base URL 有误
  if (msg.includes("All candidates failed")) {
    toast.error(t("providerForm.fetchModelsEndpointNotFound"));
    return;
  }
  if (msg.includes("HTTP 404") || msg.includes("HTTP 405")) {
    toast.error(t("providerForm.fetchModelsEndpointNotFound"));
    return;
  }
  if (msg.includes("timeout") || msg.includes("timed out")) {
    toast.error(t("providerForm.fetchModelsTimeout"));
    return;
  }
  if (msg.includes("Failed to parse")) {
    toast.error(t("providerForm.fetchModelsNotSupported"));
    return;
  }

  // 通用兜底
  toast.error(t("providerForm.fetchModelsFailed"));
}
