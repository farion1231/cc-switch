// 供应商配置处理工具函数

import type { TemplateValueConfig } from "../config/claudeProviderPresets";
import { normalizeQuotes } from "@/utils/textNormalization";
import { isPlainObject } from "@/utils/configMerge";

// Gemini 通用配置禁止的键（共享常量，供 hook 和同步逻辑复用）
export const GEMINI_COMMON_ENV_FORBIDDEN_KEYS = [
  "GOOGLE_GEMINI_BASE_URL",
  "GEMINI_API_KEY",
] as const;
export type GeminiForbiddenEnvKey =
  (typeof GEMINI_COMMON_ENV_FORBIDDEN_KEYS)[number];

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

// 检查 Gemini 配置是否已包含通用配置片段（env JSON）
export const hasGeminiCommonConfigSnippet = (
  jsonString: string,
  snippetString: string,
): boolean => {
  try {
    if (!snippetString.trim()) return false;
    const config = jsonString ? JSON.parse(jsonString) : {};
    if (!isPlainObject(config)) return false;
    const envValue = (config as Record<string, unknown>).env;
    if (envValue !== undefined && !isPlainObject(envValue)) return false;
    const env = (isPlainObject(envValue) ? envValue : {}) as Record<
      string,
      unknown
    >;

    const parsed = JSON.parse(snippetString);
    if (!isPlainObject(parsed)) return false;

    const entries = Object.entries(parsed).filter(
      (entry): entry is [string, string] => {
        const [key, value] = entry;
        if (
          GEMINI_COMMON_ENV_FORBIDDEN_KEYS.includes(
            key as GeminiForbiddenEnvKey,
          )
        ) {
          return false;
        }
        if (typeof value !== "string") return false;
        return value.trim().length > 0;
      },
    );

    if (entries.length === 0) return false;

    return entries.every(([key, value]) => {
      const current = env[key];
      return typeof current === "string" && current === value.trim();
    });
  } catch {
    return false;
  }
};

/**
 * 替换通用配置片段（用于同步更新）
 * 先移除旧的通用配置，再添加新的通用配置
 */
export const replaceCommonConfigSnippet = (
  jsonString: string,
  oldSnippet: string,
  newSnippet: string,
): UpdateCommonConfigResult => {
  // 先移除旧的通用配置
  const removeResult = updateCommonConfigSnippet(jsonString, oldSnippet, false);
  if (removeResult.error) {
    return removeResult;
  }

  // 再添加新的通用配置
  return updateCommonConfigSnippet(
    removeResult.updatedConfig,
    newSnippet,
    true,
  );
};

/**
 * 替换 Gemini 通用配置片段（用于同步更新）
 * Gemini 的通用配置是 JSON 格式的 env 对象
 */
export const replaceGeminiCommonConfigSnippet = (
  jsonString: string,
  oldSnippet: string,
  newSnippet: string,
): UpdateCommonConfigResult => {
  try {
    const config = jsonString ? JSON.parse(jsonString) : {};

    // 校验 config 是对象类型
    if (!isPlainObject(config)) {
      return {
        updatedConfig: jsonString,
        error: "CONFIG_NOT_OBJECT",
      };
    }

    const env = config.env ?? {};

    // 校验 env 是对象类型
    if (!isPlainObject(env)) {
      return {
        updatedConfig: jsonString,
        error: "ENV_NOT_OBJECT",
      };
    }

    // 解析旧的通用配置
    let oldEnv: Record<string, string> = {};
    if (oldSnippet.trim()) {
      try {
        const parsed = JSON.parse(oldSnippet);
        if (isPlainObject(parsed)) {
          oldEnv = parsed as Record<string, string>;
        }
      } catch {
        // ignore parse error
      }
    }

    // 解析新的通用配置
    let newEnv: Record<string, string> = {};
    if (newSnippet.trim()) {
      try {
        const parsed = JSON.parse(newSnippet);
        if (isPlainObject(parsed)) {
          // 过滤掉禁止的键
          for (const [key, value] of Object.entries(parsed)) {
            if (
              typeof value === "string" &&
              !GEMINI_COMMON_ENV_FORBIDDEN_KEYS.includes(
                key as GeminiForbiddenEnvKey,
              )
            ) {
              newEnv[key] = value;
            }
          }
        }
      } catch {
        return {
          updatedConfig: jsonString,
          error: "COMMON_CONFIG_JSON_INVALID",
        };
      }
    }

    // 移除旧的通用配置键值对
    const updatedEnv = { ...env };
    for (const [key, value] of Object.entries(oldEnv)) {
      if (updatedEnv[key] === value) {
        delete updatedEnv[key];
      }
    }

    // 添加新的通用配置键值对
    for (const [key, value] of Object.entries(newEnv)) {
      updatedEnv[key] = value;
    }

    return {
      updatedConfig: JSON.stringify({ ...config, env: updatedEnv }, null, 2),
    };
  } catch (err) {
    return {
      updatedConfig: jsonString,
      error: "CONFIG_JSON_PARSE_FAILED",
    };
  }
};

