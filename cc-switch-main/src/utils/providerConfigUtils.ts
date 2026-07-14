// 供应商配置处理工具函数

import type { TemplateValueConfig } from "../config/claudeProviderPresets";
import type { CodexApiFormat } from "@/types";
import { deepClone } from "@/utils/deepClone";
import { normalizeTomlText } from "@/utils/textNormalization";
import { parse as parseToml } from "smol-toml";

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
      error: "配置 JSON 解析失败，无法应用通用配置",
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

// TOML 片段的合并/剥离必须走后端命令（configApi.updateTomlCommonConfigSnippet，
// toml_edit 保注释保键序）。禁止在前端用 smol-toml parse→merge→stringify
// 整文档重序列化：注释全丢、键序重排、还会生成多余的空父表头。

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

const TOML_SECTION_HEADER_PATTERN = /^\s*\[([^\]\r\n]+)\]\s*$/;
const TOML_BASE_URL_PATTERN =
  /^\s*base_url\s*=\s*(?:"((?:\\.|[^"\\\r\n])*)"|'([^'\r\n]*)')\s*(?:#.*)?$/;
const TOML_EXPERIMENTAL_BEARER_TOKEN_PATTERN =
  /^\s*experimental_bearer_token\s*=\s*(["'])([^"'\r\n]+)\1\s*(?:#.*)?$/;
const TOML_EXPERIMENTAL_BEARER_TOKEN_REPLACE_PATTERN =
  /^(\s*experimental_bearer_token\s*=\s*)(?:"(?:\\.|[^"\\\r\n])*"|'[^'\r\n]*')(\s*(?:#.*)?)$/;
// 双引号基本字符串（支持 \" \\ 等转义序列，识别 setCodexModelName 自己的
// 转义输出），提取后需反转义。刻意只做整行严格匹配、不做键名宽匹配——
// 宽匹配会误伤多行字符串里长得像赋值的文本；认不出的怪值由用户负责。
const TOML_MODEL_DOUBLE_QUOTED_PATTERN =
  /^\s*model\s*=\s*"((?:[^"\\\r\n]|\\.)*)"\s*(?:#.*)?$/;
// 单引号字面字符串（TOML 语义：无转义）
const TOML_MODEL_SINGLE_QUOTED_PATTERN =
  /^\s*model\s*=\s*'([^'\r\n]*)'\s*(?:#.*)?$/;
