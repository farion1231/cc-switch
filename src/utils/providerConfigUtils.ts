// 供应商配置处理工具函数

import type { TemplateValueConfig } from "../config/claudeProviderPresets";
import { normalizeQuotes, normalizeTomlText } from "@/utils/textNormalization";
import { parse as parseToml, stringify as stringifyToml } from "smol-toml";

const isPlainObject = (value: unknown): value is Record<string, any> => {
  return Object.prototype.toString.call(value) === "[object Object]";
};

const deepMerge = (
  target: Record<string, any>,
  source: Record<string, any>,
): Record<string, any> => {
  Object.entries(source).forEach(([key, value]) => {
    if (isPlainObject(value)) {
      if (!isPlainObject(target[key])) {
        target[key] = {};
      }
      deepMerge(target[key], value);
    } else {
      // 直接覆盖非对象字段（数组/基础类型）
      target[key] = value;
    }
  });
  return target;
};

const deepRemove = (
  target: Record<string, any>,
  source: Record<string, any>,
) => {
  Object.entries(source).forEach(([key, value]) => {
    if (!(key in target)) return;

    if (isPlainObject(value) && isPlainObject(target[key])) {
      // 只移除完全匹配的嵌套属性
      deepRemove(target[key], value);
      if (Object.keys(target[key]).length === 0) {
        delete target[key];
      }
    } else if (isSubset(target[key], value)) {
      // 只有当值完全匹配时才删除
      delete target[key];
    }
  });
};

const isSubset = (target: any, source: any): boolean => {
  if (isPlainObject(source)) {
    if (!isPlainObject(target)) return false;
    return Object.entries(source).every(([key, value]) =>
      isSubset(target[key], value),
    );
  }

  if (Array.isArray(source)) {
    if (!Array.isArray(target) || target.length !== source.length) return false;
    return source.every((item, index) => isSubset(target[index], item));
  }

  return target === source;
};

// 深拷贝函数
const deepClone = <T>(obj: T): T => {
  if (obj === null || typeof obj !== "object") return obj;
  if (obj instanceof Date) return new Date(obj.getTime()) as T;
  if (obj instanceof Array) return obj.map((item) => deepClone(item)) as T;
  if (obj instanceof Object) {
    const clonedObj = {} as T;
    for (const key in obj) {
      if (obj.hasOwnProperty(key)) {
        clonedObj[key] = deepClone(obj[key]);
      }
    }
    return clonedObj;
  }
  return obj;
};

export interface UpdateCommonConfigResult {
  updatedConfig: string;
  error?: string;
}

// 验证JSON配置格式
export const validateJsonConfig = (
  value: string,
  fieldName: string = "配置",
): string => {
  if (!value.trim()) {
    return "";
  }
  try {
    const parsed = JSON.parse(value);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return `${fieldName}必须是 JSON 对象`;
    }
    return "";
  } catch {
    return `${fieldName}JSON格式错误，请检查语法`;
  }
};

// 将通用配置片段写入/移除 settingsConfig
export const updateCommonConfigSnippet = (
  jsonString: string,
  snippetString: string,
  enabled: boolean,
): UpdateCommonConfigResult => {
  let config: Record<string, any>;
  try {
    config = jsonString ? JSON.parse(jsonString) : {};
  } catch (err) {
    return {
      updatedConfig: jsonString,
      error: "配置 JSON 解析失败，无法写入通用配置",
    };
  }

  if (!snippetString.trim()) {
    return {
      updatedConfig: JSON.stringify(config, null, 2),
    };
  }

  // 使用统一的验证函数
  const snippetError = validateJsonConfig(snippetString, "通用配置片段");
  if (snippetError) {
    return {
      updatedConfig: JSON.stringify(config, null, 2),
      error: snippetError,
    };
  }

  const snippet = JSON.parse(snippetString) as Record<string, any>;

  if (enabled) {
    const merged = deepMerge(deepClone(config), snippet);
    return {
      updatedConfig: JSON.stringify(merged, null, 2),
    };
  }

  const cloned = deepClone(config);
  deepRemove(cloned, snippet);
  return {
    updatedConfig: JSON.stringify(cloned, null, 2),
  };
};

