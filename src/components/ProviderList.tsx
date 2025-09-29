import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Provider } from "../types";
import { Play, Edit3, Trash2, CheckCircle2, Users, Check } from "lucide-react";
import { buttonStyles, cardStyles, badgeStyles, cn } from "../lib/styles";
import { AppType } from "../lib/tauri-api";
import {
  applyProviderToVSCode,
  detectApplied,
  normalizeBaseUrl,
} from "../utils/vscodeSettings";
import { getCodexBaseUrl } from "../utils/providerConfigUtils";
import { useVSCodeAutoSync } from "../hooks/useVSCodeAutoSync";
// 不再在列表中显示分类徽章，避免造成困惑

interface ProviderListProps {
  providers: Record<string, Provider>;
  currentProviderId: string;
  onSwitch: (id: string) => void;
  onDelete: (id: string) => void;
  onEdit: (id: string) => void;
  appType?: AppType;
  onNotify?: (
    message: string,
    type: "success" | "error",
    duration?: number,
  ) => void;
}

const ProviderList: React.FC<ProviderListProps> = ({
  providers,
  currentProviderId,
  onSwitch,
  onDelete,
  onEdit,
  appType,
  onNotify,
}) => {
  const { t } = useTranslation();
  // 提取API地址（兼容不同供应商配置：Claude env / Codex TOML）
  const getApiUrl = (provider: Provider): string => {
    try {
      const cfg = provider.settingsConfig;
      // Claude/Anthropic: 从 env 中读取
      if (cfg?.env?.ANTHROPIC_BASE_URL) {
        return cfg.env.ANTHROPIC_BASE_URL;
      }
      // Codex: 从 TOML 配置中解析 base_url
      if (typeof cfg?.config === "string" && cfg.config.includes("base_url")) {
        // 支持单/双引号
        const match = cfg.config.match(/base_url\s*=\s*["']([^"']+)["']/);
        return match?.[1] || "";
      }
      // Codex: 从 config 中提取 base_url（如果直接存在）
      if (appType === "codex") {
        const baseUrl = getCodexBaseUrl(provider);
        if (baseUrl) return baseUrl;
      }
      return "";
    } catch {
      return "";
    }
  };

  // 自动同步 hook
  const { vscodeAppliedFor, handleAutoSync } = useVSCodeAutoSync(
    appType,
    currentProviderId,
    providers,
    onNotify,
  );

  // 监听当前供应商变化，若开启自动同步，自动执行
  useEffect(() => {
    const autoSync = JSON.parse(
      localStorage.getItem("vsCodeAutoSync") || "false",
    );
    if (autoSync) {
      handleAutoSync();
    }
  }, [currentProviderId, handleAutoSync]);

  // 手动检测已应用到 VS Code 的供应商
  useEffect(() => {
    if (appType !== "codex") return;
    // 若已有 applied 状态，不覆盖（比如自动同步设置的）
    if (vscodeAppliedFor) return;

    const checkApplied = async () => {
      try {
        const status = await window.api.getVSCodeSettingsStatus();
        if (!status.exists) return;

        const raw = await window.api.readVSCodeSettings();
        for (const [id, provider] of Object.entries(providers)) {
          const baseUrl = getCodexBaseUrl(provider);
          if (!baseUrl) continue;

          if (detectApplied(raw, baseUrl)) {
            localStorage.setItem("vsCodeAppliedFor", id);
            return;
          }
        }
      } catch (error) {
        console.error("检测 VS Code 设置失败:", error);
      }
    };

    checkApplied();
  }, [appType, currentProviderId, providers]);

  const handleApplyToVSCode = async (provider: Provider) => {
    try {
      const status = await window.api.getVSCodeSettingsStatus();
      if (!status.exists) {
        onNotify?.(t("notifications.vscodeSettingsNotFound"), "error", 3000);
        return;
      }

      const raw = await window.api.readVSCodeSettings();
      const baseUrl = getCodexBaseUrl(provider);
      if (!baseUrl) {
        onNotify?.(t("notifications.vscodeNoBaseUrl"), "error", 3000);
        return;
      }

      const prev = localStorage.getItem("vsCodeAppliedFor");
      if (prev && prev !== provider.id) {
        const prevProvider = providers[prev];
        if (prevProvider) {
          const prevUrl = getCodexBaseUrl(prevProvider);
          if (prevUrl && detectApplied(raw, prevUrl)) {
            onNotify?.(
              t("notifications.vscodeAlreadyApplied", { name: prevProvider.name }),
              "error",
              4000,
            );
            return;
          }
        }
      }

      const settings = applyProviderToVSCode(raw, { baseUrl });
      await window.api.writeVSCodeSettings(settings);

      // 记录已应用的供应商
      localStorage.setItem("vsCodeAppliedFor", provider.id);
      window.dispatchEvent(new Event("vsCodeAppliedForChanged"));
      onNotify?.(t("notifications.vscodeApplySuccess"), "success", 3000);
    } catch (error) {
      console.error("应用到 VS Code 失败:", error);
      onNotify?.(t("notifications.vscodeApplyError", { error: String(error) }), "error", 3000);
    }
  };

  const handleRemoveFromVSCode = async () => {
    try {
      const status = await window.api.getVSCodeSettingsStatus();
      if (!status.exists) {
        onNotify?.(t("notifications.vscodeSettingsNotFound"), "error", 3000);
        return;
      }
      const raw = await window.api.readVSCodeSettings();
      const next = applyProviderToVSCode(raw, {
        baseUrl: undefined,
        key: undefined,
        model: undefined,
      });
      await window.api.writeVSCodeSettings(next);
      localStorage.removeItem("vsCodeAppliedFor");
      window.dispatchEvent(new Event("vsCodeAppliedForChanged"));
      onNotify?.(t("notifications.vscodeRemoveSuccess"), "success", 3000);
    } catch (error) {
      console.error("从 VS Code 移除失败:", error);
      onNotify?.(t("notifications.vscodeRemoveError", { error: String(error) }), "error", 3000);
    }
  };

  const providerEntries = Object.entries(providers);

  if (providerEntries.length === 0) {
    return (
      <div className="text-center py-12">
        <Users className="mx-auto h-12 w-12 text-gray-400 dark:text-gray-600" />
        <p className="mt-4 text-gray-500 dark:text-gray-400">
          {t("provider.noProvidersYet")}
        </p>
        <p className="mt-1 text-sm text-gray-400 dark:text-gray-500">
          {t("provider.clickAddToStart")}
        </p>
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {providerEntries.map(([id, provider]) => {
        const isCurrent = id === currentProviderId;
        const apiUrl = getApiUrl(provider);
        const isOfficial = provider.category === "official";

        return (
          <div
            key={id}
            className={cn(
              cardStyles.base,
              cardStyles.hover,
              isCurrent && "ring-2 ring-blue-500 dark:ring-blue-400",
            )}
          >
            <div className="flex items-center justify-between gap-4">
              <div className="flex items-center gap-3 min-w-0 flex-1">
                {isCurrent && (
                  <CheckCircle2 className="h-5 w-5 text-green-500 dark:text-green-400 flex-shrink-0" />
                )}
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2 mb-1">
                    <h3 className="font-medium text-gray-900 dark:text-gray-100 truncate">
                      {provider.name}
                    </h3>
                    {isOfficial && (
                      <span
                        className={cn(
                          badgeStyles.base,
                          badgeStyles.variants.blue,
                          badgeStyles.size.xs,
                        )}
                      >
                        {t("common.official")}
                      </span>
                    )}
                    {/* VS Code 应用状态指示器 */}
                    {appType === "codex" &&
                      vscodeAppliedFor === provider.id && (
                        <span
                          className={cn(
                            badgeStyles.base,
                            badgeStyles.variants.green,
                            badgeStyles.size.xs,
                            "inline-flex items-center gap-0.5",
                          )}
                        >
                          <Check className="w-3 h-3" />
                          VS Code
                        </span>
                      )}
                  </div>
                  <div className="flex flex-col gap-1">
                    {provider.websiteUrl && (
                      <div className="text-sm text-gray-500 dark:text-gray-400 truncate">
                        <span className="opacity-60">{t("common.website")}: </span>
                        {provider.websiteUrl}
                      </div>
                    )}
                    {apiUrl && (
                      <div className="text-sm text-gray-500 dark:text-gray-400 truncate">
                        <span className="opacity-60">API: </span>
                        {apiUrl}
                      </div>
                    )}
                  </div>
                </div>

                <div className="flex items-center gap-2 ml-4">
                  {/* VS Code 按钮占位容器 - 始终保持空间，避免布局跳动 */}
                  {appType === "codex" ? (
                    <div className="w-[130px]">
                      {provider.category !== "official" && isCurrent && (
                        <button
                          onClick={() =>
                            vscodeAppliedFor === provider.id
                              ? handleRemoveFromVSCode()
                              : handleApplyToVSCode(provider)
                          }
                          className={cn(
                            "inline-flex items-center gap-1 px-3 py-1.5 text-sm font-medium rounded-md transition-colors w-full whitespace-nowrap justify-center",
                            vscodeAppliedFor === provider.id
                              ? "border border-gray-300 text-gray-600 hover:border-red-300 hover:text-red-600 hover:bg-red-50 dark:border-gray-600 dark:text-gray-400 dark:hover:border-red-800 dark:hover:text-red-400 dark:hover:bg-red-900/20"
                              : "border border-gray-300 text-gray-700 hover:border-blue-300 hover:text-blue-600 hover:bg-blue-50 dark:border-gray-600 dark:text-gray-300 dark:hover:border-blue-700 dark:hover:text-blue-400 dark:hover:bg-blue-900/20"
                          )}
                          title={
                            vscodeAppliedFor === provider.id
                              ? t("provider.removeFromVSCode")
                              : t("provider.applyToVSCode")
                          }
                        >
                          {vscodeAppliedFor === provider.id
                            ? t("provider.removeFromVSCode")
                            : t("provider.applyToVSCode")}
                        </button>
                      )}
                    </div>
                  ) : null}

                  {!isCurrent ? (
                    <button
                      onClick={() => onSwitch(id)}
                      className={cn(
                        buttonStyles.base,
                        buttonStyles.variants.primary,
                        buttonStyles.sizes.sm,
                        "inline-flex items-center gap-1.5",
                      )}
                    >
                      <Play className="w-4 h-4" />
                      {t("common.switch")}
                    </button>
                  ) : (
                    <div className="flex items-center gap-2">
                      <button
                        onClick={() => onEdit(id)}
                        className={cn(
                          buttonStyles.base,
                          buttonStyles.variants.ghost,
                          buttonStyles.sizes.sm,
                        )}
                      >
                        <Edit3 className="w-4 h-4" />
                      </button>
                      {!isOfficial && (
                        <button
                          onClick={() => onDelete(id)}
                          className={cn(
                            buttonStyles.base,
                            buttonStyles.variants.danger,
                            buttonStyles.sizes.sm,
                          )}
                        >
                          <Trash2 className="w-4 h-4" />
                        </button>
                      )}
                    </div>
                  )}
                </div>
              </div>
            </div>
          </div>
        );
      })}
    </div>
  );
};

export default ProviderList;