// 读取配置中的 API Key（支持 Claude, Codex, Gemini）
export const getApiKeyFromConfig = (
  jsonString: string,
  appType?: string,
): string => {
  try {
    const config = JSON.parse(jsonString);
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
  options: { createIfMissing?: boolean; appType?: string } = {},
): string => {
  const { createIfMissing = false, appType } = options;
  try {
    const config = JSON.parse(jsonString);
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

    // Claude API Key (优先写入已存在的字段；若两者均不存在且允许创建，则默认创建 AUTH_TOKEN 字段)
    if ("ANTHROPIC_AUTH_TOKEN" in env) {
      env.ANTHROPIC_AUTH_TOKEN = apiKey;
    } else if ("ANTHROPIC_API_KEY" in env) {
      env.ANTHROPIC_API_KEY = apiKey;
    } else if (createIfMissing) {
      env.ANTHROPIC_AUTH_TOKEN = apiKey;
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

const TOML_COMMON_CONFIG_START = "# cc-switch:common-config:start";
const TOML_COMMON_CONFIG_END = "# cc-switch:common-config:end";

/**
 * 移除旧版标记块（兼容性清理）
 * 仅用于清理历史遗留的标记，新版本不再写入标记
 */
const stripTomlCommonConfigBlock = (tomlString: string): string => {
  const startIndex = tomlString.indexOf(TOML_COMMON_CONFIG_START);
  const endIndex = tomlString.indexOf(TOML_COMMON_CONFIG_END);
  if (startIndex === -1 || endIndex === -1 || endIndex < startIndex) {
    return tomlString;
  }
  const before = tomlString.slice(0, startIndex);
  const after = tomlString.slice(endIndex + TOML_COMMON_CONFIG_END.length);
  return `${before}${after}`;
};

/**
 * 解析 TOML 文本为段落结构
 * 返回 { topLevel: string[], sections: Map<string, string[]> }
 * - topLevel: 顶级键值对行（在任何 [section] 之前）
 * - sections: 每个 [section] 及其内容行
 */
interface TomlParsedStructure {
  topLevel: string[];
  sections: Map<string, string[]>;
  sectionOrder: string[];
}

function parseTomlStructure(tomlText: string): TomlParsedStructure {
  const lines = tomlText.split("\n");
  const topLevel: string[] = [];
  const sections = new Map<string, string[]>();
  const sectionOrder: string[] = [];

  let currentSection: string | null = null;

  for (const line of lines) {
    const trimmed = line.trim();

    // 检查是否是 section 头（如 [mcp_servers.mcpServers]）
    const sectionMatch = trimmed.match(/^\[([^\]]+)\]$/);
    if (sectionMatch) {
      currentSection = sectionMatch[1];
      if (!sections.has(currentSection)) {
        sections.set(currentSection, []);
        sectionOrder.push(currentSection);
      }
      continue;
    }

    if (currentSection === null) {
      // 顶级内容
      topLevel.push(line);
    } else {
      // section 内容
      const sectionLines = sections.get(currentSection)!;
      sectionLines.push(line);
    }
  }

  return { topLevel, sections, sectionOrder };
}

/**
 * 从顶级行中提取键名
 */
function extractKeyFromLine(line: string): string | null {
  const trimmed = line.trim();
  // 跳过空行和注释
  if (!trimmed || trimmed.startsWith("#")) {
    return null;
  }
  // 匹配 key = value 格式
  const match = trimmed.match(/^([a-zA-Z_][a-zA-Z0-9_]*)\s*=/);
  return match ? match[1] : null;
}

/**
 * 智能合并 TOML 配置
 * - 顶级键值对：snippet 中的键会覆盖或添加到 base 的顶级区域
 * - sections：同名 section 会合并内容，不同 section 会追加
 */
function mergeTomlConfigs(baseText: string, snippetText: string): string {
  const base = parseTomlStructure(baseText);
  const snippet = parseTomlStructure(snippetText);

  // 1. 合并顶级键值对
  // 提取 snippet 中的顶级键
  const snippetTopLevelKeys = new Set<string>();
  for (const line of snippet.topLevel) {
    const key = extractKeyFromLine(line);
    if (key) {
      snippetTopLevelKeys.add(key);
    }
  }

  // 过滤 base 中被 snippet 覆盖的键
  const filteredBaseTopLevel = base.topLevel.filter((line) => {
    const key = extractKeyFromLine(line);
    return key === null || !snippetTopLevelKeys.has(key);
  });

  // 合并顶级内容：base 的非覆盖行 + snippet 的顶级行
  const mergedTopLevel = [...filteredBaseTopLevel];
  // 添加 snippet 的顶级行（跳过空行，除非是有意义的）
  for (const line of snippet.topLevel) {
    const trimmed = line.trim();
    if (trimmed) {
      mergedTopLevel.push(line);
    }
  }

  // 2. 合并 sections
  const mergedSections = new Map<string, string[]>();
  const mergedSectionOrder: string[] = [];

  // 先添加 base 的 sections
  for (const sectionName of base.sectionOrder) {
    mergedSections.set(sectionName, [...base.sections.get(sectionName)!]);
    mergedSectionOrder.push(sectionName);
  }

  // 合并 snippet 的 sections
  for (const sectionName of snippet.sectionOrder) {
    const snippetLines = snippet.sections.get(sectionName)!;

    if (mergedSections.has(sectionName)) {
      // 同名 section：合并内容
      const existingLines = mergedSections.get(sectionName)!;

      // 提取 snippet section 中的键
      const snippetSectionKeys = new Set<string>();
      for (const line of snippetLines) {
        const key = extractKeyFromLine(line);
        if (key) {
          snippetSectionKeys.add(key);
        }
      }

      // 过滤 existing 中被覆盖的键
      const filteredExisting = existingLines.filter((line) => {
        const key = extractKeyFromLine(line);
        return key === null || !snippetSectionKeys.has(key);
      });

      // 合并：existing 的非覆盖行 + snippet 的行
      const merged = [...filteredExisting];
      for (const line of snippetLines) {
        const trimmed = line.trim();
        if (trimmed) {
          merged.push(line);
        }
      }
      mergedSections.set(sectionName, merged);
    } else {
      // 新 section：直接添加
      mergedSections.set(sectionName, [...snippetLines]);
      mergedSectionOrder.push(sectionName);
    }
  }

  // 3. 重建 TOML 文本
  const resultLines: string[] = [];

  // 添加顶级内容（清理多余空行）
  let lastWasEmpty = true;
  for (const line of mergedTopLevel) {
    const isEmpty = !line.trim();
    if (isEmpty && lastWasEmpty) {
      continue; // 跳过连续空行
    }
    resultLines.push(line);
    lastWasEmpty = isEmpty;
  }

  // 添加 sections
  for (const sectionName of mergedSectionOrder) {
    const sectionLines = mergedSections.get(sectionName)!;

    // 确保 section 前有空行分隔
    if (resultLines.length > 0 && resultLines[resultLines.length - 1].trim()) {
      resultLines.push("");
    }

    // 添加 section 头
    resultLines.push(`[${sectionName}]`);

    // 添加 section 内容（清理多余空行）
    lastWasEmpty = false;
    for (const line of sectionLines) {
      const isEmpty = !line.trim();
      if (isEmpty && lastWasEmpty) {
        continue;
      }
      resultLines.push(line);
      lastWasEmpty = isEmpty;
    }
  }

  // 清理末尾空行，确保以单个换行结尾
  while (
    resultLines.length > 0 &&
    !resultLines[resultLines.length - 1].trim()
  ) {
    resultLines.pop();
  }

  return resultLines.join("\n") + "\n";
}

/**
 * 从 TOML 配置中移除指定的片段内容
 * - 移除 snippet 中定义的顶级键
 * - 移除 snippet 中定义的 section 内的键（如果 section 变空则移除整个 section）
 */
function removeTomlSnippet(baseText: string, snippetText: string): string {
  const base = parseTomlStructure(baseText);
  const snippet = parseTomlStructure(snippetText);

  // 1. 从顶级移除 snippet 的键
  const snippetTopLevelKeys = new Set<string>();
  for (const line of snippet.topLevel) {
    const key = extractKeyFromLine(line);
    if (key) {
      snippetTopLevelKeys.add(key);
    }
  }

  const filteredTopLevel = base.topLevel.filter((line) => {
    const key = extractKeyFromLine(line);
    return key === null || !snippetTopLevelKeys.has(key);
  });

  // 2. 从 sections 移除 snippet 的键
  const filteredSections = new Map<string, string[]>();
  const filteredSectionOrder: string[] = [];

  for (const sectionName of base.sectionOrder) {
    const baseLines = base.sections.get(sectionName)!;

    // 检查 snippet 是否有这个 section
    if (snippet.sections.has(sectionName)) {
      const snippetLines = snippet.sections.get(sectionName)!;

      // 提取 snippet section 中的键
      const snippetSectionKeys = new Set<string>();
      for (const line of snippetLines) {
        const key = extractKeyFromLine(line);
        if (key) {
          snippetSectionKeys.add(key);
        }
      }

      // 过滤掉 snippet 中的键
      const filtered = baseLines.filter((line) => {
        const key = extractKeyFromLine(line);
        return key === null || !snippetSectionKeys.has(key);
      });

      // 检查过滤后是否还有实质内容
      const hasContent = filtered.some((line) => {
        const trimmed = line.trim();
        return trimmed && !trimmed.startsWith("#");
      });

      if (hasContent) {
        filteredSections.set(sectionName, filtered);
        filteredSectionOrder.push(sectionName);
      }
      // 如果没有实质内容，整个 section 被移除
    } else {
      // snippet 中没有这个 section，保留
      filteredSections.set(sectionName, baseLines);
      filteredSectionOrder.push(sectionName);
    }
  }

  // 3. 重建 TOML 文本
  const resultLines: string[] = [];

  // 添加顶级内容
  let lastWasEmpty = true;
  for (const line of filteredTopLevel) {
    const isEmpty = !line.trim();
    if (isEmpty && lastWasEmpty) {
      continue;
    }
    resultLines.push(line);
    lastWasEmpty = isEmpty;
  }

  // 添加 sections
  for (const sectionName of filteredSectionOrder) {
    const sectionLines = filteredSections.get(sectionName)!;

    if (resultLines.length > 0 && resultLines[resultLines.length - 1].trim()) {
      resultLines.push("");
    }

    resultLines.push(`[${sectionName}]`);

    lastWasEmpty = false;
    for (const line of sectionLines) {
      const isEmpty = !line.trim();
      if (isEmpty && lastWasEmpty) {
        continue;
      }
      resultLines.push(line);
      lastWasEmpty = isEmpty;
    }
  }

  while (
    resultLines.length > 0 &&
    !resultLines[resultLines.length - 1].trim()
  ) {
    resultLines.pop();
  }

  return resultLines.length > 0 ? resultLines.join("\n") + "\n" : "";
}

// 将通用配置片段写入/移除 TOML 配置
// 使用智能合并，避免重复定义 section
export const updateTomlCommonConfigSnippet = (
  tomlString: string,
  snippetString: string,
  enabled: boolean,
): UpdateTomlCommonConfigResult => {
  if (enabled) {
    if (!snippetString.trim()) {
      return {
        updatedConfig: tomlString,
      };
    }

    // 先清理旧版标记块（兼容性）
    let baseConfig = stripTomlCommonConfigBlock(tomlString);
    baseConfig = baseConfig.replace(/\n{3,}/g, "\n\n").trim();
    if (baseConfig) {
      baseConfig += "\n";
    }

    // 使用智能合并
    const merged = mergeTomlConfigs(baseConfig, snippetString);

    return {
      updatedConfig: merged,
    };
  } else {
    // 移除通用配置
    // 先清理旧版标记块（兼容性）
    const strippedConfig = stripTomlCommonConfigBlock(tomlString);
    if (strippedConfig !== tomlString) {
      const cleaned = strippedConfig.replace(/\n{3,}/g, "\n\n").trim();
      return {
        updatedConfig: cleaned ? cleaned + "\n" : "",
      };
    }

    // 使用智能移除
    if (snippetString.trim()) {
      const removed = removeTomlSnippet(tomlString, snippetString);
      return {
        updatedConfig: removed,
      };
    }

    return {
      updatedConfig: tomlString,
    };
  }
};

// 检查 TOML 配置是否已包含通用配置片段
export const hasTomlCommonConfigSnippet = (
  tomlString: string,
  snippetString: string,
): boolean => {
  if (!snippetString.trim()) return false;

  // 简单检查配置是否包含片段内容
  // 去除空白字符后比较，避免格式差异影响
  const normalizeWhitespace = (str: string) => str.replace(/\s+/g, " ").trim();

  return normalizeWhitespace(tomlString).includes(
    normalizeWhitespace(snippetString),
  );
};

/**
 * 替换 TOML 通用配置片段（用于同步更新）
 * 先移除旧的通用配置，再添加新的通用配置
 */
export const replaceTomlCommonConfigSnippet = (
  tomlString: string,
  oldSnippet: string,
  newSnippet: string,
): UpdateTomlCommonConfigResult => {
  // 先清理旧版标记块（兼容性）
  let configWithoutOld = stripTomlCommonConfigBlock(tomlString);

  // 通过内容匹配移除旧片段
  if (oldSnippet.trim() && configWithoutOld.includes(oldSnippet.trim())) {
    configWithoutOld = configWithoutOld.replace(oldSnippet.trim(), "");
    // 清理多余的空行
    configWithoutOld = configWithoutOld.replace(/\n{3,}/g, "\n\n").trim();
    if (configWithoutOld) {
      configWithoutOld += "\n";
    }
  }

  // 添加新的通用配置
  return updateTomlCommonConfigSnippet(configWithoutOld, newSnippet, true);
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
  if (!trimmed) {
    return configText;
  }
  // 归一化原文本中的引号（既能匹配，也能输出稳定格式）
  const normalizedText = normalizeQuotes(configText);

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
  if (!trimmed) {
    return configText;
  }

  // 归一化原文本中的引号（既能匹配，也能输出稳定格式）
  const normalizedText = normalizeQuotes(configText);

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

// ============================================================================
// Gemini Common Config Parsing Utilities
// ============================================================================

/**
 * Error codes for Gemini common config parsing.
 * These codes are used for consistent error handling and i18n mapping.
 */
export const GEMINI_CONFIG_ERROR_CODES = {
  NOT_OBJECT: "GEMINI_CONFIG_NOT_OBJECT",
  ENV_NOT_OBJECT: "GEMINI_CONFIG_ENV_NOT_OBJECT",
  VALUE_NOT_STRING: "GEMINI_CONFIG_VALUE_NOT_STRING",
  FORBIDDEN_KEYS: "GEMINI_CONFIG_FORBIDDEN_KEYS",
} as const;

/**
 * Result of parsing Gemini common config snippet
 */
export interface GeminiCommonConfigParseResult {
  /** Parsed env key-value pairs (empty if invalid) */
  env: Record<string, string>;
  /** Error message if parsing/validation failed (starts with error code) */
  error?: string;
  /** Warning message (non-fatal, config still usable) */
  warning?: string;
}

/**
 * Parse Gemini common config snippet with full validation.
 *
 * Supports three formats:
 * - ENV format: KEY=VALUE lines (one per line, # for comments)
 * - Flat JSON: {"KEY": "VALUE", ...}
 * - Wrapped JSON: {"env": {"KEY": "VALUE", ...}}
 *
 * Validation rules:
 * - Forbidden keys (GOOGLE_GEMINI_BASE_URL, GEMINI_API_KEY) are rejected
 * - Non-string values are rejected
 * - Empty string values are filtered out
 * - Arrays and non-plain objects are rejected
 *
 * @param snippet - The common config snippet string
 * @param options - Optional configuration
 * @returns Parse result with env, error, and warning
 */
export function parseGeminiCommonConfigSnippet(
  snippet: string,
  options?: {
    /** If true, reject forbidden keys with error; otherwise filter them with warning */
    strictForbiddenKeys?: boolean;
  },
): GeminiCommonConfigParseResult {
  const trimmed = snippet.trim();
  if (!trimmed) {
    return { env: {} };
  }

  const strictForbiddenKeys = options?.strictForbiddenKeys ?? true;
  let rawEnv: Record<string, unknown> = {};
  let isJson = false;

  // Try JSON first
  try {
    const parsed = JSON.parse(trimmed);

    // Must be a plain object (not array, null, etc.)
    if (!isPlainObject(parsed)) {
      return {
        env: {},
        error: `${GEMINI_CONFIG_ERROR_CODES.NOT_OBJECT}: must be a JSON object, not array or primitive`,
      };
    }

    isJson = true;

    // Check if wrapped format {"env": {...}}
    if ("env" in parsed) {
      const envField = parsed.env;
      if (!isPlainObject(envField)) {
        return {
          env: {},
          error: `${GEMINI_CONFIG_ERROR_CODES.ENV_NOT_OBJECT}: 'env' field must be a plain object`,
        };
      }
      rawEnv = envField as Record<string, unknown>;
    } else {
      // Flat format
      rawEnv = parsed as Record<string, unknown>;
    }
  } catch {
    // Not JSON, parse as ENV format (KEY=VALUE lines)
    isJson = false;
    for (const line of trimmed.split("\n")) {
      const lineTrimmed = line.trim();
      if (!lineTrimmed || lineTrimmed.startsWith("#")) continue;
      const equalIndex = lineTrimmed.indexOf("=");
      if (equalIndex > 0) {
        const key = lineTrimmed.substring(0, equalIndex).trim();
        // Strip surrounding quotes (single or double) from value
        // e.g., KEY="value" or KEY='value' -> value
        const rawValue = lineTrimmed.substring(equalIndex + 1).trim();
        const value = rawValue.replace(/^["'](.*)["']$/, "$1");
        if (key) {
          rawEnv[key] = value;
        }
      }
    }
  }

  // Validate and filter entries
  const env: Record<string, string> = {};
  const warnings: string[] = [];
  const forbiddenKeysFound: string[] = [];

  for (const [key, value] of Object.entries(rawEnv)) {
    // Check forbidden keys
    if (
      GEMINI_COMMON_ENV_FORBIDDEN_KEYS.includes(key as GeminiForbiddenEnvKey)
    ) {
      forbiddenKeysFound.push(key);
      continue;
    }

    // Must be string
    if (typeof value !== "string") {
      if (isJson) {
        return {
          env: {},
          error: `${GEMINI_CONFIG_ERROR_CODES.VALUE_NOT_STRING}: value for '${key}' must be a string, got ${typeof value}`,
        };
      }
      // For ENV format, skip non-strings silently (shouldn't happen)
      continue;
    }

    // Filter empty strings
    const trimmedValue = value.trim();
    if (!trimmedValue) {
      continue;
    }

    env[key] = trimmedValue;
  }

  // Handle forbidden keys
  if (forbiddenKeysFound.length > 0) {
    const msg = `${GEMINI_CONFIG_ERROR_CODES.FORBIDDEN_KEYS}: ${forbiddenKeysFound.join(", ")}`;
    if (strictForbiddenKeys) {
      return { env: {}, error: msg };
    }
    warnings.push(msg);
  }

  return {
    env,
    warning: warnings.length > 0 ? warnings.join("; ") : undefined,
  };
}

/**
 * Map Gemini common config warning to i18n-friendly message.
 *
 * @param warning - The raw warning string from parseGeminiCommonConfigSnippet
 * @param t - The i18n translation function
 * @returns Translated warning message
 */
export function mapGeminiWarningToI18n(
  warning: string,
  t: (key: string, options?: { keys?: string; defaultValue?: string }) => string,
): string {
  if (warning.startsWith(GEMINI_CONFIG_ERROR_CODES.FORBIDDEN_KEYS)) {
    // Extract key list: "GEMINI_CONFIG_FORBIDDEN_KEYS: KEY1, KEY2" -> "KEY1, KEY2"
    const keys = warning.replace(
      `${GEMINI_CONFIG_ERROR_CODES.FORBIDDEN_KEYS}: `,
      "",
    );
    return t("geminiConfig.forbiddenKeysWarning", { keys });
  }
  // Other warnings: return as-is
  return warning;
}