const TOML_WIRE_API_PATTERN =
  /^\s*wire_api\s*=\s*(["'])([^"'\r\n]+)\1\s*(?:#.*)?$/;
const TOML_MODEL_PROVIDER_LINE_PATTERN =
  /^\s*model_provider\s*=\s*(["'])([^"'\r\n]+)\1\s*(?:#.*)?$/;
const TOML_PROVIDER_NAME_PATTERN =
  /^\s*name\s*=\s*(["'])([^"'\r\n]+)\1\s*(?:#.*)?$/;
const TOML_PROVIDER_NAME_REPLACE_PATTERN =
  /^(\s*name\s*=\s*)(?:"(?:\\.|[^"\\\r\n])*"|'[^'\r\n]*')(\s*(?:#.*)?)$/;
const TOML_GOALS_FEATURE_PATTERN = /^\s*goals\s*=\s*(true|false)\s*(?:#.*)?$/;
const TOML_GOALS_FEATURE_REPLACE_PATTERN =
  /^(\s*goals\s*=\s*)(true|false)(\s*(?:#.*)?)$/;
const CODEX_RESERVED_MODEL_PROVIDER_IDS = new Set([
  "amazon-bedrock",
  "openai",
  "ollama",
  "lmstudio",
  "oss",
  "ollama-chat",
]);

interface TomlSectionRange {
  bodyEndIndex: number;
  bodyStartIndex: number;
  headerLineIndex: number;
}

interface TomlAssignmentMatch {
  index: number;
  sectionName?: string;
  value: string;
}

const finalizeTomlText = (lines: string[]): string =>
  lines
    .join("\n")
    .replace(/\n{3,}/g, "\n\n")
    .replace(/^\n+/, "");

const getTomlSectionRange = (
  lines: string[],
  sectionName: string,
): TomlSectionRange | undefined => {
  let headerLineIndex = -1;

  for (let index = 0; index < lines.length; index += 1) {
    const match = lines[index].match(TOML_SECTION_HEADER_PATTERN);
    if (!match) {
      continue;
    }

    if (headerLineIndex === -1) {
      if (match[1] === sectionName) {
        headerLineIndex = index;
      }
      continue;
    }

    return {
      bodyStartIndex: headerLineIndex + 1,
      bodyEndIndex: index,
      headerLineIndex,
    };
  }

  if (headerLineIndex === -1) {
    return undefined;
  }

  return {
    bodyStartIndex: headerLineIndex + 1,
    bodyEndIndex: lines.length,
    headerLineIndex,
  };
};

const getTopLevelEndIndex = (lines: string[]): number => {
  const firstSectionIndex = lines.findIndex((line) =>
    TOML_SECTION_HEADER_PATTERN.test(line),
  );
  return firstSectionIndex === -1 ? lines.length : firstSectionIndex;
};

const getTomlSectionInsertIndex = (
  lines: string[],
  sectionRange: TomlSectionRange,
): number => {
  let insertIndex = sectionRange.bodyEndIndex;
  while (
    insertIndex > sectionRange.bodyStartIndex &&
    lines[insertIndex - 1].trim() === ""
  ) {
    insertIndex -= 1;
  }
  return insertIndex;
};

const getCodexModelProviderName = (configText: string): string | undefined => {
  const normalized = normalizeTomlText(configText);
  try {
    const parsed = parseToml(normalized) as Record<string, any>;
    const providerName =
      typeof parsed.model_provider === "string"
        ? parsed.model_provider.trim()
        : undefined;
    if (providerName) return providerName;
  } catch {
    // Fall back to a top-level line scan while the user is editing invalid TOML.
  }

  const lines = normalized.split("\n");
  const index = getTopLevelModelProviderLineIndex(lines);
  if (index === -1) return undefined;
  const match = lines[index].match(TOML_MODEL_PROVIDER_LINE_PATTERN);
  const providerName = match?.[2]?.trim();
  return providerName || undefined;
};

const getCodexProviderSectionName = (
  configText: string,
): string | undefined => {
  const providerName = getCodexModelProviderName(configText);
  return providerName ? `model_providers.${providerName}` : undefined;
};

const isCustomCodexModelProviderId = (providerName: string): boolean => {
  const id = providerName.trim().toLowerCase();
  return Boolean(id) && !CODEX_RESERVED_MODEL_PROVIDER_IDS.has(id);
};

const getCodexCustomProviderSectionName = (
  configText: string,
): string | undefined => {
  const providerName = getCodexModelProviderName(configText);
  return providerName && isCustomCodexModelProviderId(providerName)
    ? `model_providers.${providerName}`
    : undefined;
};

const findTomlAssignmentInRange = (
  lines: string[],
  pattern: RegExp,
  startIndex: number,
  endIndex: number,
  sectionName?: string,
): TomlAssignmentMatch | undefined => {
  for (let index = startIndex; index < endIndex; index += 1) {
    const match = lines[index].match(pattern);
    const value = match?.[2] ?? match?.[1];
    if (value) {
      return {
        index,
        sectionName,
        value,
      };
    }
  }

  return undefined;
};

const findTomlAssignmentsInRange = (
  lines: string[],
  pattern: RegExp,
  startIndex: number,
  endIndex: number,
  sectionName?: string,
): TomlAssignmentMatch[] => {
  const matches: TomlAssignmentMatch[] = [];

  for (let index = startIndex; index < endIndex; index += 1) {
    const match = lines[index].match(pattern);
    const value = match?.[2] ?? match?.[1];
    if (value) {
      matches.push({ index, sectionName, value });
    }
  }

  return matches;
};

const findTomlLineInRange = (
  lines: string[],
  pattern: RegExp,
  startIndex: number,
  endIndex: number,
): number => {
  for (let index = startIndex; index < endIndex; index += 1) {
    if (pattern.test(lines[index])) {
      return index;
    }
  }

  return -1;
};

const findTomlAssignments = (
  lines: string[],
  pattern: RegExp,
): TomlAssignmentMatch[] => {
  const assignments: TomlAssignmentMatch[] = [];
  let currentSectionName: string | undefined;

  lines.forEach((line, index) => {
    const sectionMatch = line.match(TOML_SECTION_HEADER_PATTERN);
    if (sectionMatch) {
      currentSectionName = sectionMatch[1];
      return;
    }

    const match = line.match(pattern);
    const value = match?.[2] ?? match?.[1];
    if (!value) {
      return;
    }

    assignments.push({
      index,
      sectionName: currentSectionName,
      value,
    });
  });

  return assignments;
};

const isMcpServerSection = (sectionName?: string): boolean =>
  sectionName === "mcp_servers" ||
  sectionName?.startsWith("mcp_servers.") === true;

const isOtherProviderSection = (
  sectionName: string | undefined,
  targetSectionName: string | undefined,
): boolean =>
  Boolean(
    sectionName &&
      sectionName !== targetSectionName &&
      (sectionName === "model_providers" ||
        sectionName.startsWith("model_providers.")),
  );

const getRecoverableBaseUrlAssignments = (
  assignments: TomlAssignmentMatch[],
  targetSectionName: string | undefined,
): TomlAssignmentMatch[] =>
  assignments.filter(
    ({ sectionName }) =>
      sectionName !== targetSectionName &&
      !isMcpServerSection(sectionName) &&
      !isOtherProviderSection(sectionName, targetSectionName),
  );

const getRecoverableCodexProviderAssignments = getRecoverableBaseUrlAssignments;

const getTopLevelModelProviderLineIndex = (lines: string[]): number => {
  const topLevelEndIndex = getTopLevelEndIndex(lines);

  for (let index = 0; index < topLevelEndIndex; index += 1) {
    if (TOML_MODEL_PROVIDER_LINE_PATTERN.test(lines[index])) {
      return index;
    }
  }

  return -1;
};

const hasTomlSectionBodyContent = (
  lines: string[],
  sectionRange: TomlSectionRange,
): boolean =>
  lines
    .slice(sectionRange.bodyStartIndex, sectionRange.bodyEndIndex)
    .some((line) => line.trim() !== "");

const TOML_BASIC_STRING_ESCAPES: Record<string, string> = {
  '"': '\\"',
  "\\": "\\\\",
  "\b": "\\b",
  "\t": "\\t",
  "\n": "\\n",
  "\f": "\\f",
  "\r": "\\r",
};

const escapeTomlBasicString = (value: string): string =>
  value.replace(/["\\\u0000-\u001f]/g, (ch) => {
    const escaped = TOML_BASIC_STRING_ESCAPES[ch];
    if (escaped) return escaped;
    return `\\u${ch.charCodeAt(0).toString(16).padStart(4, "0")}`;
  });

const tomlBasicString = (value: string): string =>
  `"${escapeTomlBasicString(value)}"`;

const TOML_BASIC_STRING_UNESCAPES: Record<string, string> = {
  '"': '"',
  "\\": "\\",
  b: "\b",
  t: "\t",
  n: "\n",
  f: "\f",
  r: "\r",
};

// escapeTomlBasicString 的逆运算；未知转义序列原样保留
const unescapeTomlBasicString = (value: string): string =>
  value.replace(
    /\\(?:u([0-9a-fA-F]{4})|U([0-9a-fA-F]{8})|(.))/g,
    (match, u4, u8, ch) => {
      if (u4 || u8) return String.fromCodePoint(parseInt(u4 || u8, 16));
      return TOML_BASIC_STRING_UNESCAPES[ch] ?? match;
    },
  );

const CODEX_CHAT_WIRE_API_VALUES = new Set([
  "chat",
  "chat_completions",
  "chat-completions",
  "openai_chat",
  "openai-chat",
  "openai_chat_completions",
]);

// 判断给定的 wire_api 字符串是否表示 Codex 的 Chat Completions 协议
export const isCodexChatWireApi = (
  wireApi: string | undefined | null,
): boolean =>
  CODEX_CHAT_WIRE_API_VALUES.has((wireApi ?? "").trim().toLowerCase());

export const isCodexAnthropicWireApi = (
  wireApi: string | undefined | null,
): boolean =>
  [
    "anthropic",
    "anthropic_messages",
    "anthropic-messages",
    "messages",
    "claude",
  ].includes((wireApi ?? "").trim().toLowerCase());

export const codexApiFormatFromWireApi = (
  wireApi: string | undefined | null,
): CodexApiFormat | undefined => {
  if (isCodexChatWireApi(wireApi)) return "openai_chat";
  if (isCodexAnthropicWireApi(wireApi)) return "anthropic";
  switch ((wireApi ?? "").trim().toLowerCase()) {
    case "responses":
    case "openai_responses":
    case "openai-responses":
      return "openai_responses";
    default:
      return undefined;
  }
};

// 从 Codex 的 TOML 配置文本中提取 wire_api（支持单/双引号）
export const extractCodexWireApi = (
  configText: string | undefined | null,
): string | undefined => {
  try {
    const raw = typeof configText === "string" ? configText : "";
    const text = normalizeTomlText(raw);
    if (!text) return undefined;

    const lines = text.split("\n");
    const targetSectionName = getCodexProviderSectionName(text);

    if (targetSectionName) {
      const sectionRange = getTomlSectionRange(lines, targetSectionName);
      if (sectionRange) {
        const match = findTomlAssignmentInRange(
          lines,
          TOML_WIRE_API_PATTERN,
          sectionRange.bodyStartIndex,
          sectionRange.bodyEndIndex,
          targetSectionName,
        );
        if (match?.value) {
          return match.value;
        }
      }
    }

    const topLevelMatch = findTomlAssignmentInRange(
      lines,
      TOML_WIRE_API_PATTERN,
      0,
      getTopLevelEndIndex(lines),
    );
    if (topLevelMatch?.value) {
      return topLevelMatch.value;
    }

    const fallbackAssignments = getRecoverableCodexProviderAssignments(
      findTomlAssignments(lines, TOML_WIRE_API_PATTERN),
      targetSectionName,
    );
    return fallbackAssignments.length === 1
      ? fallbackAssignments[0].value
      : undefined;
  } catch {
    return undefined;
  }
};

// 在 Codex 的 TOML 配置文本中写入或更新 wire_api 字段
export const setCodexWireApi = (
  configText: string,
  wireApi: "responses" | "chat",
): string => {
  const normalizedText = normalizeTomlText(configText);
  const lines = normalizedText ? normalizedText.split("\n") : [];
  const targetSectionName = getCodexProviderSectionName(normalizedText);
  const replacementLine = `wire_api = "${wireApi}"`;
  const allAssignments = findTomlAssignments(lines, TOML_WIRE_API_PATTERN);
  const recoverableAssignments = getRecoverableCodexProviderAssignments(
    allAssignments,
    targetSectionName,
  );

  if (targetSectionName) {
    let targetSectionRange = getTomlSectionRange(lines, targetSectionName);
    const targetMatch = targetSectionRange
      ? findTomlAssignmentInRange(
          lines,
          TOML_WIRE_API_PATTERN,
          targetSectionRange.bodyStartIndex,
          targetSectionRange.bodyEndIndex,
          targetSectionName,
        )
      : undefined;

    if (targetMatch) {
      lines[targetMatch.index] = replacementLine;
      return finalizeTomlText(lines);
    }

    if (recoverableAssignments.length === 1) {
      lines.splice(recoverableAssignments[0].index, 1);
      targetSectionRange = getTomlSectionRange(lines, targetSectionName);
    }

    if (targetSectionRange) {
      const insertIndex = getTomlSectionInsertIndex(lines, targetSectionRange);
      lines.splice(insertIndex, 0, replacementLine);
      return finalizeTomlText(lines);
    }

    if (lines.length > 0 && lines[lines.length - 1].trim() !== "") {
      lines.push("");
    }
    lines.push(`[${targetSectionName}]`, replacementLine);
    return finalizeTomlText(lines);
  }

  const topLevelEndIndex = getTopLevelEndIndex(lines);
  const topLevelMatch = findTomlAssignmentInRange(
    lines,
    TOML_WIRE_API_PATTERN,
    0,
    topLevelEndIndex,
  );
  if (topLevelMatch) {
    lines[topLevelMatch.index] = replacementLine;
    return finalizeTomlText(lines);
  }

  const modelProviderIndex = getTopLevelModelProviderLineIndex(lines);
  if (modelProviderIndex !== -1) {
    lines.splice(modelProviderIndex + 1, 0, replacementLine);
    return finalizeTomlText(lines);
  }

  if (lines.length === 0) {
    return `${replacementLine}\n`;
  }

  lines.splice(topLevelEndIndex, 0, replacementLine);
  return finalizeTomlText(lines);
};

export const isCodexGoalModeEnabled = (
  configText: string | undefined | null,
): boolean => {
  try {
    const raw = typeof configText === "string" ? configText : "";
    const text = normalizeTomlText(raw);
    if (!text) return false;

    try {
      const parsed = parseToml(text) as Record<string, any>;
      return parsed.features?.goals === true;
    } catch {
      // Fall back to line scanning while the user is editing invalid TOML.
    }

    const lines = text.split("\n");
    const featureRange = getTomlSectionRange(lines, "features");
    if (!featureRange) return false;

    const index = findTomlLineInRange(
      lines,
      TOML_GOALS_FEATURE_PATTERN,
      featureRange.bodyStartIndex,
      featureRange.bodyEndIndex,
    );
    if (index === -1) return false;

    return lines[index].match(TOML_GOALS_FEATURE_PATTERN)?.[1] === "true";
  } catch {
    return false;
  }
};

export const setCodexGoalMode = (
  configText: string,
  enabled: boolean,
): string => {
  const normalizedText = normalizeTomlText(configText);
  const lines = normalizedText ? normalizedText.split("\n") : [];
  let featureRange = getTomlSectionRange(lines, "features");

  if (featureRange) {
    const goalLineIndex = findTomlLineInRange(
      lines,
      TOML_GOALS_FEATURE_REPLACE_PATTERN,
      featureRange.bodyStartIndex,
      featureRange.bodyEndIndex,
    );

    if (enabled) {
      if (goalLineIndex !== -1) {
        lines[goalLineIndex] = lines[goalLineIndex].replace(
          TOML_GOALS_FEATURE_REPLACE_PATTERN,
          "$1true$3",
        );
      } else {
        lines.splice(
          getTomlSectionInsertIndex(lines, featureRange),
          0,
          "goals = true",
        );
      }
      return finalizeTomlText(lines);
    }

    if (goalLineIndex !== -1) {
      lines.splice(goalLineIndex, 1);
      featureRange = getTomlSectionRange(lines, "features");
      if (featureRange && !hasTomlSectionBodyContent(lines, featureRange)) {
        lines.splice(
          featureRange.headerLineIndex,
          featureRange.bodyEndIndex - featureRange.headerLineIndex,
        );
      }
    }
    return finalizeTomlText(lines);
  }

  if (!enabled) return normalizedText;

  const topLevelEndIndex = getTopLevelEndIndex(lines);
  const sectionLines: string[] = [];
  if (topLevelEndIndex > 0 && lines[topLevelEndIndex - 1].trim() !== "") {
    sectionLines.push("");
  }
  sectionLines.push("[features]", "goals = true");
  if (
    topLevelEndIndex < lines.length &&
    lines[topLevelEndIndex]?.trim() !== ""
  ) {
    sectionLines.push("");
  }

  lines.splice(topLevelEndIndex, 0, ...sectionLines);
  return finalizeTomlText(lines);
};

export const isCodexRemoteCompactionEnabled = (
  configText: string | undefined | null,
): boolean => {
  try {
    const raw = typeof configText === "string" ? configText : "";
    const text = normalizeTomlText(raw);
    if (!text) return false;

    try {
      const parsed = parseToml(text) as Record<string, any>;
      const providerId =
        typeof parsed.model_provider === "string"
          ? parsed.model_provider.trim()
          : "";
      if (!providerId || !isCustomCodexModelProviderId(providerId)) {
        return false;
      }

      return parsed.model_providers?.[providerId]?.name === "OpenAI";
    } catch {
      // Fall back to line scanning while the user is editing invalid TOML.
    }

    const lines = text.split("\n");
    const targetSectionName = getCodexCustomProviderSectionName(text);
    if (!targetSectionName) return false;

    const sectionRange = getTomlSectionRange(lines, targetSectionName);
    if (!sectionRange) return false;

    const nameAssignment = findTomlAssignmentInRange(
      lines,
      TOML_PROVIDER_NAME_PATTERN,
      sectionRange.bodyStartIndex,
      sectionRange.bodyEndIndex,
      targetSectionName,
    );

    return nameAssignment?.value === "OpenAI";
  } catch {
    return false;
  }
};

export const setCodexRemoteCompaction = (
  configText: string,
  enabled: boolean,
  fallbackProviderName?: string,
): string => {
  const normalizedText = normalizeTomlText(configText);
  const lines = normalizedText ? normalizedText.split("\n") : [];
  const targetSectionName = getCodexCustomProviderSectionName(normalizedText);

  if (!targetSectionName) {
    return normalizedText;
  }

  let targetSectionRange = getTomlSectionRange(lines, targetSectionName);
  const replacementName = enabled
    ? "OpenAI"
    : fallbackProviderName?.trim() ||
      getCodexModelProviderName(normalizedText) ||
      "custom";
  const replacementLine = `name = ${tomlBasicString(replacementName)}`;

  if (targetSectionRange) {
    const nameLine = findTomlLineInRange(
      lines,
      TOML_PROVIDER_NAME_REPLACE_PATTERN,
      targetSectionRange.bodyStartIndex,
      targetSectionRange.bodyEndIndex,
    );

    if (nameLine !== -1) {
      lines[nameLine] = lines[nameLine].replace(
        TOML_PROVIDER_NAME_REPLACE_PATTERN,
        `$1${tomlBasicString(replacementName)}$2`,
      );
      return finalizeTomlText(lines);
    }

    lines.splice(
      getTomlSectionInsertIndex(lines, targetSectionRange),
      0,
      replacementLine,
    );
    return finalizeTomlText(lines);
  }

  if (!enabled) return normalizedText;

  if (lines.length > 0 && lines[lines.length - 1].trim() !== "") {
    lines.push("");
  }

  lines.push(`[${targetSectionName}]`, replacementLine);
  targetSectionRange = getTomlSectionRange(lines, targetSectionName);
  return targetSectionRange ? finalizeTomlText(lines) : normalizedText;
};

// 从 Codex 的 TOML 配置文本中提取 base_url（支持单/双引号）
export const extractCodexBaseUrl = (
  configText: string | undefined | null,
): string | undefined => {
  try {
    const raw = typeof configText === "string" ? configText : "";
    const text = normalizeTomlText(raw);
    if (!text) return undefined;

    const lines = text.split("\n");
    const targetSectionName = getCodexProviderSectionName(text);

    if (targetSectionName) {
      const sectionRange = getTomlSectionRange(lines, targetSectionName);
      if (sectionRange) {
        const match = findTomlAssignmentInRange(
          lines,
          TOML_BASE_URL_PATTERN,
          sectionRange.bodyStartIndex,
          sectionRange.bodyEndIndex,
          targetSectionName,
        );
        if (match?.value) {
          return match.value;
        }
      }
    }

    const topLevelMatch = findTomlAssignmentInRange(
      lines,
      TOML_BASE_URL_PATTERN,
      0,
      getTopLevelEndIndex(lines),
    );
    if (topLevelMatch?.value) {
      return topLevelMatch.value;
    }

    const fallbackAssignments = getRecoverableBaseUrlAssignments(
      findTomlAssignments(lines, TOML_BASE_URL_PATTERN),
      targetSectionName,
    );
    return fallbackAssignments.length === 1
      ? fallbackAssignments[0].value
      : undefined;
  } catch {
    return undefined;
  }
};

// 从 Codex 的 TOML 配置文本中提取 experimental_bearer_token（兼容 Mobile 模式）
export const extractCodexExperimentalBearerToken = (
  configText: string | undefined | null,
): string | undefined => {
  try {
    const raw = typeof configText === "string" ? configText : "";
    const text = normalizeTomlText(raw);
    if (!text) return undefined;

    try {
      const parsed = parseToml(text) as Record<string, any>;
      const providerName =
        typeof parsed.model_provider === "string"
          ? parsed.model_provider.trim()
          : undefined;
      const providerToken =
        providerName &&
        isCustomCodexModelProviderId(providerName) &&
        parsed.model_providers &&
        typeof parsed.model_providers === "object" &&
        typeof parsed.model_providers[providerName]
          ?.experimental_bearer_token === "string"
          ? parsed.model_providers[
              providerName
            ].experimental_bearer_token.trim()
          : undefined;
      if (providerToken) return providerToken;
      const topLevelToken =
        typeof parsed.experimental_bearer_token === "string"
          ? parsed.experimental_bearer_token.trim()
          : undefined;
      if (topLevelToken) return topLevelToken;
    } catch {
      // Fall back to the line scanner for partially edited TOML.
    }

    const lines = text.split("\n");
    const targetSectionName = getCodexCustomProviderSectionName(text);

    if (targetSectionName) {
      const sectionRange = getTomlSectionRange(lines, targetSectionName);
      if (sectionRange) {
        const match = findTomlAssignmentInRange(
          lines,
          TOML_EXPERIMENTAL_BEARER_TOKEN_PATTERN,
          sectionRange.bodyStartIndex,
          sectionRange.bodyEndIndex,
          targetSectionName,
        );
        if (match?.value) {
          return match.value;
        }
      }
    }

    const topLevelMatch = findTomlAssignmentInRange(
      lines,
      TOML_EXPERIMENTAL_BEARER_TOKEN_PATTERN,
      0,
      getTopLevelEndIndex(lines),
    );
    return topLevelMatch?.value;
  } catch {
    return undefined;
  }
};

// 同步更新 Codex config.toml 中已有的 experimental_bearer_token
// 仅修改已存在的条目, 不主动新增——避免破坏未使用 Mobile 兼容模式的普通 third-party 配置
// token 为空时删除该行 (让用户能真正清空 API key, 而不是被 pickCodexApiKey 的 fallback 又填回去)
export const updateCodexExperimentalBearerToken = (
  configText: string,
  token: string,
): string => {
  const normalizedText = normalizeTomlText(configText);
  if (
    !normalizedText ||
    !normalizedText.includes("experimental_bearer_token")
  ) {
    return configText;
  }

  const lines = normalizedText.split("\n");
  const targetSectionName = getCodexCustomProviderSectionName(normalizedText);

  let tokenLineIndex = -1;
  if (targetSectionName) {
    const sectionRange = getTomlSectionRange(lines, targetSectionName);
    if (sectionRange) {
      const index = findTomlLineInRange(
        lines,
        TOML_EXPERIMENTAL_BEARER_TOKEN_REPLACE_PATTERN,
        sectionRange.bodyStartIndex,
        sectionRange.bodyEndIndex,
      );
      if (index !== -1) tokenLineIndex = index;
    }
  }
  if (tokenLineIndex === -1) {
    const topLevelIndex = findTomlLineInRange(
      lines,
      TOML_EXPERIMENTAL_BEARER_TOKEN_REPLACE_PATTERN,
      0,
      getTopLevelEndIndex(lines),
    );
    if (topLevelIndex !== -1) tokenLineIndex = topLevelIndex;
  }

  if (tokenLineIndex === -1) return configText;

  const trimmed = token.trim();
  if (!trimmed) {
    lines.splice(tokenLineIndex, 1);
  } else {
    const escaped = escapeTomlBasicString(trimmed);
    const existingLine = lines[tokenLineIndex];
    lines[tokenLineIndex] = TOML_EXPERIMENTAL_BEARER_TOKEN_REPLACE_PATTERN.test(
      existingLine,
    )
      ? existingLine.replace(
          TOML_EXPERIMENTAL_BEARER_TOKEN_REPLACE_PATTERN,
          `$1"${escaped}"$2`,
        )
      : `experimental_bearer_token = "${escaped}"`;
  }
  return finalizeTomlText(lines);
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
  const normalizedText = normalizeTomlText(configText);
  const lines = normalizedText ? normalizedText.split("\n") : [];
  const targetSectionName = getCodexProviderSectionName(normalizedText);
  const allAssignments = findTomlAssignments(lines, TOML_BASE_URL_PATTERN);
  const recoverableAssignments = getRecoverableBaseUrlAssignments(
    allAssignments,
    targetSectionName,
  );

  if (!trimmed) {
    if (!normalizedText) return normalizedText;

    if (targetSectionName) {
      const sectionRange = getTomlSectionRange(lines, targetSectionName);
      const targetMatches = sectionRange
        ? findTomlAssignmentsInRange(
            lines,
            TOML_BASE_URL_PATTERN,
            sectionRange.bodyStartIndex,
            sectionRange.bodyEndIndex,
            targetSectionName,
          )
        : [];

      if (targetMatches.length > 0) {
        for (const match of [...targetMatches].reverse()) {
          lines.splice(match.index, 1);
        }
        return finalizeTomlText(lines);
      }
    }

    if (recoverableAssignments.length === 1) {
      lines.splice(recoverableAssignments[0].index, 1);
      return finalizeTomlText(lines);
    }

    return finalizeTomlText(lines);
  }

  const normalizedUrl = trimmed.replace(/\s+/g, "");
  const replacementLine = `base_url = ${tomlBasicString(normalizedUrl)}`;

  if (targetSectionName) {
    let targetSectionRange = getTomlSectionRange(lines, targetSectionName);
    const targetMatches = targetSectionRange
      ? findTomlAssignmentsInRange(
          lines,
          TOML_BASE_URL_PATTERN,
          targetSectionRange.bodyStartIndex,
          targetSectionRange.bodyEndIndex,
          targetSectionName,
        )
      : [];

    if (targetMatches.length > 0) {
      const [firstMatch, ...duplicateMatches] = targetMatches;
      lines[firstMatch.index] = replacementLine;
      for (const match of [...duplicateMatches].reverse()) {
        lines.splice(match.index, 1);
      }
      return finalizeTomlText(lines);
    }

    if (recoverableAssignments.length === 1) {
      lines.splice(recoverableAssignments[0].index, 1);
      targetSectionRange = getTomlSectionRange(lines, targetSectionName);
    }

    if (targetSectionRange) {
      const insertIndex = getTomlSectionInsertIndex(lines, targetSectionRange);
      lines.splice(insertIndex, 0, replacementLine);
      return finalizeTomlText(lines);
    }

    if (lines.length > 0 && lines[lines.length - 1].trim() !== "") {
      lines.push("");
    }
    lines.push(`[${targetSectionName}]`, replacementLine);
    return finalizeTomlText(lines);
  }

  const topLevelEndIndex = getTopLevelEndIndex(lines);
  const topLevelMatches = findTomlAssignmentsInRange(
    lines,
    TOML_BASE_URL_PATTERN,
    0,
    topLevelEndIndex,
  );
  if (topLevelMatches.length > 0) {
    const [firstMatch, ...duplicateMatches] = topLevelMatches;
    lines[firstMatch.index] = replacementLine;
    for (const match of [...duplicateMatches].reverse()) {
      lines.splice(match.index, 1);
    }
    return finalizeTomlText(lines);
  }

  const modelProviderIndex = getTopLevelModelProviderLineIndex(lines);
  if (modelProviderIndex !== -1) {
    lines.splice(modelProviderIndex + 1, 0, replacementLine);
    return finalizeTomlText(lines);
  }

  if (lines.length === 0) {
    return `${replacementLine}\n`;
  }

  const insertIndex = topLevelEndIndex;
  lines.splice(insertIndex, 0, replacementLine);
  return finalizeTomlText(lines);
};

// ========== Codex model name utils ==========

// 顶层范围内第一个能被严格模式识别的 model 行；-1 表示没有
const findTopLevelModelLineIndex = (
  lines: string[],
  topLevelEndIndex: number,
): number => {
  for (let i = 0; i < topLevelEndIndex; i += 1) {
    if (
      TOML_MODEL_DOUBLE_QUOTED_PATTERN.test(lines[i]) ||
      TOML_MODEL_SINGLE_QUOTED_PATTERN.test(lines[i])
    ) {
      return i;
    }
  }
  return -1;
};

// 从 Codex 的 TOML 配置文本中提取 model 字段（支持单/双引号；
// 双引号串按 TOML 基本字符串反转义，保证与 setCodexModelName round-trip）
export const extractCodexModelName = (
  configText: string | undefined | null,
): string | undefined => {
  try {
    const raw = typeof configText === "string" ? configText : "";
    const text = normalizeTomlText(raw);
    if (!text) return undefined;
    const lines = text.split("\n");
    const topLevelEndIndex = getTopLevelEndIndex(lines);
    for (let i = 0; i < topLevelEndIndex; i += 1) {
      const doubleQuoted = lines[i].match(TOML_MODEL_DOUBLE_QUOTED_PATTERN);
      if (doubleQuoted) return unescapeTomlBasicString(doubleQuoted[1]);
      const singleQuoted = lines[i].match(TOML_MODEL_SINGLE_QUOTED_PATTERN);
      if (singleQuoted) return singleQuoted[1];
    }
    return undefined;
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
  const normalizedText = normalizeTomlText(configText);
  const lines = normalizedText ? normalizedText.split("\n") : [];
  const topLevelEndIndex = getTopLevelEndIndex(lines);
  const modelLineIndex = findTopLevelModelLineIndex(lines, topLevelEndIndex);

  if (!trimmed) {
    if (!normalizedText) return normalizedText;
    if (modelLineIndex !== -1) {
      lines.splice(modelLineIndex, 1);
    }
    return finalizeTomlText(lines);
  }

  // 模型名可能来自远端 /models 响应（下拉选择），必须转义——裸插值会让
  // 含引号/控制字符的 id 破坏甚至注入 config.toml（如伪造 [mcp_servers.*]）
  const replacementLine = `model = ${tomlBasicString(trimmed)}`;
  if (modelLineIndex !== -1) {
    lines[modelLineIndex] = replacementLine;
    return finalizeTomlText(lines);
  }

  const modelProviderIndex = getTopLevelModelProviderLineIndex(lines);
  if (modelProviderIndex !== -1) {
    lines.splice(modelProviderIndex + 1, 0, replacementLine);
    return finalizeTomlText(lines);
  }

  if (lines.length === 0) {
    return `${replacementLine}\n`;
  }

  lines.splice(topLevelEndIndex, 0, replacementLine);
  return finalizeTomlText(lines);
};

// ========== Codex top-level integer field utils ==========

const tomlTopLevelIntPattern = (field: string) =>
  new RegExp(`^\\s*${field}\\s*=\\s*(\\d+)\\s*(?:#.*)?$`);

const findTopLevelIntMatch = (
  lines: string[],
  fieldName: string,
  topLevelEndIndex: number,
): { index: number; value: number } | undefined => {
  const pattern = tomlTopLevelIntPattern(fieldName);
  for (let i = 0; i < topLevelEndIndex; i += 1) {
    const m = lines[i].match(pattern);
    if (m) {
      return { index: i, value: Number(m[1]) };
    }
  }
  return undefined;
};

// 从 Codex TOML 配置中提取顶级整数字段
export const extractCodexTopLevelInt = (
  configText: string | undefined | null,
  fieldName: string,
): number | undefined => {
  try {
    const raw = typeof configText === "string" ? configText : "";
    const text = normalizeTomlText(raw);
    if (!text) return undefined;
    const lines = text.split("\n");
    return findTopLevelIntMatch(lines, fieldName, getTopLevelEndIndex(lines))
      ?.value;
  } catch {
    return undefined;
  }
};

// 在 Codex TOML 配置中设置或更新顶级整数字段
export const setCodexTopLevelInt = (
  configText: string,
  fieldName: string,
  value: number,
): string => {
  const normalizedText = normalizeTomlText(configText);
  const lines = normalizedText ? normalizedText.split("\n") : [];
  const topLevelEndIndex = getTopLevelEndIndex(lines);
  const existing = findTopLevelIntMatch(lines, fieldName, topLevelEndIndex);
  const replacementLine = `${fieldName} = ${value}`;

  if (existing) {
    lines[existing.index] = replacementLine;
    return finalizeTomlText(lines);
  }

  // 插入位置：顶级区域末尾（section header 之前）
  if (lines.length === 0) {
    return `${replacementLine}\n`;
  }

  lines.splice(topLevelEndIndex, 0, replacementLine);
  return finalizeTomlText(lines);
};

// 从 Codex TOML 配置中移除顶级字段行
export const removeCodexTopLevelField = (
  configText: string,
  fieldName: string,
): string => {
  const normalizedText = normalizeTomlText(configText);
  if (!normalizedText) return normalizedText;
  const lines = normalizedText.split("\n");
  const topLevelEndIndex = getTopLevelEndIndex(lines);
  const existing = findTopLevelIntMatch(lines, fieldName, topLevelEndIndex);
  if (existing) {
    lines.splice(existing.index, 1);
  }
  return finalizeTomlText(lines);
};