// 检查当前配置是否已包含通用配置片段
export const hasCommonConfigSnippet = (
  jsonString: string,
  snippetString: string,
): boolean => {
  try {
    if (!snippetString.trim()) return false;
    const config = jsonString ? JSON.parse(jsonString) : {};
    const snippet = JSON.parse(snippetString);
    if (!isPlainObject(snippet)) return false;
    return isSubset(config, snippet);
  } catch (err) {
    return false;
  }
};

// 读取配置中的 API Key（支持 Claude, Codex, Gemini）
export const getApiKeyFromConfig = (
  jsonString: string,
  appType?: string,
): string => {
  try {
    const config = JSON.parse(jsonString);

    // 优先检查顶层 apiKey 字段（用于 Bedrock API Key 等预设）
    if (
      typeof config?.apiKey === "string" &&
      config.apiKey &&
      !config.apiKey.includes("${")
    ) {
      return config.apiKey;
    }

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

    // 检查顶层 apiKey 字段（用于 Bedrock API Key 等预设）
    if (Object.prototype.hasOwnProperty.call(config, "apiKey")) {
      return true;
    }

    const env = config?.env ?? {};

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

    // 优先检查顶层 apiKey 字段（用于 Bedrock API Key 等预设）
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

// ========== TOML Config Utilities ==========

export interface UpdateTomlCommonConfigResult {
  updatedConfig: string;
  error?: string;
}

// Write/remove common config snippet to/from TOML config (structural merge)
export const updateTomlCommonConfigSnippet = (
  tomlString: string,
  snippetString: string,
  enabled: boolean,
): UpdateTomlCommonConfigResult => {
  if (!snippetString.trim()) {
    return { updatedConfig: tomlString };
  }

  try {
    const config = parseToml(normalizeTomlText(tomlString || ""));
    const snippet = parseToml(normalizeTomlText(snippetString));

    if (enabled) {
      const merged = deepMerge(
        deepClone(config) as Record<string, any>,
        deepClone(snippet) as Record<string, any>,
      );
      return { updatedConfig: stringifyToml(merged) };
    } else {
      const result = deepClone(config) as Record<string, any>;
      deepRemove(result, snippet as Record<string, any>);
      return { updatedConfig: stringifyToml(result) };
    }
  } catch (e) {
    return { updatedConfig: tomlString, error: String(e) };
  }
};

// Check if TOML config already contains the common config snippet (structural subset check)
export const hasTomlCommonConfigSnippet = (
  tomlString: string,
  snippetString: string,
): boolean => {
  if (!snippetString.trim()) return false;

  try {
    const config = parseToml(normalizeTomlText(tomlString || ""));
    const snippet = parseToml(normalizeTomlText(snippetString));
    return isSubset(config, snippet);
  } catch {
    // Fallback to text-based matching if TOML parsing fails
    const norm = (s: string) => s.replace(/\s+/g, " ").trim();
    return norm(tomlString).includes(norm(snippetString));
  }
};

// ========== Codex base_url utils ==========

// 从 Codex 的 TOML 配置文本中提取 base_url（支持单/双引号）
export const extractCodexBaseUrl = (
  configText: string | undefined | null,
): string | undefined => {
  try {
    const raw = typeof configText === "string" ? configText : "";
    // 归一化中文/全角引号，避免正则提取失败
    const text = normalizeQuotes(raw);
    if (!text) return undefined;
    const m = text.match(/base_url\s*=\s*(['"])([^'\"]+)\1/);
    return m && m[2] ? m[2] : undefined;
  } catch {
    return undefined;
  }
};

// 从 Provider 对象中提取 Codex base_url（当 settingsConfig.config 为 TOML 字符串时）
export const getCodexBaseUrl = (
  provider: { settingsConfig?: Record<string, any> } | undefined | null,
): string | undefined => {
  try {
    const text =
      typeof provider?.settingsConfig?.config === "string"
        ? (provider as any).settingsConfig.config
        : "";
    return extractCodexBaseUrl(text);
  } catch {
    return undefined;
  }
};

// 在 Codex 的 TOML 配置文本中写入或更新 base_url 字段
export const setCodexBaseUrl = (
  configText: string,
  baseUrl: string,
): string => {
  const trimmed = baseUrl.trim();
  // 归一化原文本中的引号（既能匹配，也能输出稳定格式）
  const normalizedText = normalizeQuotes(configText);

  // 允许清空：当 baseUrl 为空时，移除 base_url 行
  if (!trimmed) {
    if (!normalizedText) return normalizedText;
    const next = normalizedText
      .split("\n")
      .filter((line) => !/^\s*base_url\s*=/.test(line))
      .join("\n")
      // 避免移除后留下过多空行
      .replace(/\n{3,}/g, "\n\n")
      // 避免开头出现空行
      .replace(/^\n+/, "");
    return next;
  }

  const normalizedUrl = trimmed.replace(/\s+/g, "");
  const replacementLine = `base_url = "${normalizedUrl}"`;
  const pattern = /base_url\s*=\s*(["'])([^"']+)\1/;

  if (pattern.test(normalizedText)) {
    return normalizedText.replace(pattern, replacementLine);
  }

  const prefix =
    normalizedText && !normalizedText.endsWith("\n")
      ? `${normalizedText}\n`
      : normalizedText;
  return `${prefix}${replacementLine}\n`;
};

// ========== Codex model name utils ==========

// 从 Codex 的 TOML 配置文本中提取 model 字段（支持单/双引号）
export const extractCodexModelName = (
  configText: string | undefined | null,
): string | undefined => {
  try {
    const raw = typeof configText === "string" ? configText : "";
    // 归一化中文/全角引号，避免正则提取失败
    const text = normalizeQuotes(raw);
    if (!text) return undefined;

    // 匹配 model = "xxx" 或 model = 'xxx'
    const m = text.match(/^model\s*=\s*(['"])([^'"]+)\1/m);
    return m && m[2] ? m[2] : undefined;
  } catch {
    return undefined;
  }
};

// 在 Codex 的 TOML 配置文本中写入或更新 model 字段
export const setCodexModelName = (
  configText: string,
  modelName: string,
): string => {
  const trimmed = modelName.trim();
  // 归一化原文本中的引号（既能匹配，也能输出稳定格式）
  const normalizedText = normalizeQuotes(configText);

  // 允许清空：当 modelName 为空时，移除 model 行
  if (!trimmed) {
    if (!normalizedText) return normalizedText;
    const next = normalizedText
      .split("\n")
      .filter((line) => !/^\s*model\s*=/.test(line))
      .join("\n")
      .replace(/\n{3,}/g, "\n\n")
      .replace(/^\n+/, "");
    return next;
  }

  const replacementLine = `model = "${trimmed}"`;
  const pattern = /^model\s*=\s*["']([^"']+)["']/m;

  if (pattern.test(normalizedText)) {
    return normalizedText.replace(pattern, replacementLine);
  }

  // 如果不存在 model 字段，尝试在 model_provider 之后插入
  // 如果 model_provider 也不存在，则插入到开头
  const providerPattern = /^model_provider\s*=\s*["'][^"']+["']/m;
  const match = normalizedText.match(providerPattern);

  if (match && match.index !== undefined) {
    // 在 model_provider 行之后插入
    const endOfLine = normalizedText.indexOf("\n", match.index);
    if (endOfLine !== -1) {
      return (
        normalizedText.slice(0, endOfLine + 1) +
        replacementLine +
        "\n" +
        normalizedText.slice(endOfLine + 1)
      );
    }
  }

  // 在文件开头插入
  const lines = normalizedText.split("\n");
  return `${replacementLine}\n${lines.join("\n")}`;
};
