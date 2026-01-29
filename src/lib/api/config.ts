// 配置相关 API
import { invoke } from "@tauri-apps/api/core";
import type { Provider } from "@/types";
import { providersApi } from "./providers";
import {
  hasCommonConfigSnippet,
  hasTomlCommonConfigSnippet,
  hasGeminiCommonConfigSnippet,
} from "@/utils/providerConfigUtils";

export type AppType = "claude" | "codex" | "gemini";

// ============================================================================
// 同步防抖管理
// ============================================================================

/** 同步结果类型 */
export interface SyncResult {
  /** 成功更新的供应商数量 */
  count: number;
  /** 错误信息（如果有） */
  error?: string;
  /** 是否已排队等待执行（debounce 中） */
  queued?: boolean;
}

/** 同步结果回调类型 */
export type SyncResultCallback = (result: SyncResult) => void;

// 每个 appType 的 debounce 定时器
const syncDebounceTimers: Record<
  AppType,
  ReturnType<typeof setTimeout> | null
> = {
  claude: null,
  codex: null,
  gemini: null,
};

// 每个 appType 的同步锁（防止并发）
const syncInFlight: Record<AppType, boolean> = {
  claude: false,
  codex: false,
  gemini: false,
};

// 每个 appType 的最新同步参数（用于 single-flight）
const pendingSyncParams: Record<
  AppType,
  {
    oldSnippet: string;
    newSnippet: string;
    updateFn: (
      settingsConfig: string,
      oldSnippet: string,
      newSnippet: string,
    ) => { updatedConfig: string; error?: string };
    currentProviderId?: string;
    onComplete?: SyncResultCallback;
  } | null
> = {
  claude: null,
  codex: null,
  gemini: null,
};

const SYNC_DEBOUNCE_MS = 500;

/**
 * 获取通用配置片段（统一接口）
 * @param appType - 应用类型（claude/codex/gemini）
 * @returns 通用配置片段（原始字符串），如果不存在则返回 null
 */
export async function getCommonConfigSnippet(
  appType: AppType,
): Promise<string | null> {
  return invoke<string | null>("get_common_config_snippet", { appType });
}

/**
 * 设置通用配置片段（统一接口）
 * @param appType - 应用类型（claude/codex/gemini）
 * @param snippet - 通用配置片段（原始字符串）
 * @throws 如果格式无效（Claude/Gemini 验证 JSON，Codex 暂不验证）
 */
export async function setCommonConfigSnippet(
  appType: AppType,
  snippet: string,
): Promise<void> {
  return invoke("set_common_config_snippet", { appType, snippet });
}

/**
 * 提取通用配置片段
 *
 * 默认读取当前激活供应商的配置；若传入 `options.settingsConfig`，则从编辑器当前内容提取。
 * 会自动排除差异化字段（API Key、模型配置、端点等），返回可复用的通用配置片段。
 *
 * @param appType - 应用类型（claude/codex/gemini）
 * @param options - 可选：提取来源
 * @returns 提取的通用配置片段（JSON/TOML 字符串）
 */
export type ExtractCommonConfigSnippetOptions = {
  settingsConfig?: string;
};

export async function extractCommonConfigSnippet(
  appType: AppType,
  options?: ExtractCommonConfigSnippetOptions,
): Promise<string> {
  const args: Record<string, unknown> = { appType };
  const settingsConfig = options?.settingsConfig;

  if (typeof settingsConfig === "string" && settingsConfig.trim()) {
    args.settingsConfig = settingsConfig;
  }

  return invoke<string>("extract_common_config_snippet", args);
}

// ============================================================================
// 通用配置同步功能
// ============================================================================

/**
 * 同步通用配置到所有启用了该配置的供应商（带 debounce + 同步锁）
 *
 * 当通用配置片段被修改时，需要更新所有启用了通用配置的供应商的 settingsConfig
 * 使用 debounce + single-flight + in-flight lock 模式，避免快速编辑时的并发问题
 *
 * @param appType - 应用类型
 * @param oldSnippet - 旧的通用配置片段
 * @param newSnippet - 新的通用配置片段（空字符串表示清空/移除）
 * @param updateFn - 更新函数，用于替换配置中的通用配置片段
 * @param currentProviderId - 当前正在编辑的供应商 ID（跳过，因为已在编辑器中更新）
 * @param onComplete - 同步完成后的回调，用于通知 UI 层
 */
export function syncCommonConfigToProviders(
  appType: AppType,
  oldSnippet: string,
  newSnippet: string,
  updateFn: (
    settingsConfig: string,
    oldSnippet: string,
    newSnippet: string,
  ) => { updatedConfig: string; error?: string },
  currentProviderId?: string,
  onComplete?: SyncResultCallback,
): void {
  // 保存最新的同步参数（覆盖之前的，实现 single-flight）
  pendingSyncParams[appType] = {
    oldSnippet,
    newSnippet,
    updateFn,
    currentProviderId,
    onComplete,
  };

  // 清除之前的定时器
  if (syncDebounceTimers[appType]) {
    clearTimeout(syncDebounceTimers[appType]!);
  }

  // 设置新的定时器，在 debounce 后执行实际同步
  syncDebounceTimers[appType] = setTimeout(() => {
    executeSyncWithLock(appType);
  }, SYNC_DEBOUNCE_MS);
}

