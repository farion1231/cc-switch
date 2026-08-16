import { isPlainObject } from "@/utils/providerConfigUtils";

const CLAUDE_CONTEXT_WINDOW_ENV_KEYS = [
  "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
  "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
] as const;

const CLAUDE_CONTEXT_WINDOW_STATE_KEYS = {
  CLAUDE_CODE_AUTO_COMPACT_WINDOW: "ACW",
  CLAUDE_CODE_MAX_CONTEXT_TOKENS: "MAX",
} as const;

export const AUTO_SYNC_COMPACT_RATIO_MIN = 0.2;
export const AUTO_SYNC_COMPACT_RATIO_MAX = 0.95;

/**
 * autoSyncContextWindow 开关有效值：显式字段优先，缺失即关闭
 * （spec：缺失该开关时默认关闭）。
 */
export function resolveAutoSyncContextWindow(config: string): boolean {
  try {
    const parsed = JSON.parse(config || "{}") as Record<string, unknown>;
    return parsed.autoSyncContextWindow === true;
  } catch {
    return false;
  }
}

function removeClaudeContextWindowEnvFields(config: Record<string, unknown>) {
  const env = config.env;
  if (!env || typeof env !== "object" || Array.isArray(env)) return;

  const envRecord = env as Record<string, unknown>;
  for (const key of CLAUDE_CONTEXT_WINDOW_ENV_KEYS) {
    delete envRecord[key];
  }
}

function contextWindowValueAsString(value: unknown): string | undefined {
  if (typeof value === "string") {
    const trimmed = value.trim();
    return trimmed.length > 0 ? trimmed : undefined;
  }
  if (
    typeof value === "number" &&
    Number.isFinite(value) &&
    value > 0 &&
    Number.isInteger(value)
  ) {
    return String(value);
  }
  return undefined;
}

export function matchesAutoSyncSource(
  state: Record<string, unknown>,
  shortKey: string,
  liveValue: unknown,
): boolean {
  // 与 Rust backfill/live 侧对齐：live 值先规范化成字符串，账本值只认字符串且严格相等；
  // 数字型账本值不匹配（Rust as_str 对数字返回 None）。
  const live = contextWindowValueAsString(liveValue);
  if (live === undefined) return false;
  return ["lastWritten", "staticInjected"].some((sourceName) => {
    const source = state[sourceName];
    if (!isPlainObject(source)) return false;
    const sourceValue = source[shortKey];
    return typeof sourceValue === "string" && sourceValue === live;
  });
}

function restorableLedgerValue(value: unknown): string | undefined {
  if (typeof value === "string") return value;
  if (typeof value === "number" && Number.isFinite(value) && value > 0) {
    return String(value);
  }
  return undefined;
}

export function restoreAutoSyncContextWindowValue(
  state: Record<string, unknown>,
  shortKey: string,
): string | undefined {
  for (const sourceName of ["lastWritten", "staticInjected"] as const) {
    const source = state[sourceName];
    if (!isPlainObject(source)) continue;
    const value = restorableLedgerValue(source[shortKey]);
    if (value !== undefined) return value;
  }
  return undefined;
}

/**
 * 写入自动同步开关状态。
 *
 * 关闭时按 autoSyncState 清理等于 lastWritten/staticInjected 的自动值，
 * 其余 live 值原样保留（不再写入 userExplicit）。
 * 开启时从 lastWritten/staticInjected 恢复；都没有则不写回。
 */
export function applyAutoSyncContextWindowSetting(
  config: string,
  enabled: boolean,
): string {
  try {
    const parsed = JSON.parse(config || "{}") as Record<string, unknown>;
    parsed.autoSyncContextWindow = enabled;
    if (enabled) {
      const state = parsed.autoSyncState;
      if (isPlainObject(state)) {
        const writes: Array<[string, string]> = [];
        for (const envKey of CLAUDE_CONTEXT_WINDOW_ENV_KEYS) {
          const shortKey = CLAUDE_CONTEXT_WINDOW_STATE_KEYS[envKey];
          const value = restoreAutoSyncContextWindowValue(state, shortKey);
          if (value !== undefined) writes.push([envKey, value]);
        }
        if (writes.length > 0) {
          if (!isPlainObject(parsed.env)) parsed.env = {};
          const env = parsed.env as Record<string, unknown>;
          for (const [envKey, value] of writes) env[envKey] = value;
        }
      }
      return JSON.stringify(parsed, null, 2);
    }

    const state = parsed.autoSyncState;
    if (!isPlainObject(state)) {
      removeClaudeContextWindowEnvFields(parsed);
      return JSON.stringify(parsed, null, 2);
    }

    const env = parsed.env;
    if (isPlainObject(env)) {
      for (const envKey of CLAUDE_CONTEXT_WINDOW_ENV_KEYS) {
        const liveValue = env[envKey];
        if (liveValue === undefined) continue;
        const shortKey = CLAUDE_CONTEXT_WINDOW_STATE_KEYS[envKey];
        if (matchesAutoSyncSource(state, shortKey, liveValue)) {
          delete env[envKey];
        }
      }
    }
    return JSON.stringify(parsed, null, 2);
  } catch {
    return config;
  }
}

/**
 * 写入或清空自动压缩比例。
 *
 * null 表示用户留空，不持久化字段，watcher 按 1 处理；
 * 数字必须在 0.2 ~ 0.95 之间，调用方应已校验。
 */
export function applyAutoSyncCompactRatioSetting(
  config: string,
  ratio: number | null,
): string {
  try {
    const parsed = JSON.parse(config || "{}") as Record<string, unknown>;
    if (ratio === null) {
      delete parsed.autoSyncCompactRatio;
    } else {
      parsed.autoSyncCompactRatio = ratio;
    }
    return JSON.stringify(parsed, null, 2);
  } catch {
    return config;
  }
}

/**
 * 合并 settings.json 配置字符串时，保留当前表单中的 autoSyncContextWindow
 * 和 autoSyncCompactRatio。
 *
 * ClaudeFormFields 的开关、比例和模型/API Key/Base URL 等输入都会整包更新
 * settingsConfig。react-hook-form 的 setValue 默认不触发重渲染，后续输入
 * 可能基于旧配置重建 JSON，把刚改的自动同步状态盖掉。这里以当前表单值为准，
 * 只要当前有显式值，就把它强制写回新配置；当前缺失时自动写入不得补写该字段。
 * ACW/MAX 的清理只发生在开关关闭动作中，这里不重复删除。
 */
export function mergeSettingsConfigPreservingAutoSync(
  currentConfig: string,
  nextConfig: string,
): string {
  try {
    const current = JSON.parse(currentConfig || "{}") as Record<
      string,
      unknown
    >;
    const next = JSON.parse(nextConfig || "{}") as Record<string, unknown>;

    if (typeof current.autoSyncContextWindow === "boolean") {
      next.autoSyncContextWindow = current.autoSyncContextWindow;
    } else {
      delete next.autoSyncContextWindow;
    }

    if (
      typeof current.autoSyncCompactRatio === "number" &&
      Number.isFinite(current.autoSyncCompactRatio) &&
      current.autoSyncCompactRatio >= AUTO_SYNC_COMPACT_RATIO_MIN &&
      current.autoSyncCompactRatio <= AUTO_SYNC_COMPACT_RATIO_MAX
    ) {
      next.autoSyncCompactRatio = current.autoSyncCompactRatio;
    } else {
      delete next.autoSyncCompactRatio;
    }

    return JSON.stringify(next, null, 2);
  } catch {
    return nextConfig;
  }
}
