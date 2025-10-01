import React from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Provider } from "../types";
import { Play, Edit3, Trash2, CheckCircle2, Users, Check } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import { AppType } from "../lib/query";
import {
  useVSCodeSettingsQuery,
  useVSCodeSyncMutation,
  useVSCodeRemoveMutation,
  useSwitchProviderMutation,
  useDeleteProviderMutation,
  useVSCodeAppliedQuery
} from "../lib/query";
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
        <Card>
          <CardContent className="flex flex-col items-center justify-center py-12">
            <div className="w-16 h-16 mb-4 bg-muted rounded-full flex items-center justify-center">
              <Users size={24} className="text-muted-foreground" />
            </div>
            <h3 className="text-lg font-semibold mb-2">
              {t("provider.noProviders")}
            </h3>
            <p className="text-muted-foreground text-sm text-center max-w-sm">
              {t("provider.noProvidersDescription")}
            </p>
          </CardContent>
        </Card>
      ) : (
        <div className="space-y-3">
          {Object.values(providers).map((provider) => {
            const isCurrent = provider.id === currentProviderId;
            const apiUrl = getApiUrl(provider);

            return (
              <Card
                key={provider.id}
                className={cn(
                  "transition-all duration-200",
                  isCurrent && "border-primary bg-primary/5"
                )}
              >
                <CardContent className="p-4">
                  <div className="flex items-start justify-between">
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-3 mb-2">
                        <h3 className="font-medium truncate">
                          {provider.name}
                        </h3>
                        {isCurrent && (
                          <Badge variant="default" className="gap-1">
                            <CheckCircle2 size={12} />
                            {t("provider.currentlyUsing")}
                          </Badge>
                        )}
                      </div>

                      <div className="flex items-center gap-2 text-sm">
                        {provider.websiteUrl ? (
                          <Button
                            variant="link"
                            className="h-auto p-0 text-blue-600 hover:text-blue-700 dark:text-blue-400 dark:hover:text-blue-300"
                            onClick={() => handleUrlClick(provider.websiteUrl!)}
                            title={`访问 ${provider.websiteUrl}`}
                          >
                            {provider.websiteUrl}
                          </Button>
                        ) : (
                          <span
                            className="text-muted-foreground truncate"
                            title={apiUrl}
                          >
                            {apiUrl}
                          </span>
                        )}
                      </div>
                    </div>

                    <div className="flex items-center gap-2 ml-4 flex-shrink-0">
                      {/* VS Code 按钮占位容器 - 始终保持空间，避免布局跳动 */}
                      {appType === "codex" ? (
                        <div className="w-[130px]">
                          {provider.category !== "official" && isCurrent && (
                            <Button
                              variant={vscodeAppliedFor === provider.id ? "outline" : "secondary"}
                              size="sm"
                              onClick={() =>
                                vscodeAppliedFor === provider.id
                                  ? handleRemoveFromVSCode()
                                  : handleApplyToVSCode(provider)
                              }
                              disabled={vscodeSyncMutation.isPending}
                              className="w-full"
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
                            </Button>
                          )}
                        </div>
                      ) : null}

                      <Button
                        size="sm"
                        onClick={() => handleSwitch(provider.id)}
                        disabled={isCurrent || switchProviderMutation.isPending}
                        variant={isCurrent ? "secondary" : "default"}
                        className="w-[90px]"
                      >
                        {isCurrent ? (
                          <>
                            <Check size={14} />
                            {t("provider.inUse")}
                          </>
                        ) : switchProviderMutation.isPending ? (
                          <>
                            <div className="w-3 h-3 border border-current border-t-transparent rounded-full animate-spin" />
                            {t("common.switching")}
                          </>
                        ) : (
                          <>
                            <Play size={14} />
                            {t("provider.enable")}
                          </>
                        )}
                      </Button>

                      <Button
                        variant="ghost"
                        size="icon"
                        onClick={() => onEdit(provider.id)}
                        title={t("provider.editProvider")}
                      >
                        <Edit3 size={16} />
                      </Button>

                      <Button
                        variant="ghost"
                        size="icon"
                        onClick={() => handleDelete(provider.id)}
                        disabled={isCurrent || deleteProviderMutation.isPending}
                        className={cn(
                          "hover:text-destructive hover:bg-destructive/10",
                          (isCurrent || deleteProviderMutation.isPending) && "opacity-50 cursor-not-allowed"
                        )}
                        title={t("provider.deleteProvider")}
                      >
                        {deleteProviderMutation.isPending ? (
                          <div className="w-4 h-4 border border-current border-t-transparent rounded-full animate-spin" />
                        ) : (
                          <Trash2 size={16} />
                        )}
                      </Button>
                    </div>
                  </div>
                </CardContent>
              </Card>
            );
          })}
        </div>
      )}
    </div>
  );
};

export default ProviderList;
