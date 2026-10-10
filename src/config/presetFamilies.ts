/**
 * 预设的「厂商 / 版本」：同一家的国内站、海外站、各种套餐在添加供应商第 1 步合成一行，
 * 第 2 步的预设条里再选版本。只是显示用的分组，不影响请求和 `category`。
 *
 * 写法：预设上加 `family`（这里的 id），再按版本的实际差别写两个维度：
 * `planKey`（套餐，见 PRESET_PLAN_KEYS）和 `regionKey`（地区，见 PRESET_REGION_KEYS）。
 * 只在一个维度上有差别的只写那一个（例如智谱只有国内 / 海外，火山只有不同套餐）。
 * 只给在同一个应用里真有多个版本的厂商打；某个应用里只剩一个版本时不打，照旧单独一行。
 * 两个都没写的版本用它的域名当标签（例如 SudoCode 的 sudocode.chat / sudocode.us）。
 */
export interface PresetFamilyInfo {
  /** 合成那一行的名称 */
  name: string;
  /** 名称需要翻译时用的 i18n key */
  nameKey?: string;
  /** 套餐的显示顺序（设计稿顺序）；不写或没列到的套餐按预设文件里第一次出现的顺序排在后面 */
  planOrder?: readonly PresetPlanKey[];
}

export const PRESET_FAMILIES = {
  "aws-bedrock": { name: "AWS Bedrock" },
  "baidu-qianfan": { name: "Baidu Qianfan" },
  compshare: { name: "Compshare", nameKey: "providerForm.presets.ucloud" },
  kimi: {
    name: "Kimi",
    planOrder: ["payg", "coding"],
  },
  minimax: { name: "MiniMax" },
  // Go 是编程订阅、Zen 是按量付费，同一个工作区 Key
  opencode: {
    name: "OpenCode",
    planOrder: ["coding", "payg"],
  },
  qianwen: { name: "千问AI平台" },
  qwencloud: { name: "QwenCloud" },
  siliconflow: { name: "SiliconFlow" },
  stepfun: { name: "StepFun" },
  sudocode: { name: "SudoCode" },
  tencent: {
    name: "Tencent Cloud",
    nameKey: "providerPreset.family.tencent",
    planOrder: ["tokenPlan", "enterpriseLite", "enterprisePro"],
  },
  volcengine: {
    name: "Volcengine",
    nameKey: "providerPreset.family.volcengine",
  },
  "xiaomi-mimo": { name: "Xiaomi MiMo" },
  zhipu: { name: "Zhipu GLM" },
} as const satisfies Record<string, PresetFamilyInfo>;

export type PresetFamilyId = keyof typeof PRESET_FAMILIES;

/**
 * 套餐，显示为 `providerPreset.plan.<key>`。AWS Bedrock 的 AKSK / API Key 是认证方式，
 * 也放在这一维（它只有这一维，界面上照旧叫「版本」）。
 */
export const PRESET_PLAN_KEYS = [
  "payg",
  "coding",
  "codingPlan",
  "agentPlan",
  "tokenPlan",
  "enterpriseLite",
  "enterprisePro",
  "stepPlan",
  "aksk",
  "apiKey",
] as const;

export type PresetPlanKey = (typeof PRESET_PLAN_KEYS)[number];

/** 地区，显示为 `providerPreset.region.<key>`；数组顺序就是显示顺序（先国内后海外） */
export const PRESET_REGION_KEYS = ["cn", "intl"] as const;

export type PresetRegionKey = (typeof PRESET_REGION_KEYS)[number];

/** 各应用预设接口共用的字段 */
export interface PresetFamilyFields {
  /** 厂商（同一家的多个版本合成一行），见 PRESET_FAMILIES */
  family?: PresetFamilyId;
  /** 套餐，见 PRESET_PLAN_KEYS */
  planKey?: PresetPlanKey;
  /** 地区，见 PRESET_REGION_KEYS */
  regionKey?: PresetRegionKey;
}
