/**
 * Qoder CLI BYOK provider catalog.
 *
 * Qoder does not route arbitrary OpenAI-compatible endpoints. The provider,
 * model and plan type must match the catalog returned by Qoder. Keep this list
 * intentionally separate from the Claude/Codex presets.
 */
import type { ProviderCategory } from "../types";

export type QoderCliPlanType = "cp" | "tp" | "pg";

export function getQoderCliPlanLabel(type: QoderCliPlanType): string {
  if (type === "cp") return "Coding Plan";
  if (type === "tp") return "Token Plan";
  return "Pay As You Go";
}

export interface QoderCliPresetModel {
  model: string;
  type: QoderCliPlanType;
  format: "openai";
  displayName: string;
  maxInputTokens?: number;
  isVl?: boolean;
  isReasoning?: boolean;
}

export function getQoderCliModelDisplayLabel(
  model: Pick<QoderCliPresetModel, "displayName" | "type">,
): string {
  const planLabel = getQoderCliPlanLabel(model.type);
  return planLabel ? `${model.displayName} · ${planLabel}` : model.displayName;
}

export interface QoderCliProviderConfig {
  provider: string;
  apiKey: string;
  /** The first (and currently only) item becomes Qoder's active model. */
  models: QoderCliPresetModel[];
}

export interface QoderCliProviderPreset {
  name: string;
  nameKey?: string;
  websiteUrl: string;
  apiKeyUrl: string;
  providerKey: string;
  settingsConfig: QoderCliProviderConfig;
  /** Complete model catalog for this Qoder provider. */
  models: QoderCliPresetModel[];
  category: ProviderCategory;
  icon: string;
  iconColor: string;
  isOfficial?: boolean;
  isPartner?: boolean;
  primePartner?: boolean;
  partnerPromotionKey?: string;
  theme?: {
    icon?: "claude" | "codex" | "gemini" | "generic";
    backgroundColor?: string;
    textColor?: string;
  };
}

const qoderModel = (
  model: string,
  displayName: string,
  type: QoderCliPlanType,
  maxInputTokens: number,
  isVl = false,
): QoderCliPresetModel => ({
  model,
  displayName,
  type,
  format: "openai",
  maxInputTokens,
  isReasoning: true,
  ...(isVl ? { isVl: true } : {}),
});

const bailianTpModels = [
  qoderModel("qwen3.8-max-tp", "Qwen 3.8 Max Preview", "tp", 1_000_000, true),
  qoderModel("qwen3.7-max-tp", "Qwen 3.7 Max", "tp", 1_000_000),
  qoderModel("qwen3.7-plus-tp", "Qwen 3.7 Plus", "tp", 1_000_000, true),
  qoderModel("qwen3.6-plus-tp", "Qwen 3.6 Plus", "tp", 1_000_000, true),
  qoderModel("qwen3.6-flash-tp", "Qwen 3.6 Flash", "tp", 1_000_000, true),
  qoderModel("glm5.2-tp", "GLM 5.2", "tp", 1_000_000),
  qoderModel("glm5.1-tp", "GLM 5.1", "tp", 202_000),
  qoderModel("glm5-tp", "GLM 5", "tp", 198_000),
  qoderModel("kimi-k2.7-code-tp", "Kimi K2.7 Code", "tp", 256_000, true),
  qoderModel("kimi-k2.6-tp", "Kimi K2.6", "tp", 256_000, true),
  qoderModel("kimi-k2.5-tp", "Kimi K2.5", "tp", 256_000, true),
  qoderModel("deepseek-v4-flash-tp", "DeepSeek V4 Flash", "tp", 1_000_000),
  qoderModel("deepseek-v4-pro-tp", "DeepSeek V4 Pro", "tp", 1_000_000),
  qoderModel("minimax-m2.5-tp", "MiniMax M2.5", "tp", 200_000),
];

const bailianCpModels = [
  qoderModel("qwen3.7-plus-cp", "Qwen 3.7 Plus", "cp", 1_000_000, true),
  qoderModel("qwen3.6-plus-cp", "Qwen 3.6 Plus", "cp", 1_000_000, true),
  qoderModel("glm5-cp", "GLM 5", "cp", 180_000),
  qoderModel("kimi-k2.5-cp", "Kimi K2.5", "cp", 256_000, true),
  qoderModel("minimax-m2.5-cp", "MiniMax M2.5", "cp", 200_000),
];

