import type {
  ExternalOpenAIAPIBackendOption,
  ExternalOpenAIAPIProfileUpdate,
} from "@/types/proxy";

export type BackendGroup = {
  key: string;
  label: string;
  tone: "blue" | "emerald" | "amber" | "slate";
  options: ExternalOpenAIAPIBackendOption[];
};

export type BackendTargetDescription = {
  kind: string;
  protocol: string;
  auth: string;
  modelSource: string;
  compatibility: string[];
};

/// 把 app id 转成界面上更自然的分组名，避免用户看到内部枚举。
export function appDisplayName(appType: string): string {
  const names: Record<string, string> = {
    claude: "Claude",
    "claude-desktop": "Claude Desktop",
    codex: "Codex",
    gemini: "Gemini",
    opencode: "opencode",
    openclaw: "OpenClaw",
    hermes: "Hermes",
  };
  return names[appType] ?? appType;
}

/// 将后端描述转换成中文；后端仍保留原始英文值用于判断和调试。
export function displayBackendDescription(description: string): string {
  const normalized = description.trim();
  const translations: Record<string, string> = {
    "Managed OAuth provider": "OpenAI 官方登录 / 托管 OAuth",
    "OpenAI-compatible provider": "OpenAI-compatible 模型源",
    "Native provider": "原生协议模型源",
    "Codex router route": "Codex 多模型路由规则",
  };
  return translations[normalized] ?? normalized;
}

/// 根据 profile 生成与后端 runtime option 一致的稳定 key。
export function profileBackendKey(profile?: {
  backendType?: "provider" | "codex_router_route";
  appType?: string | null;
  providerId?: string | null;
  routeId?: string | null;
}): string {
  if (!profile?.backendType || !profile.appType || !profile.providerId) return "";
  return [
    profile.backendType,
    profile.appType,
    profile.providerId,
    profile.routeId ?? "",
  ].join("::");
}

/// 将代理监听地址转换为第三方 agent 需要填写的 OpenAI v1 base_url。
export function buildBaseUrl(address?: string, port?: number): string {
  const host = address && address !== "0.0.0.0" ? address : "127.0.0.1";
  return `http://${host}:${port && port > 0 ? port : 15721}/v1`;
}

/// 生成面向局域网或公网客户端展示的 base_url 模板。
export function buildReachableBaseUrl(address?: string, port?: number): string {
  const resolvedPort = port && port > 0 ? port : 15721;
  if (!address || address === "127.0.0.1" || address === "localhost") {
    return `http://127.0.0.1:${resolvedPort}/v1`;
  }
  if (address === "0.0.0.0" || address === "::") {
    return `http://<你的IP或域名>:${resolvedPort}/v1`;
  }
  return `http://${address}:${resolvedPort}/v1`;
}

/// 生成 OpenAI Python SDK 的最小可运行配置片段。
export function buildPythonSnippet(
  baseUrl: string,
  apiKey: string,
  model: string,
): string {
  return [
    "from openai import OpenAI",
    "",
    "client = OpenAI(",
    `    base_url="${baseUrl}",`,
    `    api_key="${apiKey}",`,
    ")",
    "",
    "response = client.chat.completions.create(",
    `    model="${model}",`,
    '    messages=[{"role": "user", "content": "ping"}],',
    ")",
    "print(response.choices[0].message.content)",
  ].join("\n");
}

/// 生成常见 OpenAI-compatible agent 可直接参考的 JSON 配置。
export function buildJsonConfig(
  baseUrl: string,
  apiKey: string,
  model: string,
): string {
  return JSON.stringify(
    {
      provider: "openai-compatible",
      base_url: baseUrl,
      api_key: apiKey,
      model,
    },
    null,
    2,
  );
}

/// 选择默认服务来源时优先使用已保存 profile，其次使用可用的官方 OAuth/路由/普通 OpenAI-compatible 来源。
export function chooseDefaultBackendKey(
  options: ExternalOpenAIAPIBackendOption[],
  candidates: Array<string | undefined | null>,
): string {
  const candidate = candidates
    .filter(Boolean)
    .find((key) => options.some((option) => option.key === key));
  if (candidate) return candidate;

  const available = options.filter((option) => option.available);
  return (
    available.find((option) => option.isManagedOAuth)?.key ??
    available.find((option) => option.backendType === "codex_router_route")?.key ??
    available[0]?.key ??
    options[0]?.key ??
    ""
  );
}

/// 将 option 归类成 GUI 可扫描的服务来源分组；不可用来源保留展示，但不会参与默认选择。
export function groupBackendOptions(
  options: ExternalOpenAIAPIBackendOption[],
): BackendGroup[] {
  const groups: BackendGroup[] = [
    {
      key: "official",
      label: "OpenAI 官方登录",
      tone: "emerald",
      options: [],
    },
    {
      key: "router",
      label: "Codex 多模型路由",
      tone: "blue",
      options: [],
    },
    {
      key: "compatible",
      label: "OpenAI-compatible 模型源",
      tone: "amber",
      options: [],
    },
    {
      key: "unavailable",
      label: "暂不可接入",
      tone: "slate",
      options: [],
    },
  ];

  for (const option of options) {
    if (!option.available) {
      groups[3].options.push(option);
    } else if (option.isManagedOAuth) {
      groups[0].options.push(option);
    } else if (option.backendType === "codex_router_route") {
      groups[1].options.push(option);
    } else {
      groups[2].options.push(option);
    }
  }

  return groups
    .map((group) => ({
      ...group,
      options: group.options.sort((a, b) => a.label.localeCompare(b.label)),
    }))
    .filter((group) => group.options.length > 0);
}

/// 生成服务来源的协议、认证和隔离摘要，供不同 GUI 场景复用。
export function describeBackendTarget(
  option?: ExternalOpenAIAPIBackendOption | null,
): BackendTargetDescription {
  if (!option) {
    return {
      kind: "未选择服务来源",
      protocol: "尚未建立转发路径",
      auth: "尚未配置本地访问 Key",
      modelSource: "尚未选择模型",
      compatibility: ["未配置"],
    };
  }

  const isRoute = option.backendType === "codex_router_route";
  return {
    kind: isRoute ? "路由规则" : option.isManagedOAuth ? "OpenAI 官方登录" : "直连模型源",
    protocol: isRoute
      ? "先按 Codex 路由解析，再转成 OpenAI v1 响应"
      : option.isManagedOAuth
        ? "转接 ChatGPT Codex Responses 后端"
        : "按 OpenAI-compatible /chat/completions 转发",
    auth: option.isManagedOAuth
      ? "OAuth 留在 CC Switch 内部，不暴露给第三方 agent"
      : "使用该模型源保存的凭据",
    modelSource:
      option.models.length > 0
        ? `${option.models.length} 个可选模型`
        : "使用保存的默认模型",
    compatibility: [
      "/v1/chat/completions",
      isRoute || option.isManagedOAuth ? "/v1/responses" : "仅 Chat Completions",
      option.available ? "可接入" : "需要补配置",
    ],
  };
}

/// 生成 profile 保存请求；保存 profile 不启动代理，也不切换任何 app provider。
export function buildProfileUpdate(
  selectedBackend: ExternalOpenAIAPIBackendOption,
  defaultModel: string,
  enabled: boolean,
): ExternalOpenAIAPIProfileUpdate {
  return {
    enabled,
    backendType: selectedBackend.backendType,
    appType: selectedBackend.appType,
    providerId: selectedBackend.providerId,
    routeId: selectedBackend.routeId ?? null,
    defaultModel,
  };
}
