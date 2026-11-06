export const GENESIS_BASE_URL = "https://token.moi.matrixorigin.cn";
export const GENESIS_OPENAI_BASE_URL = `${GENESIS_BASE_URL}/v1`;
export const GENESIS_CHAT_MODELS_URL = `${GENESIS_OPENAI_BASE_URL}/models?type=chat`;
// A single starter is required by clients that cannot save an empty catalog.
// The rest of the account-visible catalog is discovered from the API.
export const GENESIS_DEFAULT_MODEL = "qwen3.8-flash";
export const GENESIS_WEBSITE_URL = "https://genesis.moi.matrixorigin.cn/taas/";
export const GENESIS_API_KEY_URL =
  "https://uc.moi.matrixorigin.cn/account/credentials?app=genesis";
