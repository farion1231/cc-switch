import { useState, useCallback, useRef, useEffect } from "react";
import {
  extractCodexBaseUrl,
  setCodexBaseUrl as setCodexBaseUrlInConfig,
} from "@/utils/providerConfigUtils";
import type { ProviderCategory } from "@/types";
import type { AppId } from "@/lib/api";

interface UseBaseUrlStateProps {
  appType: AppId;
  category: ProviderCategory | undefined;
  settingsConfig: string;
  codexConfig?: string;
  onSettingsConfigChange: (config: string) => void;
  onCodexConfigChange?: (config: string) => void;
  /**
   * 现读当前配置。`settingsConfig` 是调用方在渲染期取的快照，而写入
   * settingsConfig 不会触发重渲染，所以别处（JSON 编辑器、通用配置片段合并）
   * 写进去的改动不会反映到那份快照里。改 Base URL 时若基于旧快照重建整份
   * JSON，就会把那些改动覆盖回去。传入本 getter 即可在事件发生时现读。
   */
  getSettingsConfig?: () => string;
}

/**
 * 管理 Base URL 状态
 * 支持 Claude (JSON) 和 Codex (TOML) 两种格式
 */
export function useBaseUrlState({
  appType,
  category,
  settingsConfig,
  codexConfig,
  onSettingsConfigChange,
  onCodexConfigChange,
  getSettingsConfig,
}: UseBaseUrlStateProps) {
  const [baseUrl, setBaseUrl] = useState("");
  const [codexBaseUrl, setCodexBaseUrl] = useState("");
  const [geminiBaseUrl, setGeminiBaseUrl] = useState("");
  const isUpdatingRef = useRef(false);

  // 事件处理里一律走这里取配置：有 getter 用 getter（当前值），否则退回快照。
  const readSettingsConfig = useCallback(
    () => getSettingsConfig?.() ?? settingsConfig,
    [getSettingsConfig, settingsConfig],
  );

  // 从配置同步到 state（Claude / Claude Desktop）
  useEffect(() => {
    if (appType !== "claude" && appType !== "claude-desktop") return;
    // 只有 official 类别不显示 Base URL 输入框，其他类别都需要回填
    if (category === "official") return;
    if (isUpdatingRef.current) return;

    try {
      const config = JSON.parse(settingsConfig || "{}");
      const envUrl: unknown = config?.env?.ANTHROPIC_BASE_URL;
      const nextUrl = typeof envUrl === "string" ? envUrl.trim() : "";
      if (nextUrl !== baseUrl) {
        setBaseUrl(nextUrl);
      }
    } catch {
      // ignore
    }
  }, [appType, category, settingsConfig, baseUrl]);

  // 从配置同步到 state（Codex）
  useEffect(() => {
    if (appType !== "codex") return;
    // 只有 official 类别不显示 Base URL 输入框，其他类别都需要回填
    if (category === "official") return;
    if (isUpdatingRef.current) return;
    if (!codexConfig) return;

    const extracted = extractCodexBaseUrl(codexConfig) || "";
    setCodexBaseUrl((prev) => (prev === extracted ? prev : extracted));
  }, [appType, category, codexConfig]);

  // 从Claude配置同步到 state（Gemini）
  useEffect(() => {
    if (appType !== "gemini") return;
    // 只有 official 类别不显示 Base URL 输入框，其他类别都需要回填
    if (category === "official") return;
    if (isUpdatingRef.current) return;

    try {
      const config = JSON.parse(settingsConfig || "{}");
      const envUrl: unknown = config?.env?.GOOGLE_GEMINI_BASE_URL;
      const nextUrl = typeof envUrl === "string" ? envUrl.trim() : "";
      if (nextUrl !== geminiBaseUrl) {
        setGeminiBaseUrl(nextUrl);
        setBaseUrl(nextUrl); // 也更新 baseUrl 用于 UI
      }
    } catch {
      // ignore
    }
  }, [appType, category, settingsConfig, geminiBaseUrl]);

  // 处理 Claude Base URL 变化
  const handleClaudeBaseUrlChange = useCallback(
    (url: string) => {
      const sanitized = url.trim();
      setBaseUrl(sanitized);
      isUpdatingRef.current = true;

      try {
        const config = JSON.parse(readSettingsConfig() || "{}");
        if (!config.env) {
          config.env = {};
        }
        config.env.ANTHROPIC_BASE_URL = sanitized;
        onSettingsConfigChange(JSON.stringify(config, null, 2));
      } catch {
        // ignore
      } finally {
        setTimeout(() => {
          isUpdatingRef.current = false;
        }, 0);
      }
    },
    [readSettingsConfig, onSettingsConfigChange],
  );

  // 处理 Codex Base URL 变化
  const handleCodexBaseUrlChange = useCallback(
    (url: string) => {
      const sanitized = url.trim();
      setCodexBaseUrl(sanitized);

      if (!onCodexConfigChange) {
        return;
      }

      isUpdatingRef.current = true;
      const updatedConfig = setCodexBaseUrlInConfig(
        codexConfig || "",
        sanitized,
      );
      onCodexConfigChange(updatedConfig);

      setTimeout(() => {
        isUpdatingRef.current = false;
      }, 0);
    },
    [codexConfig, onCodexConfigChange],
  );

  // 处理 Gemini Base URL 变化
  const handleGeminiBaseUrlChange = useCallback(
    (url: string) => {
      const sanitized = url.trim();
      setGeminiBaseUrl(sanitized);
      setBaseUrl(sanitized); // 也更新 baseUrl 用于 UI
      isUpdatingRef.current = true;

      try {
        const config = JSON.parse(readSettingsConfig() || "{}");
        if (!config.env) {
          config.env = {};
        }
        config.env.GOOGLE_GEMINI_BASE_URL = sanitized;
        onSettingsConfigChange(JSON.stringify(config, null, 2));
      } catch {
        // ignore
      } finally {
        setTimeout(() => {
          isUpdatingRef.current = false;
        }, 0);
      }
    },
    [readSettingsConfig, onSettingsConfigChange],
  );

  return {
    baseUrl,
    setBaseUrl,
    codexBaseUrl,
    setCodexBaseUrl,
    geminiBaseUrl,
    setGeminiBaseUrl,
    handleClaudeBaseUrlChange,
    handleCodexBaseUrlChange,
    handleGeminiBaseUrlChange,
  };
}
