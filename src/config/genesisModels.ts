/** Chat-capable models from Genesis /v1/models?type=chat (2026-11-06).
 * Embedding and reranking models are excluded from coding clients. Users can
 * refresh the catalog in apps that offer model discovery as Genesis evolves.
 */
export const GENESIS_CHAT_MODELS = [
  "kimi-k2.7-code",
  "qwen3.7-plus",
  "glm-5.2",
  "qwen3.8-27b",
  "qwen3.8-max",
  "qwen3.8-flash",
  "kimi-k3",
  "deepseek-v4.1-flash",
  "glm-5.3",
  "glm-5.3-flash",
] as const;

export const GENESIS_BASE_URL = "https://token.moi.matrixorigin.cn";
export const GENESIS_OPENAI_BASE_URL = `${GENESIS_BASE_URL}/v1`;
export const GENESIS_WEBSITE_URL = "https://genesis.moi.matrixorigin.cn/taas/";
export const GENESIS_API_KEY_URL =
  "https://uc.moi.matrixorigin.cn/account/credentials?app=genesis";