/**
 * 带锁执行同步（防止并发）
 */
async function executeSyncWithLock(appType: AppType): Promise<void> {
  // 如果正在同步中，等待下一次调度
  if (syncInFlight[appType]) {
    // 已有同步在执行，参数已保存，等同步完成后会检查是否需要再次执行
    return;
  }

  // 获取最新的参数
  const params = pendingSyncParams[appType];
  if (!params) {
    return;
  }

  // 清除参数并设置锁
  pendingSyncParams[appType] = null;
  syncDebounceTimers[appType] = null;
  syncInFlight[appType] = true;

  try {
    // 执行实际同步
    const result = await doSyncCommonConfigToProviders(
      appType,
      params.oldSnippet,
      params.newSnippet,
      params.updateFn,
      params.currentProviderId,
    );

    // 输出结果
    if (result.error) {
      console.warn(`[syncCommonConfig] ${appType} 同步失败: ${result.error}`);
    } else if (result.count > 0) {
      console.log(
        `[syncCommonConfig] 共更新 ${result.count} 个 ${appType} 供应商`,
      );
    }

    // 通知回调
    if (params.onComplete) {
      params.onComplete(result);
    }
  } finally {
    // 释放锁
    syncInFlight[appType] = false;

    // 检查是否有新的待执行参数（在同步期间又有新的请求）
    if (pendingSyncParams[appType]) {
      // 递归执行，处理新的请求
      executeSyncWithLock(appType);
    }
  }
}

/**
 * 实际执行同步的内部函数
 * @returns 同步结果，包含更新数量和可能的错误信息
 */
async function doSyncCommonConfigToProviders(
  appType: AppType,
  oldSnippet: string,
  newSnippet: string,
  updateFn: (
    settingsConfig: string,
    oldSnippet: string,
    newSnippet: string,
  ) => { updatedConfig: string; error?: string },
  currentProviderId?: string,
): Promise<SyncResult> {
  try {
    const providers = await providersApi.getAll(appType);
    let updatedCount = 0;
    const errors: string[] = [];

    for (const [id, provider] of Object.entries(providers)) {
      // 跳过当前正在编辑的供应商
      if (id === currentProviderId) {
        continue;
      }

      // 获取当前配置字符串（提前获取，用于内容检测）
      const settingsConfigStr = getSettingsConfigString(provider, appType);
      if (!settingsConfigStr) {
        continue;
      }

      // 检查是否启用了通用配置
      const metaByApp = provider.meta?.commonConfigEnabledByApp;
      const resolvedMetaEnabled =
        metaByApp?.[appType] ?? provider.meta?.commonConfigEnabled;

      let isEnabled: boolean;
      let needsMetaBackfill = false;

      if (resolvedMetaEnabled !== undefined) {
        // meta 有明确值，直接使用
        isEnabled = resolvedMetaEnabled;
      } else {
        // meta 缺失，回退到内容检测（旧/新片段均可触发）
        isEnabled = detectCommonConfigEnabledByContent(
          appType,
          settingsConfigStr,
          oldSnippet,
          newSnippet,
        );
        // 如果检测到启用，标记需要补写 meta
        if (isEnabled) {
          needsMetaBackfill = true;
        }
      }

      if (!isEnabled) {
        continue;
      }

      // 使用更新函数替换配置
      const { updatedConfig, error } = updateFn(
        settingsConfigStr,
        oldSnippet,
        newSnippet,
      );

      if (error) {
        errors.push(`供应商 ${id}: ${error}`);
        continue;
      }

      // 更新供应商配置
      const updateResult = updateProviderSettingsConfig(
        provider,
        updatedConfig,
        appType,
      );

      if (updateResult.error) {
        errors.push(`供应商 ${id}: ${updateResult.error}`);
        continue;
      }

      // 如果需要补写 meta，添加 commonConfigEnabledByApp
      let providerToSave = updateResult.provider;
      if (needsMetaBackfill) {
        providerToSave = {
          ...providerToSave,
          meta: {
            ...providerToSave.meta,
            commonConfigEnabledByApp: {
              ...providerToSave.meta?.commonConfigEnabledByApp,
              [appType]: true,
            },
          },
        };
        console.log(
          `[syncCommonConfig] 供应商 ${id} 检测到通用配置，已补写 meta`,
        );
      }

      try {
        await providersApi.update(providerToSave, appType);
        updatedCount++;
        console.log(`[syncCommonConfig] 已更新供应商 ${id}`);
      } catch (updateError) {
        errors.push(`供应商 ${id}: 保存失败 - ${String(updateError)}`);
      }
    }

    if (errors.length > 0) {
      return {
        count: updatedCount,
        error: `部分供应商更新失败: ${errors.join("; ")}`,
      };
    }

    return { count: updatedCount };
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : String(error);
    return {
      count: 0,
      error: `同步失败: ${errorMessage}`,
    };
  }
}