const bailianPgModels = [
  qoderModel("qwen3.7-max-pg", "Qwen 3.7 Max", "pg", 1_000_000),
  qoderModel("qwen3.7-plus-pg", "Qwen 3.7 Plus", "pg", 1_000_000, true),
  qoderModel("qwen3.6-max-pg", "Qwen 3.6 Max", "pg", 1_000_000, true),
  qoderModel("qwen3.6-plus-pg", "Qwen 3.6 Plus", "pg", 1_000_000, true),
  qoderModel("glm5.2-pg", "GLM 5.2", "pg", 1_000_000),
  qoderModel("deepseek-v4-pro-pg", "DeepSeek V4 Pro", "pg", 1_000_000),
];

const bailianModels = [
  ...bailianTpModels,
  ...bailianCpModels,
  ...bailianPgModels,
];

const zhipuModels = [
  qoderModel("glm5.2-cp", "GLM 5.2", "cp", 1_000_000),
  qoderModel("glm5.1-cp", "GLM 5.1", "cp", 200_000),
  qoderModel("glm-5v-turbo-cp", "GLM 5V Turbo", "cp", 200_000, true),
  qoderModel("glm5-cp", "GLM 5", "cp", 200_000),
  qoderModel("glm4.7-cp", "GLM 4.7", "cp", 200_000),
  qoderModel("glm4.6-cp", "GLM 4.6", "cp", 200_000),
  qoderModel("glm5.2-pg", "GLM 5.2", "pg", 1_000_000),
  qoderModel("glm5.1-pg", "GLM 5.1", "pg", 200_000),
  qoderModel("glm-5v-turbo-pg", "GLM 5V Turbo", "pg", 200_000, true),
  qoderModel("glm5-pg", "GLM 5", "pg", 200_000),
];

const zhipuIntlModels = zhipuModels.filter(
  (model) => model.model !== "glm-5v-turbo-cp",
);

const kimiModels = [
  qoderModel("kimi-k3-pg", "Kimi K3", "pg", 1_000_000, true),
  qoderModel("kimi-k2.7-code-pg", "Kimi K2.7 Code", "pg", 256_000, true),
  qoderModel(
    "kimi-k2.7-code-highspeed-pg",
    "Kimi K2.7 Code Highspeed",
    "pg",
    256_000,
    true,
  ),
  qoderModel("kimi-k2.6-pg", "Kimi K2.6", "pg", 256_000, true),
  qoderModel("kimi-k3-cp", "Kimi K3", "cp", 1_000_000, true),
  qoderModel("kimi-k2.7-code-cp", "Kimi K2.7 Code", "cp", 256_000, true),
  qoderModel(
    "kimi-k2.7-code-highspeed-cp",
    "Kimi K2.7 Code Highspeed",
    "cp",
    256_000,
    true,
  ),
  qoderModel("kimi-k2.6-cp", "Kimi K2.6", "cp", 256_000, true),
  qoderModel("kimi-for-coding-cp", "Kimi for Coding", "cp", 256_000, true),
];

const minimaxModels = [
  qoderModel("minimax-m3-cp", "MiniMax M3", "cp", 1_000_000, true),
  qoderModel("minimax-m2.7-cp", "MiniMax M2.7", "cp", 200_000),
  qoderModel(
    "minimax-m2.7-highspeed-cp",
    "MiniMax M2.7 Highspeed",
    "cp",
    200_000,
  ),
  qoderModel("minimax-m2.5-cp", "MiniMax M2.5", "cp", 200_000),
];

const deepseekModels = [
  qoderModel("deepseek-v4-pro-pg", "DeepSeek V4 Pro", "pg", 1_000_000),
  qoderModel("deepseek-v4-flash-pg", "DeepSeek V4 Flash", "pg", 1_000_000),
];

const xiaomiModels = [
  qoderModel("mimo-v2.5-pro-tp", "MiMo V2.5 Pro", "tp", 1_000_000, true),
  qoderModel("mimo-v2.5-tp", "MiMo V2.5", "tp", 1_000_000, true),
  qoderModel("mimo-v2.5-pro-pg", "MiMo V2.5 Pro", "pg", 1_000_000, true),
  qoderModel("mimo-v2.5-pg", "MiMo V2.5", "pg", 1_000_000, true),
];

const createPreset = (
  providerKey: string,
  name: string,
  models: QoderCliPresetModel[],
  websiteUrl: string,
  apiKeyUrl: string,
  icon: string,
  iconColor: string,
): QoderCliProviderPreset => ({
  name,
  websiteUrl,
  apiKeyUrl,
  providerKey,
  settingsConfig: {
    provider: providerKey,
    apiKey: "",
    models: models.slice(0, 1),
  },
  models,
  category: "cn_official",
  icon,
  iconColor,
});

