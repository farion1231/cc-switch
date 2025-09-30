import React from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Provider } from "../types";
import { Play, Edit3, Trash2, CheckCircle2, Users, Check } from "lucide-react";
import { buttonStyles, cardStyles, badgeStyles, cn } from "../lib/styles";
import { AppType } from "../lib/query";
import {
  useVSCodeSettingsQuery,
  useVSCodeSyncMutation,
  useVSCodeRemoveMutation,
  useSwitchProviderMutation,
  useDeleteProviderMutation,
  useVSCodeAppliedQuery
} from "../lib/query";
import { getCodexBaseUrl } from "../utils/providerConfigUtils";
import { useVSCodeAutoSync } from "../hooks/useVSCodeAutoSync";
// 不再在列表中显示分类徽章，避免造成困惑

interface ProviderListProps {
  providers: Record<string, Provider>;
  currentProviderId: string;
  onEdit: (id: string) => void;
  appType?: AppType;
}

const ProviderList: React.FC<ProviderListProps> = ({
  providers,
  currentProviderId,
  onEdit,
  appType,
}) => {
  const { t } = useTranslation();

  // React Query mutations
  const switchProviderMutation = useSwitchProviderMutation(appType!);
  const deleteProviderMutation = useDeleteProviderMutation(appType!);
  const vscodeSyncMutation = useVSCodeSyncMutation(appType!);
  const vscodeRemoveMutation = useVSCodeRemoveMutation();
  const { refetch: refetchVSCodeSettings } = useVSCodeSettingsQuery();
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

  const handleSwitch = (providerId: string) => {
    switchProviderMutation.mutate(providerId, {
      onSuccess: () => {
        toast.success(t("notifications.providerSwitched"));
      },
      onError: (error: Error) => {
        toast.error(error.message);
      }
    });
  };

  const handleDelete = (providerId: string) => {
    deleteProviderMutation.mutate(providerId, {
      onSuccess: () => {
        toast.success(t("notifications.providerDeleted"));
      },
      onError: (error: Error) => {
        toast.error(error.message);
      }
    });
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
  const { enableAutoSync, disableAutoSync } = useVSCodeAutoSync();
  const { data: vscodeAppliedFor } = useVSCodeAppliedQuery(appType!, currentProviderId, providers);

  const handleApplyToVSCode = (provider: Provider) => {
    vscodeSyncMutation.mutate(provider.id, {
      onSuccess: () => {
        toast.success(t("notifications.appliedToVSCode"));
        enableAutoSync();
        // Refetch VS Code settings to update state
        refetchVSCodeSettings();
      },
      onError: (error: Error) => {
        toast.error(error.message);
      }
    });
  };

  const handleRemoveFromVSCode = () => {
    vscodeRemoveMutation.mutate(undefined, {
      onSuccess: () => {
        toast.success(t("notifications.removedFromVSCode"));
        disableAutoSync();
        // Refetch VS Code settings to update state
        refetchVSCodeSettings();
      },
      onError: (error: Error) => {
        toast.error(error.message);
      }
    });
  };

  // 供应商列表已在查询函数中排序

  return (
    <div className="space-y-4">
      {Object.values(providers).length === 0 ? (
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
          {Object.values(providers).map((provider) => {
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
                                : vscodeSyncMutation.isPending
                                ? "border border-gray-300 text-gray-400 cursor-wait dark:border-gray-600 dark:text-gray-500"
                                : "border border-gray-300 text-gray-700 hover:border-blue-300 hover:text-blue-600 hover:bg-blue-50 dark:border-gray-600 dark:text-gray-300 dark:hover:border-blue-700 dark:hover:text-blue-400 dark:hover:bg-blue-900/20"
                            )}
                            title={
                              vscodeAppliedFor === provider.id
                                ? t("provider.removeFromVSCode")
                                : vscodeSyncMutation.isPending
                                ? t("common.applying")
                                : t("provider.applyToVSCode")
                            }
                          >
                            {vscodeAppliedFor === provider.id
                              ? t("provider.removeFromVSCode")
                              : vscodeSyncMutation.isPending
                              ? <div className="w-3 h-3 border border-current border-t-transparent rounded-full animate-spin" />
                              : t("provider.applyToVSCode")}
                          </button>
                        )}
                      </div>
                    ) : null}
                    <button
                      onClick={() => handleSwitch(provider.id)}
                      disabled={isCurrent || switchProviderMutation.isPending}
                      className={cn(
                        "inline-flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-md transition-colors w-[90px] justify-center whitespace-nowrap",
                        isCurrent
                          ? "bg-gray-100 text-gray-400 dark:bg-gray-800 dark:text-gray-500 cursor-not-allowed"
                          : switchProviderMutation.isPending
                          ? "bg-gray-300 text-gray-500 dark:bg-gray-700 dark:text-gray-400 cursor-wait"
                          : "bg-blue-500 text-white hover:bg-blue-600 dark:bg-blue-600 dark:hover:bg-blue-700"
                      )}
                    >
                      {isCurrent ? <Check size={14} /> : switchProviderMutation.isPending ? <div className="w-3 h-3 border border-current border-t-transparent rounded-full animate-spin" /> : <Play size={14} />}
                      {isCurrent ? t("provider.inUse") : switchProviderMutation.isPending ? t("common.switching") : t("provider.enable")}
                    </button>

                    <button
                      onClick={() => onEdit(provider.id)}
                      className={buttonStyles.icon}
                      title={t("provider.editProvider")}
                    >
                      <Edit3 size={16} />
                    </button>

                    <button
                      onClick={() => handleDelete(provider.id)}
                      disabled={isCurrent || deleteProviderMutation.isPending}
                      className={cn(
                        buttonStyles.icon,
                        isCurrent
                          ? "text-gray-400 cursor-not-allowed"
                          : deleteProviderMutation.isPending
                          ? "text-gray-400 cursor-wait"
                          : "text-gray-500 hover:text-red-500 hover:bg-red-100 dark:text-gray-400 dark:hover:text-red-400 dark:hover:bg-red-500/10"
                      )}
                      title={t("provider.deleteProvider")}
                    >
                      {deleteProviderMutation.isPending ? <div className="w-4 h-4 border border-current border-t-transparent rounded-full animate-spin" /> : <Trash2 size={16} />}
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
