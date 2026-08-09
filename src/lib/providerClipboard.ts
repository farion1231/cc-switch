import { copyText, readText } from "@/lib/clipboard";
import type { AppId } from "@/lib/api";
import type { Provider, UniversalProvider, UsageScript } from "@/types";

/**
 * 剪贴板可移植配置信封。
 *
 * 作用：把任意客户端的 Provider、统一供应商（UniversalProvider）或用量查询脚本
 * （UsageScript）序列化为带识别头的 JSON 文本，通过系统剪贴板在应用间 / 机器间传递，
 * 导入时再按 kind 还原。
 */

/** 信封识别标记，避免误把任意剪贴板 JSON 当作配置导入 */
export const CC_SWITCH_CLIPBOARD_MAGIC = "$ccSwitch" as const;

/** 当前信封版本 */
export const CC_SWITCH_CLIPBOARD_VERSION = 1;

export type ClipboardKind = "provider" | "universal-provider" | "usage-script";

/** 客户端供应商 / 统一供应商信封 */
export interface ProviderClipboardEnvelope {
  $ccSwitch: true;
  version: 1;
  kind: "provider" | "universal-provider";
  /** kind === "provider" 时标注来源客户端 */
  appType?: AppId;
  provider: Provider | UniversalProvider;
}

/** 用量查询脚本信封 */
export interface UsageScriptClipboardEnvelope {
  $ccSwitch: true;
  version: 1;
  kind: "usage-script";
  script: UsageScript;
  /** 来源客户端与供应商名（仅用于展示，导入时忽略） */
  appType?: AppId;
  providerName?: string;
}

export type AnyClipboardEnvelope =
  | ProviderClipboardEnvelope
  | UsageScriptClipboardEnvelope;

// ─── 导出 ────────────────────────────────────────────────────

/** 导出客户端供应商配置到剪贴板（完整携带 meta / 用量脚本 / 图标等） */
export async function exportProviderToClipboard(
  appType: AppId,
  provider: Provider,
): Promise<void> {
  const envelope: ProviderClipboardEnvelope = {
    $ccSwitch: true,
    version: CC_SWITCH_CLIPBOARD_VERSION,
    kind: "provider",
    appType,
    provider,
  };
  await copyText(JSON.stringify(envelope));
}

/** 导出统一供应商到剪贴板 */
export async function exportUniversalProviderToClipboard(
  provider: UniversalProvider,
): Promise<void> {
  const envelope: ProviderClipboardEnvelope = {
    $ccSwitch: true,
    version: CC_SWITCH_CLIPBOARD_VERSION,
    kind: "universal-provider",
    provider,
  };
  await copyText(JSON.stringify(envelope));
}

/** 导出用量查询脚本到剪贴板 */
export async function exportUsageScriptToClipboard(
  script: UsageScript,
  context?: { appType?: AppId; providerName?: string },
): Promise<void> {
  const envelope: UsageScriptClipboardEnvelope = {
    $ccSwitch: true,
    version: CC_SWITCH_CLIPBOARD_VERSION,
    kind: "usage-script",
    script,
    appType: context?.appType,
    providerName: context?.providerName,
  };
  await copyText(JSON.stringify(envelope));
}

// ─── 导入 ────────────────────────────────────────────────────

/**
 * 从剪贴板读取并解析配置信封（通用入口）。
 *
 * @returns 识别成功时返回信封；剪贴板为空或非 cc-switch 信封时返回 `null`。
 */
export async function importConfigFromClipboard(): Promise<AnyClipboardEnvelope | null> {
  let raw = "";
  try {
    raw = await readText();
  } catch {
    return null;
  }
  const text = raw.trim();
  if (!text) return null;

  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return null;
  }

  return normalizeEnvelope(parsed);
}

/** 导入客户端 / 统一供应商信封（usage-script 信封返回 null） */
export async function importProviderFromClipboard(): Promise<ProviderClipboardEnvelope | null> {
  const env = await importConfigFromClipboard();
  if (env && (env.kind === "provider" || env.kind === "universal-provider")) {
    return env;
  }
  return null;
}

/** 导入用量查询脚本信封（provider 信封返回 null） */
export async function importUsageScriptFromClipboard(): Promise<UsageScriptClipboardEnvelope | null> {
  const env = await importConfigFromClipboard();
  if (env && env.kind === "usage-script") {
    return env;
  }
  return null;
}

function normalizeEnvelope(value: unknown): AnyClipboardEnvelope | null {
  if (!value || typeof value !== "object") return null;
  const obj = value as Record<string, unknown>;
  if (obj[CC_SWITCH_CLIPBOARD_MAGIC] !== true) return null;
  if (obj.version !== CC_SWITCH_CLIPBOARD_VERSION) return null;

  const kind = obj.kind;
  if (kind === "provider" || kind === "universal-provider") {
    const provider = obj.provider;
    if (!provider || typeof provider !== "object") return null;
    const envelope: ProviderClipboardEnvelope = {
      $ccSwitch: true,
      version: CC_SWITCH_CLIPBOARD_VERSION,
      kind,
      provider: provider as Provider | UniversalProvider,
    };
    if (kind === "provider" && typeof obj.appType === "string") {
      envelope.appType = obj.appType as AppId;
    }
    return envelope;
  }

  if (kind === "usage-script") {
    const script = obj.script;
    if (!script || typeof script !== "object") return null;
    const envelope: UsageScriptClipboardEnvelope = {
      $ccSwitch: true,
      version: CC_SWITCH_CLIPBOARD_VERSION,
      kind: "usage-script",
      script: script as UsageScript,
    };
    if (typeof obj.appType === "string")
      envelope.appType = obj.appType as AppId;
    if (typeof obj.providerName === "string") {
      envelope.providerName = obj.providerName;
    }
    return envelope;
  }

  return null;
}