export const qodercliProviderPresets: QoderCliProviderPreset[] = [
  createPreset(
    "deepseek",
    "DeepSeek",
    deepseekModels,
    "https://www.deepseek.com",
    "https://platform.deepseek.com/api_keys",
    "deepseek",
    "#1E88E5",
  ),
  createPreset(
    "bailian",
    "Alibaba Cloud Model Studio（中国）",
    bailianModels,
    "https://bailian.console.aliyun.com",
    "https://bailian.console.aliyun.com/cn-beijing/?tab=model#/efm/coding_plan",
    "bailian",
    "#624AFF",
  ),
  createPreset(
    "bailian-intl",
    "Alibaba Cloud Model Studio（新加坡）",
    bailianModels,
    "https://modelstudio.console.alibabacloud.com",
    "https://modelstudio.console.alibabacloud.com/ap-southeast-1/?tab=globalset#/efm/coding_plan",
    "bailian",
    "#624AFF",
  ),
  createPreset(
    "bailian-america",
    "Alibaba Cloud Model Studio（美国）",
    bailianPgModels,
    "https://modelstudio.console.aliyun.com",
    "https://modelstudio.console.aliyun.com/us-east-1?tab=globalset#/efm/api_key",
    "bailian",
    "#624AFF",
  ),
  createPreset(
    "zhipu",
    "Z.ai（中国）",
    zhipuModels,
    "https://bigmodel.cn",
    "https://bigmodel.cn/usercenter/proj-mgmt/apikeys",
    "zhipu",
    "#0F62FE",
  ),
  createPreset(
    "zhipu-intl",
    "Z.ai（国际）",
    zhipuIntlModels,
    "https://z.ai",
    "https://z.ai/manage-apikey/subscription",
    "zhipu",
    "#0F62FE",
  ),
  createPreset(
    "kimi",
    "Kimi",
    kimiModels,
    "https://www.kimi.com",
    "https://www.kimi.com/code/console",
    "kimi",
    "#6366F1",
  ),
  createPreset(
    "minimax",
    "MiniMax（中国）",
    minimaxModels,
    "https://www.minimaxi.com",
    "https://platform.minimaxi.com/user-center/basic-information/interface-key",
    "minimax",
    "#FF6B6B",
  ),
  createPreset(
    "minimax-intl",
    "MiniMax（国际）",
    minimaxModels,
    "https://www.minimax.io",
    "https://platform.minimax.io/user-center/basic-information/interface-key",
    "minimax",
    "#FF6B6B",
  ),
  createPreset(
    "xiaomi-china",
    "Xiaomi MiMo",
    xiaomiModels,
    "https://platform.xiaomimimo.com",
    "https://platform.xiaomimimo.com/console/plan-manage",
    "xiaomi",
    "#FF6900",
  ),
];

export const QODERCLI_SUPPORTED_PROVIDER_KEYS = qodercliProviderPresets.map(
  (preset) => preset.providerKey,
);

export function isQoderCliSupportedProvider(provider: string): boolean {
  return QODERCLI_SUPPORTED_PROVIDER_KEYS.includes(provider);
}

export function getQoderCliPreset(
  provider: string,
): QoderCliProviderPreset | undefined {
  return qodercliProviderPresets.find(
    (preset) => preset.providerKey === provider,
  );
}

export function isQoderCliSupportedModel(
  provider: string,
  model: Pick<QoderCliPresetModel, "model" | "type" | "format">,
): boolean {
  return (
    getQoderCliPreset(provider)?.models.some(
      (item) =>
        item.model === model.model &&
        item.type === model.type &&
        item.format === model.format,
    ) ?? false
  );
}

export function getQoderCliSupportedPlanTypes(
  provider: string,
): QoderCliPlanType[] {
  const planTypes = getQoderCliPreset(provider)?.models.map(
    (model) => model.type,
  );
  return [...new Set(planTypes ?? [])];
}

/**
 * Qoder restricts BYOK providers and plan types to its catalog, but its CLI
 * also allows users to enter another model ID within a supported group.
 */
export function isQoderCliAllowedModel(
  provider: string,
  model: Pick<QoderCliPresetModel, "model" | "type" | "format">,
): boolean {
  return (
    model.model.trim().length > 0 &&
    model.format === "openai" &&
    getQoderCliSupportedPlanTypes(provider).includes(model.type)
  );
}

/**
 * Qoder's live model key is also the stable CC Switch record ID.
 *
 * A supplier can expose multiple BYOK models, so using only the supplier key
 * (for example `deepseek`) would make the second model overwrite the first.
 */
export function buildQoderCliModelProviderId(
  provider: string,
  model: Pick<QoderCliPresetModel, "model">,
): string {
  const providerKey = provider.trim();
  const modelId = model.model.trim();
  if (!providerKey || !modelId) {
    return "";
  }
  return `${providerKey}/${modelId}`;
}
