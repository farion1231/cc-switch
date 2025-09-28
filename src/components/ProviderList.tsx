import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Provider } from "../types";
import { Play, Edit3, Trash2, CheckCircle2, Users } from "lucide-react";
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
    duration?: number
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
        const match = cfg.config.match(/base_url\s*=\s*(['"])([^'\"]+)\1/);
        if (match && match[2]) return match[2];
      }
      return t("provider.notConfigured");
    } catch {
      return t("provider.configError");
    }
  };

  const handleUrlClick = async (url: string) => {
    try {
      await window.api.openExternal(url);
    } catch (error) {
      console.error(t("console.openLinkFailed"), error);
    }
  };

  // 解析 Codex 配置中的 base_url（已提取到公共工具）

  // VS Code 按钮：仅在 Codex + 当前供应商显示；按钮文案根据是否"已应用"变化
  const [vscodeAppliedFor, setVscodeAppliedFor] = useState<string | null>(null);
  const { enableAutoSync, disableAutoSync } = useVSCodeAutoSync();

  // 当当前供应商或 appType 变化时，尝试读取 VS Code settings 并检测状态
  useEffect(() => {
    const check = async () => {
      if (appType !== "codex" || !currentProviderId) {
        setVscodeAppliedFor(null);
        return;
      }
      const status = await window.api.getVSCodeSettingsStatus();
      if (!status.exists) {
        setVscodeAppliedFor(null);
        return;
      }
      try {
        const content = await window.api.readVSCodeSettings();
        const detected = detectApplied(content);
        // 认为“已应用”的条件（非官方供应商）：VS Code 中的 apiBase 与当前供应商的 base_url 完全一致
        const current = providers[currentProviderId];
        let applied = false;
        if (current && current.category !== "official") {
          const base = getCodexBaseUrl(current);
          if (detected.apiBase && base) {
            applied =
              normalizeBaseUrl(detected.apiBase) === normalizeBaseUrl(base);
          }
        }
        setVscodeAppliedFor(applied ? currentProviderId : null);
      } catch {
        setVscodeAppliedFor(null);
      }
    };
    check();
  }, [appType, currentProviderId, providers]);

  const handleApplyToVSCode = async (provider: Provider) => {
    try {
      const status = await window.api.getVSCodeSettingsStatus();
      if (!status.exists) {
        onNotify?.(t("notifications.vscodeSettingsNotFound"), "error", 3000);
        return;
      }

      const raw = await window.api.readVSCodeSettings();

      const isOfficial = provider.category === "official";
      // 非官方且缺少 base_url 时直接报错并返回，避免“空写入”假成功
      if (!isOfficial) {
        const parsed = getCodexBaseUrl(provider);
        if (!parsed) {
          onNotify?.(t("notifications.missingBaseUrl"), "error", 4000);
          return;
        }
      }

      const baseUrl = isOfficial ? undefined : getCodexBaseUrl(provider);
      const next = applyProviderToVSCode(raw, { baseUrl, isOfficial });

      if (next === raw) {
        // 幂等：没有变化也提示成功
        onNotify?.(t("notifications.appliedToVSCode"), "success", 3000);
        setVscodeAppliedFor(provider.id);
        // 用户手动应用时，启用自动同步
        enableAutoSync();
        return;
      }

      await window.api.writeVSCodeSettings(next);
      onNotify?.(t("notifications.appliedToVSCode"), "success", 3000);
      setVscodeAppliedFor(provider.id);
      // 用户手动应用时，启用自动同步
      enableAutoSync();
    } catch (e: any) {
      console.error(e);
      const msg =
        e && e.message ? e.message : t("notifications.syncVSCodeFailed");
      onNotify?.(msg, "error", 5000);
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
        isOfficial: true,
      });
      if (next === raw) {
        onNotify?.(t("notifications.removedFromVSCode"), "success", 3000);
        setVscodeAppliedFor(null);
        // 用户手动移除时，禁用自动同步
        disableAutoSync();
        return;
      }
      await window.api.writeVSCodeSettings(next);
      onNotify?.(t("notifications.removedFromVSCode"), "success", 3000);
      setVscodeAppliedFor(null);
      // 用户手动移除时，禁用自动同步
      disableAutoSync();
    } catch (e: any) {
      console.error(e);
      const msg =
        e && e.message ? e.message : t("notifications.syncVSCodeFailed");
      onNotify?.(msg, "error", 5000);
    }
  };

  // 对供应商列表进行排序
  const sortedProviders = Object.values(providers).sort((a, b) => {
    // 按添加时间排序
    // 没有时间戳的视为最早添加的（排在最前面）
    // 有时间戳的按时间升序排列
    const timeA = a.createdAt || 0;
    const timeB = b.createdAt || 0;

    // 如果都没有时间戳，按名称排序
    if (timeA === 0 && timeB === 0) {
      return a.name.localeCompare(b.name, "zh-CN");
    }

    // 如果只有一个没有时间戳，没有时间戳的排在前面
    if (timeA === 0) return -1;
    if (timeB === 0) return 1;

    // 都有时间戳，按时间升序
    return timeA - timeB;
  });

  return (
    <div className="space-y-4">
      {sortedProviders.length === 0 ? (
        <div className="text-center py-12">
          <div className="w-16 h-16 mx-auto mb-4 bg-gray-100 rounded-full flex items-center justify-center">
            <Users size={24} className="text-gray-400" />
          </div>
          <h3 className="text-lg font-medium text-gray-900 dark:text-gray-100 mb-2">
            {t("provider.noProviders")}
          </h3>
          <p className="text-gray-500 dark:text-gray-400 text-sm">
            {t("provider.noProvidersDescription")}
          </p>
        </div>
      ) : (
        <div className="space-y-3">
          {sortedProviders.map((provider) => {
            const isCurrent = provider.id === currentProviderId;
            const apiUrl = getApiUrl(provider);

            return (
              <div
                key={provider.id}
                className={cn(
                  isCurrent ? cardStyles.selected : cardStyles.interactive
                )}
              >
                <div className="flex items-start justify-between">
                  <div className="flex-1">
                    <div className="flex items-center gap-3 mb-2">
                      <h3 className="font-medium text-gray-900 dark:text-gray-100">
                        {provider.name}
                      </h3>
                      {/* 分类徽章已移除 */}
                      <div
                        className={cn(
                          badgeStyles.success,
                          !isCurrent && "invisible"
                        )}
                      >
                        <CheckCircle2 size={12} />
                        {t("provider.currentlyUsing")}
                      </div>
                    </div>

                    <div className="flex items-center gap-2 text-sm">
                      {provider.websiteUrl ? (
                        <button
                          onClick={(e) => {
                            e.preventDefault();
                            handleUrlClick(provider.websiteUrl!);
                          }}
                          className="inline-flex items-center gap-1 text-blue-500 dark:text-blue-400 hover:opacity-90 transition-colors"
                          title={`访问 ${provider.websiteUrl}`}
                        >
                          {provider.websiteUrl}
                        </button>
                      ) : (
                        <span
                          className="text-gray-500 dark:text-gray-400"
                          title={apiUrl}
                        >
                          {apiUrl}
                        </span>
                      )}
                    </div>
                  </div>

                  <div className="flex items-center gap-2 ml-4">
                    {appType === "codex" &&
                      provider.category !== "official" && (
                        <button
                          onClick={() =>
                            vscodeAppliedFor === provider.id
                              ? handleRemoveFromVSCode()
                              : handleApplyToVSCode(provider)
                          }
                          className={cn(
                            "inline-flex items-center gap-1 px-3 py-1.5 text-sm font-medium rounded-md transition-colors w-[130px] whitespace-nowrap justify-center",
                            !isCurrent && "invisible",
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
                    <button
                      onClick={() => onSwitch(provider.id)}
                      disabled={isCurrent}
                      className={cn(
                        "inline-flex items-center gap-1 px-3 py-1.5 text-sm font-medium rounded-md transition-colors w-[76px] justify-center whitespace-nowrap",
                        isCurrent
                          ? "bg-gray-100 text-gray-400 dark:bg-gray-800 dark:text-gray-500 cursor-not-allowed"
                          : "bg-blue-500 text-white hover:bg-blue-600 dark:bg-blue-600 dark:hover:bg-blue-700"
                      )}
                    >
                      {!isCurrent && <Play size={14} />}
                      {isCurrent ? t("provider.inUse") : t("provider.enable")}
                    </button>

                    <button
                      onClick={() => onEdit(provider.id)}
                      className={buttonStyles.icon}
                      title={t("provider.editProvider")}
                    >
                      <Edit3 size={16} />
                    </button>

                    <button
                      onClick={() => onDelete(provider.id)}
                      disabled={isCurrent}
                      className={cn(
                        buttonStyles.icon,
                        isCurrent
                          ? "text-gray-400 cursor-not-allowed"
                          : "text-gray-500 hover:text-red-500 hover:bg-red-100 dark:text-gray-400 dark:hover:text-red-400 dark:hover:bg-red-500/10"
                      )}
                      title={t("provider.deleteProvider")}
                    >
                      <Trash2 size={16} />
                    </button>
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
};

export default ProviderList;