function detectCommonConfigEnabledByContent(
  appType: AppType,
  settingsConfigStr: string,
  oldSnippet: string,
  newSnippet: string,
): boolean {
  const candidates = [oldSnippet, newSnippet].filter(
    (snippet) => typeof snippet === "string" && snippet.trim(),
  );
  if (candidates.length === 0) return false;

  switch (appType) {
    case "codex":
      return candidates.some((snippet) =>
        hasTomlCommonConfigSnippet(settingsConfigStr, snippet),
      );
    case "gemini":
      return candidates.some((snippet) =>
        hasGeminiCommonConfigSnippet(settingsConfigStr, snippet),
      );
    case "claude":
      return candidates.some((snippet) =>
        hasCommonConfigSnippet(settingsConfigStr, snippet),
      );
    default:
      return false;
  }
}

/** 更新供应商配置的结果 */
interface UpdateProviderResult {
  provider: Provider;
  error?: string;
}

/**
 * 从供应商获取配置字符串
 */
function getSettingsConfigString(
  provider: Provider,
  appType: AppType,
): string | null {
  const config = provider.settingsConfig;
  if (!config) return null;

  switch (appType) {
    case "claude":
      // Claude: settingsConfig 直接是 JSON 对象
      return typeof config === "string" ? config : JSON.stringify(config);

    case "codex":
      // Codex: settingsConfig.config 是 TOML 字符串
      // 先校验 settingsConfig 是对象类型
      if (typeof config === "string") {
        // settingsConfig 是字符串（可能是之前保存失败的情况）
        // 尝试解析为 JSON
        try {
          const parsed = JSON.parse(config);
          if (
            typeof parsed === "object" &&
            parsed !== null &&
            typeof parsed.config === "string"
          ) {
            return parsed.config;
          }
        } catch {
          console.warn(
            `[getSettingsConfigString] Codex provider settingsConfig 是无效字符串，跳过`,
          );
        }
        return null;
      }
      if (typeof config === "object" && config !== null) {
        const codexConfig = (config as Record<string, unknown>).config;
        return typeof codexConfig === "string" ? codexConfig : null;
      }
      return null;

    case "gemini":
      // Gemini: settingsConfig 是包含 env 字段的 JSON 对象
      return typeof config === "string" ? config : JSON.stringify(config);

    default:
      return null;
  }
}

/**
 * 更新供应商的 settingsConfig
 * @returns 更新结果，包含更新后的 provider 和可能的错误信息
 */
function updateProviderSettingsConfig(
  provider: Provider,
  updatedConfig: string,
  appType: AppType,
): UpdateProviderResult {
  switch (appType) {
    case "claude":
      // Claude: 直接替换 settingsConfig
      try {
        const parsed = JSON.parse(updatedConfig);
        if (typeof parsed !== "object" || parsed === null) {
          return {
            provider,
            error: "解析后的配置不是有效的 JSON 对象",
          };
        }
        return {
          provider: {
            ...provider,
            settingsConfig: parsed,
          },
        };
      } catch (e) {
        return {
          provider,
          error: `JSON 解析失败: ${e instanceof Error ? e.message : String(e)}`,
        };
      }

    case "codex": {
      // Codex: 更新 settingsConfig.config
      // 先确保 settingsConfig 是对象类型
      let baseConfig: Record<string, unknown>;
      const currentConfig = provider.settingsConfig;

      if (typeof currentConfig === "string") {
        // 尝试解析字符串
        try {
          const parsed = JSON.parse(currentConfig);
          baseConfig =
            typeof parsed === "object" && parsed !== null ? parsed : {};
        } catch {
          baseConfig = {};
        }
      } else if (typeof currentConfig === "object" && currentConfig !== null) {
        baseConfig = currentConfig as Record<string, unknown>;
      } else {
        baseConfig = {};
      }

      return {
        provider: {
          ...provider,
          settingsConfig: {
            ...baseConfig,
            config: updatedConfig,
          },
        },
      };
    }

    case "gemini":
      // Gemini: 直接替换 settingsConfig
      try {
        const parsed = JSON.parse(updatedConfig);
        if (typeof parsed !== "object" || parsed === null) {
          return {
            provider,
            error: "解析后的配置不是有效的 JSON 对象",
          };
        }
        return {
          provider: {
            ...provider,
            settingsConfig: parsed,
          },
        };
      } catch (e) {
        return {
          provider,
          error: `JSON 解析失败: ${e instanceof Error ? e.message : String(e)}`,
        };
      }

    default:
      return { provider };
  }
}
