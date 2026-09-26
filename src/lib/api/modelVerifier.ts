import type { Provider } from "@/types";
import { usageApi } from "./usage";
import type { AppId } from "./types";
import { parse as parseToml } from "smol-toml";
import {
  codexApiFormatFromWireApi,
  extractCodexBaseUrl,
  extractCodexModelName,
  extractCodexWireApi,
} from "@/utils/providerConfigUtils";

export type VerificationVerdict =
  | "officialEndpointConsistent"
  | "reportedExactMatch"
  | "reportedFamilyMatch"
  | "reportedMismatch"
  | "insufficientEvidence"
  | "requestFailed";

export interface VerificationEvidence {
  id: string;
  passed: boolean;
  weight: number;
  summary: string;
}

export interface ModelVerificationResult {
  success: boolean;
  providerId: string;
  providerName: string;
  requestedModel: string;
  reportedModel?: string | null;
  protocol: "openai_responses" | "openai_chat";
  endpoint: string;
  endpointHost?: string | null;
  officialOpenaiEndpoint: boolean;
  responseTimeMs: number;
  responseId?: string | null;
  responseObject?: string | null;
  systemFingerprint?: string | null;
  hasUsage: boolean;
  confidenceScore: number;
  verdict: VerificationVerdict;
  evidence: VerificationEvidence[];
  error?: string | null;
  testedAt: number;
}

