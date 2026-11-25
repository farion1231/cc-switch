// 供应商配置处理工具函数

import type { TemplateValueConfig } from "../config/claudeProviderPresets";

// 读取配置中的 API Key（支持 Claude, Codex, Gemini）
export const getApiKeyFromConfig = (
  jsonString: string,
  appType?: string,
): string => {
  try {
    const config = JSON.parse(jsonString);

    // Bedrock API Key：Claude Code 读的是 env.AWS_BEARER_TOKEN_BEDROCK
    const bedrockToken = config?.env?.AWS_BEARER_TOKEN_BEDROCK;
    if (typeof bedrockToken === "string" && bedrockToken) {
      return bedrockToken;
    }

    // 其次顶层 apiKey：旧版 Bedrock API Key 预设写在这里，存量行照常显示
    if (
      typeof config?.apiKey === "string" &&
      config.apiKey &&
      !config.apiKey.includes("${")
    ) {
      return config.apiKey;
    }

    if (appType === "kimi")
      return typeof config?.provider?.api_key === "string"
        ? config.provider.api_key
        : "";
    const env = config?.env;

    if (!env) return "";

    // Gemini API Key
    if (appType === "gemini") {
      const geminiKey = env.GEMINI_API_KEY;
      return typeof geminiKey === "string" ? geminiKey : "";
    }

    // Codex API Key
    if (appType === "codex") {
      const codexKey = env.CODEX_API_KEY;
      return typeof codexKey === "string" ? codexKey : "";
    }

    // Claude API Key (优先 ANTHROPIC_AUTH_TOKEN，其次 ANTHROPIC_API_KEY)
    const token = env.ANTHROPIC_AUTH_TOKEN;
    const apiKey = env.ANTHROPIC_API_KEY;
    const value =
      typeof token === "string"
        ? token
        : typeof apiKey === "string"
          ? apiKey
          : "";
    return value;
  } catch (err) {
    return "";
  }
};

// 模板变量替换
export const applyTemplateValues = (
  config: any,
  templateValues: Record<string, TemplateValueConfig> | undefined,
): any => {
  const resolvedValues = Object.fromEntries(
    Object.entries(templateValues ?? {}).map(([key, value]) => {
      const resolvedValue =
        value.editorValue !== undefined
          ? value.editorValue
          : (value.defaultValue ?? "");
      return [key, resolvedValue];
    }),
  );

  const replaceInString = (str: string): string => {
    return Object.entries(resolvedValues).reduce((acc, [key, value]) => {
      const placeholder = `\${${key}}`;
      if (!acc.includes(placeholder)) {
        return acc;
      }
      return acc.split(placeholder).join(value ?? "");
    }, str);
  };

  const traverse = (obj: any): any => {
    if (typeof obj === "string") {
      return replaceInString(obj);
    }
    if (Array.isArray(obj)) {
      return obj.map(traverse);
    }
    if (obj && typeof obj === "object") {
      const result: any = {};
      for (const [key, value] of Object.entries(obj)) {
        result[key] = traverse(value);
      }
      return result;
    }
    return obj;
  };

  return traverse(config);
};

// 判断配置中是否存在 API Key 字段
export const hasApiKeyField = (
  jsonString: string,
  appType?: string,
): boolean => {
  try {
    const config = JSON.parse(jsonString);

    // 顶层 apiKey：旧版 Bedrock API Key 预设
    if (Object.prototype.hasOwnProperty.call(config, "apiKey")) {
      return true;
    }

    const env = config?.env ?? {};

    if (Object.prototype.hasOwnProperty.call(env, "AWS_BEARER_TOKEN_BEDROCK")) {
      return true;
    }

    if (appType === "gemini") {
      return Object.prototype.hasOwnProperty.call(env, "GEMINI_API_KEY");
    }

    if (appType === "codex") {
      return Object.prototype.hasOwnProperty.call(env, "CODEX_API_KEY");
    }

    return (
      Object.prototype.hasOwnProperty.call(env, "ANTHROPIC_AUTH_TOKEN") ||
      Object.prototype.hasOwnProperty.call(env, "ANTHROPIC_API_KEY")
    );
  } catch (err) {
    return false;
  }
};

// 写入/更新配置中的 API Key，默认不新增缺失字段
export const setApiKeyInConfig = (
  jsonString: string,
  apiKey: string,
  options: {
    createIfMissing?: boolean;
    appType?: string;
    apiKeyField?: string;
  } = {},
): string => {
  const { createIfMissing = false, appType, apiKeyField } = options;
  try {
    const config = JSON.parse(jsonString);

    // Bedrock API Key：写回 env.AWS_BEARER_TOKEN_BEDROCK
    if (
      config?.env &&
      Object.prototype.hasOwnProperty.call(
        config.env,
        "AWS_BEARER_TOKEN_BEDROCK",
      )
    ) {
      config.env.AWS_BEARER_TOKEN_BEDROCK = apiKey;
      return JSON.stringify(config, null, 2);
    }

    // 顶层 apiKey：旧版 Bedrock API Key 预设的存量行，写回原处
    if (Object.prototype.hasOwnProperty.call(config, "apiKey")) {
      config.apiKey = apiKey;
      return JSON.stringify(config, null, 2);
    }

    if (!config.env) {
      if (!createIfMissing) return jsonString;
      config.env = {};
    }
    const env = config.env as Record<string, any>;

    // Gemini API Key
    if (appType === "gemini") {
      if ("GEMINI_API_KEY" in env) {
        env.GEMINI_API_KEY = apiKey;
      } else if (createIfMissing) {
        env.GEMINI_API_KEY = apiKey;
      } else {
        return jsonString;
      }
      return JSON.stringify(config, null, 2);
    }

    // Codex API Key
    if (appType === "codex") {
      if ("CODEX_API_KEY" in env) {
        env.CODEX_API_KEY = apiKey;
      } else if (createIfMissing) {
        env.CODEX_API_KEY = apiKey;
      } else {
        return jsonString;
      }
      return JSON.stringify(config, null, 2);
    }

    // Claude API Key (优先写入已存在的字段；若两者均不存在且允许创建，则使用 apiKeyField 或默认 AUTH_TOKEN 字段)
    if ("ANTHROPIC_AUTH_TOKEN" in env) {
      env.ANTHROPIC_AUTH_TOKEN = apiKey;
    } else if ("ANTHROPIC_API_KEY" in env) {
      env.ANTHROPIC_API_KEY = apiKey;
    } else if (createIfMissing) {
      env[apiKeyField ?? "ANTHROPIC_AUTH_TOKEN"] = apiKey;
    } else {
      return jsonString;
    }
    return JSON.stringify(config, null, 2);
  } catch (err) {
    return jsonString;
  }
};
