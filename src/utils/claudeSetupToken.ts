export const CLAUDE_SETUP_TOKEN_ENV = "CLAUDE_CODE_OAUTH_TOKEN";

const parseConfigObject = (
  settingsConfig: string | Record<string, unknown> | undefined,
): Record<string, unknown> | null => {
  if (!settingsConfig) return null;
  if (typeof settingsConfig === "string") {
    try {
      const parsed = JSON.parse(settingsConfig);
      return parsed && typeof parsed === "object" && !Array.isArray(parsed)
        ? (parsed as Record<string, unknown>)
        : null;
    } catch {
      return null;
    }
  }
  return typeof settingsConfig === "object" && !Array.isArray(settingsConfig)
    ? settingsConfig
    : null;
};

export const getClaudeSetupTokenFromConfig = (
  settingsConfig: string | Record<string, unknown> | undefined,
): string => {
  const config = parseConfigObject(settingsConfig);
  const env = config?.env;
  if (!env || typeof env !== "object" || Array.isArray(env)) {
    return "";
  }
  const token = (env as Record<string, unknown>)[CLAUDE_SETUP_TOKEN_ENV];
  return typeof token === "string" ? token : "";
};

export const hasClaudeSetupToken = (
  settingsConfig: string | Record<string, unknown> | undefined,
): boolean => getClaudeSetupTokenFromConfig(settingsConfig).trim().length > 0;

export const setClaudeSetupTokenInConfig = (
  jsonString: string,
  token: string,
): string => {
  try {
    const parsed = JSON.parse(jsonString || "{}");
    const config: Record<string, unknown> =
      parsed && typeof parsed === "object" && !Array.isArray(parsed)
        ? (parsed as Record<string, unknown>)
        : {};
    const trimmed = token.trim();
    const existingEnv =
      config.env && typeof config.env === "object" && !Array.isArray(config.env)
        ? { ...(config.env as Record<string, unknown>) }
        : {};

    if (trimmed) {
      existingEnv[CLAUDE_SETUP_TOKEN_ENV] = trimmed;
      config.env = existingEnv;
    } else if (CLAUDE_SETUP_TOKEN_ENV in existingEnv) {
      delete existingEnv[CLAUDE_SETUP_TOKEN_ENV];
      config.env = existingEnv;
    }

    return JSON.stringify(config, null, 2);
  } catch {
    return jsonString;
  }
};

export const clearClaudeSetupTokenInConfig = (jsonString: string): string =>
  setClaudeSetupTokenInConfig(jsonString, "");

export const looksLikeClaudeSetupToken = (token: string): boolean =>
  token.trim().startsWith("sk-ant-oat");