interface ProbePayload {
  reportedModel: string | null;
  responseId: string | null;
  responseObject: string | null;
  systemFingerprint: string | null;
  hasUsage: boolean;
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  Boolean(value && typeof value === "object" && !Array.isArray(value));

const sameModelFamily = (requested: string, reported: string): boolean => {
  const left = requested.trim().toLowerCase();
  const right = reported.trim().toLowerCase();
  if (!left || !right) return false;
  if (left === right) return true;

  const boundaryPrefix = (shorter: string, longer: string) => {
    if (!longer.startsWith(shorter)) return false;
    const rest = longer.slice(shorter.length);
    return rest.startsWith("-") || rest.startsWith("_");
  };

  return boundaryPrefix(left, right) || boundaryPrefix(right, left);
};

const extractCodexQueryParams = (
  configText: string,
): Record<string, string> => {
  if (!configText.trim()) return {};

  try {
    const parsed = parseToml(configText) as unknown;
    if (!isRecord(parsed)) return {};

    const providerName =
      typeof parsed.model_provider === "string"
        ? parsed.model_provider.trim()
        : "";
    const providerTables = isRecord(parsed.model_providers)
      ? parsed.model_providers
      : null;
    const activeProvider =
      providerName && providerTables && isRecord(providerTables[providerName])
        ? providerTables[providerName]
        : null;

    const rawParams =
      activeProvider && isRecord(activeProvider.query_params)
        ? activeProvider.query_params
        : isRecord(parsed.query_params)
          ? parsed.query_params
          : null;
    if (!rawParams) return {};

    return Object.fromEntries(
      Object.entries(rawParams).flatMap(([key, value]) => {
        if (
          value === null ||
          value === undefined ||
          typeof value === "object"
        ) {
          return [];
        }
        return [[key, String(value)]];
      }),
    );
  } catch {
    return {};
  }
};

const buildEndpoint = (
  baseUrl: string,
  leaf: string,
  isFullUrl: boolean,
  queryParams: Record<string, string>,
): string => {
  const rawBase = baseUrl.trim();
  if (!rawBase) return "";

  // Full-URL providers are already configured with the production endpoint.
  // Do not append a protocol leaf or rewrite their query string.
  if (isFullUrl) return rawBase;

  try {
    const parsed = new URL(rawBase);
    const path = parsed.pathname.replace(/\/+$/, "");
    const basePath = !path || path === "/" ? "/v1" : path;
    parsed.pathname = `${basePath}/${leaf}`.replace(/\/{2,}/g, "/");

    for (const [key, value] of Object.entries(queryParams)) {
      parsed.searchParams.set(key, value);
    }

    return parsed.toString();
  } catch {
    const base = rawBase.replace(/\/+$/, "");
    const endpoint = `${base}/${leaf}`;
    const query = new URLSearchParams(queryParams).toString();
    if (!query) return endpoint;
    return `${endpoint}${endpoint.includes("?") ? "&" : "?"}${query}`;
  }
};

const buildProbeScript = (
  requestedModel: string,
  protocol: "openai_responses" | "openai_chat",
  endpoint: string,
  customUserAgent?: string,
): string => {
  const requestBody =
    protocol === "openai_chat"
      ? {
          model: requestedModel,
          messages: [{ role: "user", content: "Reply with exactly OK." }],
          stream: false,
        }
      : {
          model: requestedModel,
          input: "Reply with exactly OK.",
          stream: false,
        };

  const headers: Record<string, string> = {
    Authorization: "Bearer {{apiKey}}",
    "Content-Type": "application/json",
    Accept: "application/json",
    "Accept-Encoding": "identity",
  };
  if (customUserAgent?.trim()) headers["User-Agent"] = customUserAgent.trim();

  const headersJson = JSON.stringify(headers);
  const bodyJson = JSON.stringify(JSON.stringify(requestBody));
  const endpointJson = JSON.stringify(endpoint);

  return `({
    request: {
      url: ${endpointJson},
      method: "POST",
      headers: ${headersJson},
      body: ${bodyJson}
    },
    extractor: function (response) {
      var reportedModel = response && typeof response.model === "string" ? response.model : null;
      return {
        planName: reportedModel,
        extra: JSON.stringify({
          reportedModel: reportedModel,
          responseId: response && typeof response.id === "string" ? response.id : null,
          responseObject: response && typeof response.object === "string" ? response.object : null,
          systemFingerprint: response && typeof response.system_fingerprint === "string" ? response.system_fingerprint : null,
          hasUsage: !!(response && response.usage)
        })
      };
    }
  })`;
};

const assess = (
  requestedModel: string,
  payload: ProbePayload | null,
  officialEndpoint: boolean,
  requestSucceeded: boolean,
): Pick<
  ModelVerificationResult,
  "confidenceScore" | "verdict" | "evidence"
> => {
  const reported = payload?.reportedModel?.trim() || null;
  const exactMatch = Boolean(
    reported && reported.toLowerCase() === requestedModel.trim().toLowerCase(),
  );
  const familyMatch = Boolean(
    reported && sameModelFamily(requestedModel, reported),
  );

  const evidence: VerificationEvidence[] = [
    {
      id: "request_success",
      passed: requestSucceeded,
      weight: 20,
      summary: requestSucceeded
        ? "真实 API 请求成功并返回可解析 JSON"
        : "真实 API 请求失败",
    },
    {
      id: "reported_model",
      passed: Boolean(reported),
      weight: 20,
      summary: reported
        ? `服务端响应声明模型为 \`${reported}\``
        : "响应没有提供 model 字段",
    },
    {
      id: "model_match",
      passed: exactMatch || familyMatch,
      weight: 30,
      summary: exactMatch
        ? "返回模型与配置模型完全一致"
        : familyMatch
          ? "返回模型与配置模型属于同一别名/模型系列"
          : reported
            ? "返回模型与配置模型不一致"
            : "缺少可比较的模型标识",
    },
    {
      id: "official_endpoint",
      passed: officialEndpoint,
      weight: 20,
      summary: officialEndpoint
        ? "请求目标是 api.openai.com"
        : "请求目标是第三方 API；其返回字段理论上可以被伪造",
    },
    {
      id: "response_identity",
      passed: Boolean(payload?.responseId || payload?.responseObject),
      weight: 5,
      summary:
        payload?.responseId || payload?.responseObject
          ? "响应包含 id/object 等协议身份字段"
          : "未观察到 id/object 协议身份字段",
    },
    {
      id: "system_fingerprint",
      passed: Boolean(payload?.systemFingerprint),
      weight: 5,
      summary: payload?.systemFingerprint
        ? "响应包含 system_fingerprint"
        : "响应未提供 system_fingerprint",
    },
  ];

  let score = evidence
    .filter((item) => item.passed)
    .reduce((sum, item) => sum + item.weight, 0);
  if (reported && !exactMatch && !familyMatch) score = Math.max(0, score - 20);
  if (!officialEndpoint) score = Math.min(score, 74);

  const verdict: VerificationVerdict = !requestSucceeded
    ? "requestFailed"
    : reported && !exactMatch && !familyMatch
      ? "reportedMismatch"
      : officialEndpoint && (exactMatch || familyMatch)
        ? "officialEndpointConsistent"
        : exactMatch
          ? "reportedExactMatch"
          : familyMatch
            ? "reportedFamilyMatch"
            : "insufficientEvidence";

  return { confidenceScore: Math.min(score, 100), verdict, evidence };
};

export const modelVerifierApi = {
  verifyProvider: async (
    appType: AppId,
    provider: Provider,
  ): Promise<ModelVerificationResult> => {
    if (appType !== "codex") {
      throw new Error("Model Verifier v1 仅支持 Codex Provider");
    }

    const settings = provider.settingsConfig as Record<string, unknown>;
    const configText =
      typeof settings.config === "string" ? settings.config : "";
    const requestedModel =
      (typeof settings.model === "string" ? settings.model.trim() : "") ||
      extractCodexModelName(configText)?.trim() ||
      "";
    if (!requestedModel)
      throw new Error("该 Provider 没有配置可核验的上游模型");

    const apiFormat =
      provider.meta?.apiFormat ??
      (typeof settings.apiFormat === "string"
        ? settings.apiFormat
        : undefined) ??
      (typeof settings.api_format === "string"
        ? settings.api_format
        : undefined) ??
      codexApiFormatFromWireApi(extractCodexWireApi(configText)) ??
      "openai_responses";
    if (apiFormat === "anthropic") {
      throw new Error(
        "Model Verifier v1 暂不支持 Anthropic-wire Codex Provider",
      );
    }

    const protocol: "openai_responses" | "openai_chat" =
      apiFormat === "openai_chat" ? "openai_chat" : "openai_responses";
    const baseUrl = extractCodexBaseUrl(configText) ?? "";
    if (!baseUrl) throw new Error("无法从 Provider 配置中解析 Base URL");

    const queryParams = extractCodexQueryParams(configText);
    const endpoint = buildEndpoint(
      baseUrl,
      protocol === "openai_chat" ? "chat/completions" : "responses",
      provider.meta?.isFullUrl === true,
      queryParams,
    );
    const endpointHost = (() => {
      try {
        return new URL(endpoint).hostname || null;
      } catch {
        return null;
      }
    })();
    const officialOpenaiEndpoint = endpointHost === "api.openai.com";

    const script = buildProbeScript(
      requestedModel,
      protocol,
      endpoint,
      provider.meta?.customUserAgent,
    );
    const started = performance.now();
    const usageResult = await usageApi.testScript(
      provider.id,
      appType,
      script,
      30,
    );
    const responseTimeMs = Math.round(performance.now() - started);

    let payload: ProbePayload | null = null;
    const extra = usageResult.data?.[0]?.extra;
    if (typeof extra === "string" && extra.trim()) {
      try {
        payload = JSON.parse(extra) as ProbePayload;
      } catch {
        payload = null;
      }
    }

    const assessment = assess(
      requestedModel,
      payload,
      officialOpenaiEndpoint,
      usageResult.success,
    );

    return {
      success: usageResult.success,
      providerId: provider.id,
      providerName: provider.name,
      requestedModel,
      reportedModel: payload?.reportedModel ?? null,
      protocol,
      endpoint,
      endpointHost,
      officialOpenaiEndpoint,
      responseTimeMs,
      responseId: payload?.responseId ?? null,
      responseObject: payload?.responseObject ?? null,
      systemFingerprint: payload?.systemFingerprint ?? null,
      hasUsage: payload?.hasUsage ?? false,
      confidenceScore: assessment.confidenceScore,
      verdict: assessment.verdict,
      evidence: assessment.evidence,
      error: usageResult.error ?? null,
      testedAt: Math.floor(Date.now() / 1000),
    };
  },
};
