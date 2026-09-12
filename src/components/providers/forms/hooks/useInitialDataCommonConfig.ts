import { useEffect, useState } from "react";
import { configApi, type AppId } from "@/lib/api";
import { updateCommonConfigSnippet } from "@/utils/providerConfigUtils";
import { isForbiddenCommonEnvKey } from "./useGeminiCommonConfig";

/**
 * 编辑供应商时，把通用配置片段合并进 `initialData.settingsConfig`。
 *
 * 后端存储的供应商快照按设计不含通用配置片段，`meta.commonConfigEnabled` 是
 * 单一真相。编辑界面要展示"合并后"的配置，合并动作放在**数据层**——表单收到的
 * `initialData` 已是最终形态，`form.reset` / `codexConfig` / `env` 一步到位。
 *
 * 这样表单内的通用配置 hook 就不需要观察编辑器的中间值去猜测"何时被重置、
 * 何时该把片段补回去"，勾选状态与编辑器内容天然一致。
 */

/** 只有这三种应用有通用配置片段 */
type CommonConfigAppId = "claude" | "codex" | "gemini";

const isPlainObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const LEGACY_STORAGE_KEYS: Record<CommonConfigAppId, string> = {
  claude: "cc-switch:common-config-snippet",
  codex: "cc-switch:codex-common-config-snippet",
  gemini: "cc-switch:gemini-common-config-snippet",
};

/**
 * 读取片段。config.json 为空时回退到 localStorage 遗留值（只读不清理——
 * 迁移与清理由表单内的通用配置 hook 负责，这里只保证合并预览不落空）。
 */
async function loadSnippet(appId: CommonConfigAppId): Promise<string> {
  let snippet = "";
  try {
    snippet = (await configApi.getCommonConfigSnippet(appId)) ?? "";
  } catch (error) {
    console.error("加载通用配置失败:", error);
  }
  if (snippet.trim()) return snippet;

  const legacyKey = LEGACY_STORAGE_KEYS[appId];
  if (typeof window === "undefined") return "";
  try {
    return window.localStorage.getItem(legacyKey) ?? "";
  } catch {
    return "";
  }
}

interface UseInitialDataCommonConfigProps {
  appId: AppId;
  /** 供应商的 `meta.commonConfigEnabled`；非 true 时不做任何合并 */
  commonConfigEnabled?: boolean;
  /** 已经过 live/DB 协调的配置快照 */
  settingsConfig: Record<string, unknown>;
  /** 关闭时跳过（编辑面板未打开） */
  enabled?: boolean;
}

/** 合并结果连同它的来源快照一起存，用来判断结果是否仍对应当前入参 */
interface MergeResult {
  source: Record<string, unknown>;
  value: Record<string, unknown>;
}

export function useInitialDataCommonConfig({
  appId,
  commonConfigEnabled,
  settingsConfig,
  enabled = true,
}: UseInitialDataCommonConfigProps): Record<string, unknown> {
  const [merged, setMerged] = useState<MergeResult | null>(null);

  const mergeAppId: CommonConfigAppId | null =
    appId === "claude" || appId === "codex" || appId === "gemini"
      ? appId
      : null;

  useEffect(() => {
    if (!enabled || commonConfigEnabled !== true || !mergeAppId) {
      setMerged(null);
      return;
    }

    let cancelled = false;

    const run = async () => {
      const snippet = await loadSnippet(mergeAppId);
      if (cancelled) return;

      let next = settingsConfig;
      if (snippet.trim()) {
        if (mergeAppId === "claude") {
          next = mergeClaudeSnippet(settingsConfig, snippet);
        } else if (mergeAppId === "gemini") {
          next = mergeGeminiSnippet(settingsConfig, snippet);
        } else {
          next = await mergeCodexSnippet(settingsConfig, snippet);
        }
      }
      if (cancelled) return;

      // 未发生实际改动时保持入参引用，避免下游多一次无意义的表单重置
      setMerged(
        next === settingsConfig
          ? null
          : { source: settingsConfig, value: next },
      );
    };

    void run();

    return () => {
      cancelled = true;
    };
  }, [enabled, commonConfigEnabled, mergeAppId, settingsConfig]);

  // 合并结果与当前入参对不上（入参已变、合并还没跑完）时交出入参本身：
  // 宁可短暂少显示片段，也不要把上一个快照的内容当成当前值渲染出去。
  return merged?.source === settingsConfig ? merged.value : settingsConfig;
}

/** Claude：深合并进快照，失败时原样返回（同一引用表示"未改动"）。 */
function mergeClaudeSnippet(
  settings: Record<string, unknown>,
  snippet: string,
): Record<string, unknown> {
  const { updatedConfig, error } = updateCommonConfigSnippet(
    JSON.stringify(settings, null, 2),
    snippet,
    true,
  );
  if (error) return settings;

  try {
    const parsed = JSON.parse(updatedConfig);
    return isPlainObject(parsed) ? parsed : settings;
  } catch {
    return settings;
  }
}

/**
 * Gemini：合并进 settingsConfig.env。
 *
 * 禁键判定与 useGeminiCommonConfig 共用同一个实现——片段会被合并进**其它**
 * 供应商的 env，凭据绝不能进来，两处判定必须一致。片段只要有一个键不合规就
 * 整体不应用，与编辑界面的行为保持一致。
 */
function mergeGeminiSnippet(
  settings: Record<string, unknown>,
  snippet: string,
): Record<string, unknown> {
  let parsed: unknown;
  try {
    parsed = JSON.parse(snippet.trim());
  } catch {
    return settings;
  }
  if (!isPlainObject(parsed)) return settings;

  const snippetEnv: Record<string, string> = {};
  for (const [key, value] of Object.entries(parsed)) {
    if (isForbiddenCommonEnvKey(key)) return settings;
    if (typeof value !== "string") return settings;
    const normalized = value.trim();
    if (normalized) snippetEnv[key] = normalized;
  }
  if (Object.keys(snippetEnv).length === 0) return settings;

  const currentEnv = isPlainObject(settings.env) ? settings.env : {};
  return { ...settings, env: { ...currentEnv, ...snippetEnv } };
}

async function mergeCodexSnippet(
  settings: Record<string, unknown>,
  snippet: string,
): Promise<Record<string, unknown>> {
  const config = typeof settings.config === "string" ? settings.config : "";

  try {
    const updated = await configApi.updateTomlCommonConfigSnippet(
      config,
      snippet,
      true,
    );
    if (typeof updated !== "string" || updated === config) return settings;
    return { ...settings, config: updated };
  } catch (error) {
    console.error("合并 Codex 通用配置失败:", error);
    return settings;
  }
}
