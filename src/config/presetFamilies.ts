/**
 * 预设的「厂商 / 版本」：同一家的国内站、海外站、各种套餐在添加供应商第 1 步合成一行，
 * 第 2 步的预设条里再用分段控件选版本。只是显示用的分组，不影响请求和 `category`。
 *
 * 写法：预设上加 `family`（这里的 id）和 `versionKey`（版本标签，见 PRESET_VERSION_KEYS）。
 * 只给在同一个应用里真有多个版本的厂商打；某个应用里只剩一个版本时不打，照旧单独一行。
 * 没写 `versionKey` 的版本用它的域名当标签（例如 SudoCode 的 sudocode.chat / sudocode.us）。
 */
export interface PresetFamilyInfo {
  /** 合成那一行的名称 */
  name: string;
  /** 名称需要翻译时用的 i18n key */
  nameKey?: string;
}

export const PRESET_FAMILIES = {
  "aws-bedrock": { name: "AWS Bedrock" },
  "baidu-qianfan": { name: "Baidu Qianfan" },
  compshare: { name: "Compshare", nameKey: "providerForm.presets.ucloud" },
  kimi: { name: "Kimi" },
  minimax: { name: "MiniMax" },
  qianwen: { name: "千问AI平台" },
  qwencloud: { name: "QwenCloud" },
  siliconflow: { name: "SiliconFlow" },
  stepfun: { name: "StepFun" },
  sudocode: { name: "SudoCode" },
  tencent: { name: "Tencent Token Plan" },
  volcengine: {
    name: "Volcengine",
    nameKey: "providerPreset.family.volcengine",
  },
  "xiaomi-mimo": { name: "Xiaomi MiMo" },
  zhipu: { name: "Zhipu GLM" },
} as const satisfies Record<string, PresetFamilyInfo>;

export type PresetFamilyId = keyof typeof PRESET_FAMILIES;

/** 版本标签，显示为 `providerPreset.version.<key>` */
export const PRESET_VERSION_KEYS = [
  "payg",
  "paygCn",
  "paygIntl",
  "coding",
  "codingCn",
  "codingIntl",
  "cn",
  "intl",
  "agentPlan",
  "codingPlan",
  "tokenPlan",
  "tokenPlanCn",
  "tokenPlanIntl",
  "enterpriseLiteCn",
  "enterpriseLiteIntl",
  "enterpriseProCn",
  "enterpriseProIntl",
  "stepPlanCn",
  "stepPlanIntl",
  "aksk",
  "apiKey",
] as const;

export type PresetVersionKey = (typeof PRESET_VERSION_KEYS)[number];

/** 各应用预设接口共用的两个字段 */
export interface PresetFamilyFields {
  /** 厂商（同一家的多个版本合成一行），见 PRESET_FAMILIES */
  family?: PresetFamilyId;
  /** 版本标签，见 PRESET_VERSION_KEYS；缺省时用域名 */
  versionKey?: PresetVersionKey;
}
