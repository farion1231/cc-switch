// 供应商配置处理工具函数

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

const deepRemove = (target: Record<string, any>, source: Record<string, any>) => {
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
  if (obj instanceof Array) return obj.map(item => deepClone(item)) as T;
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

  let snippet: Record<string, any>;
  try {
    const parsed = JSON.parse(snippetString);
    if (!isPlainObject(parsed)) {
      return {
        updatedConfig: JSON.stringify(config, null, 2),
        error: "通用配置片段必须是 JSON 对象",
      };
    }
    snippet = parsed;
  } catch (err) {
    return {
      updatedConfig: JSON.stringify(config, null, 2),
      error: "通用配置片段格式错误，需为合法 JSON",
    };
  }

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

// 读取配置中的 API Key（env.ANTHROPIC_AUTH_TOKEN）
export const getApiKeyFromConfig = (jsonString: string): string => {
  try {
    const config = JSON.parse(jsonString);
    const key = config?.env?.ANTHROPIC_AUTH_TOKEN;
    return typeof key === "string" ? key : "";
  } catch (err) {
    return "";
  }
};

// 判断配置中是否存在 API Key 字段
export const hasApiKeyField = (jsonString: string): boolean => {
  try {
    const config = JSON.parse(jsonString);
    return Object.prototype.hasOwnProperty.call(
      config?.env ?? {},
      "ANTHROPIC_AUTH_TOKEN",
    );
  } catch (err) {
    return false;
  }
};

// 写入/更新配置中的 API Key，默认不新增缺失字段
export const setApiKeyInConfig = (
  jsonString: string,
  apiKey: string,
  options: { createIfMissing?: boolean } = {},
): string => {
  const { createIfMissing = false } = options;
  try {
    const config = JSON.parse(jsonString);
    if (!config.env) {
      if (!createIfMissing) return jsonString;
      config.env = {};
    }
    if (!("ANTHROPIC_AUTH_TOKEN" in config.env) && !createIfMissing) {
      return jsonString;
    }
    config.env.ANTHROPIC_AUTH_TOKEN = apiKey;
    return JSON.stringify(config, null, 2);
  } catch (err) {
    return jsonString;
  }
};

// ========== TOML Config Utilities ==========

const COMMON_CONFIG_MARKER_START = "# === COMMON CONFIG START ===";
const COMMON_CONFIG_MARKER_END = "# === COMMON CONFIG END ===";

export interface UpdateTomlCommonConfigResult {
  updatedConfig: string;
  error?: string;
}

// 将通用配置片段写入/移除 TOML 配置
export const updateTomlCommonConfigSnippet = (
  tomlString: string,
  snippetString: string,
  enabled: boolean,
): UpdateTomlCommonConfigResult => {
  if (!snippetString.trim()) {
    // 如果片段为空，移除已存在的通用配置部分
    const cleaned = removeTomlCommonConfig(tomlString);
    return {
      updatedConfig: cleaned,
    };
  }

  if (enabled) {
    // 添加通用配置
    const withoutOld = removeTomlCommonConfig(tomlString);
    const commonSection = `\n${COMMON_CONFIG_MARKER_START}\n${snippetString}\n${COMMON_CONFIG_MARKER_END}\n`;
    return {
      updatedConfig: withoutOld + commonSection,
    };
  } else {
    // 移除通用配置
    const cleaned = removeTomlCommonConfig(tomlString);
    return {
      updatedConfig: cleaned,
    };
  }
};

// 从 TOML 中移除通用配置部分
const removeTomlCommonConfig = (tomlString: string): string => {
  const startIdx = tomlString.indexOf(COMMON_CONFIG_MARKER_START);
  const endIdx = tomlString.indexOf(COMMON_CONFIG_MARKER_END);
  
  if (startIdx === -1 || endIdx === -1) {
    return tomlString;
  }
  
  // 找到标记前的换行符（如果有）
  let realStartIdx = startIdx;
  if (startIdx > 0 && tomlString[startIdx - 1] === '\n') {
    realStartIdx = startIdx - 1;
  }
  
  // 找到标记后的换行符（如果有）
  let realEndIdx = endIdx + COMMON_CONFIG_MARKER_END.length;
  if (realEndIdx < tomlString.length && tomlString[realEndIdx] === '\n') {
    realEndIdx = realEndIdx + 1;
  }
  
  return tomlString.slice(0, realStartIdx) + tomlString.slice(realEndIdx);
};

// 检查 TOML 配置是否已包含通用配置片段
export const hasTomlCommonConfigSnippet = (
  tomlString: string,
  snippetString: string,
): boolean => {
  if (!snippetString.trim()) return false;
  
  const startIdx = tomlString.indexOf(COMMON_CONFIG_MARKER_START);
  const endIdx = tomlString.indexOf(COMMON_CONFIG_MARKER_END);
  
  if (startIdx === -1 || endIdx === -1 || startIdx >= endIdx) {
    return false;
  }
  
  // 提取标记之间的内容
  const existingSnippet = tomlString
    .slice(startIdx + COMMON_CONFIG_MARKER_START.length, endIdx)
    .trim();
  
  return existingSnippet === snippetString.trim();
};
