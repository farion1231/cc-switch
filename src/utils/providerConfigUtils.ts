// 供应商配置处理工具函数

// 处理includeCoAuthoredBy字段的添加/删除
export const updateCoAuthoredSetting = (
  jsonString: string,
  disable: boolean,
): string => {
  try {
    const config = JSON.parse(jsonString);

    if (disable) {
      // 添加或更新includeCoAuthoredBy字段
      config.includeCoAuthoredBy = false;
    } else {
      // 删除includeCoAuthoredBy字段
      delete config.includeCoAuthoredBy;
    }

    return JSON.stringify(config, null, 2);
  } catch (err) {
    // 如果JSON解析失败，返回原始字符串
    return jsonString;
  }
};

// 从JSON配置中检查是否包含includeCoAuthoredBy设置
export const checkCoAuthoredSetting = (jsonString: string): boolean => {
  try {
    const config = JSON.parse(jsonString);
    return config.includeCoAuthoredBy === false;
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
