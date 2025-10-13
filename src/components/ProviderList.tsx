import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { Provider, UsageScript } from "../types";
import { AppType } from "../lib/tauri-api";
import { Play, Edit3, Trash2, CheckCircle2, Users, Check, BarChart3 } from "lucide-react";
import { buttonStyles, cardStyles, badgeStyles, cn } from "../lib/styles";
import UsageFooter from "./UsageFooter";
import UsageScriptModal from "./UsageScriptModal";
// 不再在列表中显示分类徽章，避免造成困惑

interface ProviderListProps {
  providers: Record<string, Provider>;
  currentProviderId: string;
  onSwitch: (id: string) => void;
  onDelete: (id: string) => void;
  onEdit: (id: string) => void;
  appType: AppType;
  onNotify?: (
    message: string,
    type: "success" | "error",
    duration?: number,
  ) => void;
  onProvidersUpdated?: () => Promise<void>;
}

const ProviderList: React.FC<ProviderListProps> = ({
  providers,
  currentProviderId,
  onSwitch,
  onDelete,
  onEdit,
  appType,
  onNotify,
  onProvidersUpdated,
}) => {
  const { t, i18n } = useTranslation();
  const [usageModalProviderId, setUsageModalProviderId] = useState<string | null>(null);

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
      onNotify?.(
        `${t("console.openLinkFailed")}: ${String(error)}`,
        "error",
        4000,
      );
    }
  };

  // 列表页不再提供 Claude 插件按钮，统一在“设置”中控制

  // 处理用量配置保存
  const handleSaveUsageScript = async (providerId: string, script: UsageScript) => {
    try {
      const provider = providers[providerId];
      const updatedProvider = {
        ...provider,
        meta: {
          ...provider.meta,
          usage_script: script,
        },
      };
      await window.api.updateProvider(updatedProvider, appType);
      onNotify?.("用量查询配置已保存", "success", 2000);
      // 重新加载供应商列表,触发 UsageFooter 的 useEffect
      if (onProvidersUpdated) {
        await onProvidersUpdated();
      }
    } catch (error) {
      console.error("保存用量配置失败:", error);
      onNotify?.("保存失败", "error");
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
      const locale = i18n.language === "zh" ? "zh-CN" : "en-US";
      return a.name.localeCompare(b.name, locale);
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
                  isCurrent ? cardStyles.selected : cardStyles.interactive,
                )}
              >
                <div className="flex items-center justify-between">
                  <div className="flex-1">
                    <div className="flex items-center gap-3 mb-2">
                      <h3 className="font-medium text-gray-900 dark:text-gray-100">
                        {provider.name}
                      </h3>
                      {/* 分类徽章已移除 */}
                      <div
                        className={cn(
                          badgeStyles.success,
                          !isCurrent && "invisible",
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
                          title={t("providerForm.visitWebsite", {
                            url: provider.websiteUrl,
                          })}
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
                    <button
                      onClick={() => onSwitch(provider.id)}
                      disabled={isCurrent}
                      className={cn(
                        "inline-flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-md transition-colors w-[90px] justify-center whitespace-nowrap",
                        isCurrent
                          ? "bg-gray-100 text-gray-400 dark:bg-gray-800 dark:text-gray-500 cursor-not-allowed"
                          : "bg-blue-500 text-white hover:bg-blue-600 dark:bg-blue-600 dark:hover:bg-blue-700",
                      )}
                    >
                      {isCurrent ? <Check size={14} /> : <Play size={14} />}
                      {isCurrent ? t("provider.inUse") : t("provider.enable")}
                    </button>

                    <button
                      onClick={() => onEdit(provider.id)}
                      className={buttonStyles.icon}
                      title={t("provider.editProvider")}
                    >
                      <Edit3 size={16} />
                    </button>

                    {/* 新增：用量配置按钮 */}
                    <button
                      onClick={() => setUsageModalProviderId(provider.id)}
                      className={buttonStyles.icon}
                      title="配置用量查询"
                    >
                      <BarChart3 size={16} />
                    </button>

                    <button
                      onClick={() => onDelete(provider.id)}
                      disabled={isCurrent}
                      className={cn(
                        buttonStyles.icon,
                        isCurrent
                          ? "text-gray-400 cursor-not-allowed"
                          : "text-gray-500 hover:text-red-500 hover:bg-red-100 dark:text-gray-400 dark:hover:text-red-400 dark:hover:bg-red-500/10",
                      )}
                      title={t("provider.deleteProvider")}
                    >
                      <Trash2 size={16} />
                    </button>
                  </div>
                </div>

                {/* 用量信息 Footer */}
                <UsageFooter
                  providerId={provider.id}
                  appType={appType!}
                  usageEnabled={provider.meta?.usage_script?.enabled || false}
                />
              </div>
            );
          })}
        </div>
      )}

      {/* 用量配置模态框 */}
      {usageModalProviderId && providers[usageModalProviderId] && (
        <UsageScriptModal
          provider={providers[usageModalProviderId]}
          appType={appType!}
          onClose={() => setUsageModalProviderId(null)}
          onSave={(script) =>
            handleSaveUsageScript(usageModalProviderId, script)
          }
          onNotify={onNotify}
        />
      )}
    </div>
  );
};

export default ProviderList;
