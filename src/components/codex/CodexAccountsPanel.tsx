import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  Check,
  Loader2,
  Pencil,
  RefreshCw,
  RotateCcw,
  Save,
  Search,
  Undo2,
  X,
  Zap,
} from "lucide-react";
import { toast } from "sonner";
import { codexAccountsApi } from "@/lib/api";
import type { CodexAccountSummary } from "@/lib/api/codexAccounts";
import {
  useCodexAllQuotas,
  useCodexQuotaForecasts,
  useSettingsQuery,
  useSaveSettingsMutation,
} from "@/lib/query";
import { subscriptionKeys } from "@/lib/query/subscription";
import type {
  CodexQuotaForecast,
  SubscriptionQuota,
} from "@/types/subscription";
import { CodexQuotaForecastBadge } from "./CodexQuotaForecastBadge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import { extractErrorMessage } from "@/utils/errorUtils";

const QUERY_KEY = ["codex", "account-snapshots"];

export function CodexAccountsPanel() {
  return (
    <div className="px-6 flex flex-col flex-1 min-h-0 overflow-hidden">
      <div className="flex-1 overflow-y-auto overflow-x-hidden pb-12 px-1">
        <CodexAccountsManager />
      </div>
    </div>
  );
}

interface CodexAccountsManagerProps {
  embedded?: boolean;
}

export function CodexAccountsManager({
  embedded = false,
}: CodexAccountsManagerProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [label, setLabel] = useState("");

  const accountsQuery = useQuery({
    queryKey: QUERY_KEY,
    queryFn: () => codexAccountsApi.list(),
  });

  const settingsQuery = useSettingsQuery();
  const saveSettingsMutation = useSaveSettingsMutation();

  const refreshIntervalSec =
    settingsQuery.data?.codexQuotaRefreshInterval ?? 300;
  const quotasQuery = useCodexAllQuotas({ enabled: true, autoQuery: false });
  const forecastsQuery = useCodexQuotaForecasts();

  const accounts = useMemo(
    () => accountsQuery.data ?? [],
    [accountsQuery.data],
  );

  const invalidate = async () => {
    await queryClient.invalidateQueries({ queryKey: QUERY_KEY });
  };

  const captureMutation = useMutation({
    mutationFn: () => codexAccountsApi.captureCurrent(label),
    onSuccess: async () => {
      setLabel("");
      await invalidate();
      toast.success(
        t("codexAccounts.captureSuccess", {
          defaultValue: "当前 Codex 账号已保存",
        }),
      );
    },
    onError: (error: Error) => {
      toast.error(
        t("codexAccounts.captureFailed", {
          defaultValue: "保存账号失败：{{error}}",
          error: extractErrorMessage(error),
        }),
      );
    },
  });

  const switchMutation = useMutation({
    mutationFn: (accountKey: string) => codexAccountsApi.switch(accountKey),
    onSuccess: async (result) => {
      await invalidate();
      toast.success(
        result.restartRecommended
          ? t("codexAccounts.switchSuccessRestart", {
              defaultValue: "账号已切换，建议重启 Codex App",
            })
          : t("codexAccounts.switchSuccess", {
              defaultValue: "账号已是当前账号",
            }),
      );
    },
    onError: (error: Error) => {
      toast.error(
        t("codexAccounts.switchFailed", {
          defaultValue: "切换账号失败：{{error}}",
          error: extractErrorMessage(error),
        }),
      );
    },
  });

  const renameMutation = useMutation({
    mutationFn: ({
      accountKey,
      profileName,
    }: {
      accountKey: string;
      profileName: string;
    }) => codexAccountsApi.rename(accountKey, profileName),
    onSuccess: async () => {
      await invalidate();
      await queryClient.invalidateQueries({
        queryKey: subscriptionKeys.allCodexQuotas(),
      });
      toast.success(
        t("codexAccounts.renameSuccess", {
          defaultValue: "账号名称已更新",
        }),
      );
    },
    onError: (error: Error) => {
      toast.error(
        t("codexAccounts.renameFailed", {
          defaultValue: "重命名失败：{{error}}",
          error: extractErrorMessage(error),
        }),
      );
    },
  });

  const rollbackMutation = useMutation({
    mutationFn: () => codexAccountsApi.rollback(),
    onSuccess: async () => {
      await invalidate();
      toast.success(
        t("codexAccounts.rollbackSuccess", {
          defaultValue: "已回滚到上一次 Codex 账号",
        }),
      );
    },
    onError: (error: Error) => {
      toast.error(
        t("codexAccounts.rollbackFailed", {
          defaultValue: "回滚失败：{{error}}",
          error: extractErrorMessage(error),
        }),
      );
    },
  });

  const restartMutation = useMutation({
    mutationFn: () => codexAccountsApi.restartCodex(),
    onSuccess: (result) => {
      toast.success(
        result.message ||
          t("codexAccounts.restartSuccess", {
            defaultValue: "Codex App 已重启",
          }),
      );
    },
    onError: (error: Error) => {
      toast.error(
        t("codexAccounts.restartFailed", {
          defaultValue: "重启 Codex App 失败：{{error}}",
          error: extractErrorMessage(error),
        }),
      );
    },
  });

  const scanMutation = useMutation({
    mutationFn: () => codexAccountsApi.list(),
    onSuccess: (nextAccounts) => {
      queryClient.setQueryData(QUERY_KEY, nextAccounts);
      toast.success(
        t("codexAccounts.scanSuccess", {
          defaultValue: "已扫描到 {{count}} 个 Codex 账号快照",
          count: nextAccounts.length,
        }),
      );
    },
    onError: (error: Error) => {
      toast.error(
        t("codexAccounts.scanFailed", {
          defaultValue: "扫描账号快照失败：{{error}}",
          error: extractErrorMessage(error),
        }),
      );
    },
  });

  const toolbar = (
    <div className="flex flex-wrap items-center gap-2">
      <Button
        variant="outline"
        size="sm"
        onClick={() => void accountsQuery.refetch()}
        disabled={accountsQuery.isFetching}
      >
        {accountsQuery.isFetching ? (
          <Loader2 className="w-4 h-4 animate-spin" />
        ) : (
          <RefreshCw className="w-4 h-4" />
        )}
        {t("common.refresh", { defaultValue: "刷新" })}
      </Button>
      <Button
        variant="outline"
        size="sm"
        onClick={() => scanMutation.mutate()}
        disabled={scanMutation.isPending}
      >
        {scanMutation.isPending ? (
          <Loader2 className="w-4 h-4 animate-spin" />
        ) : (
          <Search className="w-4 h-4" />
        )}
        {t("codexAccounts.scanAccounts", {
          defaultValue: "扫描账号快照",
        })}
      </Button>
      <Button
        variant="outline"
        size="sm"
        onClick={() => rollbackMutation.mutate()}
        disabled={rollbackMutation.isPending}
      >
        {rollbackMutation.isPending ? (
          <Loader2 className="w-4 h-4 animate-spin" />
        ) : (
          <Undo2 className="w-4 h-4" />
        )}
        {t("codexAccounts.rollback", { defaultValue: "回滚" })}
      </Button>
      <Button
        variant="outline"
        size="sm"
        onClick={() => restartMutation.mutate()}
        disabled={restartMutation.isPending}
      >
        {restartMutation.isPending ? (
          <Loader2 className="w-4 h-4 animate-spin" />
        ) : (
          <RotateCcw className="w-4 h-4" />
        )}
        {t("codexAccounts.restart", { defaultValue: "重启 Codex" })}
      </Button>
      <Button
        variant="outline"
        size="sm"
        onClick={async () => {
          await quotasQuery.refetch();
          await forecastsQuery.refetch();
        }}
        disabled={quotasQuery.isFetching}
      >
        {quotasQuery.isFetching ? (
          <Loader2 className="w-4 h-4 animate-spin" />
        ) : (
          <Zap className="w-4 h-4" />
        )}
        {t("codexAccounts.refreshNow", { defaultValue: "立即刷新" })}
      </Button>
      <div className="flex items-center gap-1.5">
        <span className="text-xs text-muted-foreground whitespace-nowrap">
          {settingsQuery.data?.usageAdaptiveRefresh
            ? t("settings.usageAdaptiveRefresh", {
                defaultValue: "Adaptive refresh",
              })
            : t("codexAccounts.refreshInterval", { defaultValue: "刷新" })}
        </span>
        <Select
          value={String(refreshIntervalSec)}
          onValueChange={async (value) => {
            const sec = Number(value);
            const current = settingsQuery.data;
            if (!current) return;
            try {
              await saveSettingsMutation.mutateAsync({
                ...current,
                codexQuotaRefreshInterval: sec,
              });
              toast.success(
                t("codexAccounts.intervalSaved", {
                  defaultValue: "刷新间隔已保存",
                }),
              );
            } catch {
              // error handled by mutation
            }
          }}
          disabled={
            saveSettingsMutation.isPending ||
            settingsQuery.data?.usageAdaptiveRefresh === true
          }
        >
          <SelectTrigger className="h-8 w-[90px] text-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="60">1min</SelectItem>
            <SelectItem value="300">5min</SelectItem>
            <SelectItem value="1800">30min</SelectItem>
            <SelectItem value="3600">60min</SelectItem>
          </SelectContent>
        </Select>
      </div>
    </div>
  );

  return (
    <div className={cn("space-y-4", !embedded && "max-w-5xl mx-auto")}>
      {embedded ? (
        <div className="flex justify-end">{toolbar}</div>
      ) : (
        <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
          <div className="min-w-0">
            <h2 className="text-xl font-semibold tracking-normal">
              {t("codexAccounts.title", {
                defaultValue: "Codex 官方账号快照",
              })}
            </h2>
            <p className="mt-1 text-sm text-muted-foreground">
              {t("codexAccounts.subtitle", {
                defaultValue:
                  "保存、切换和回滚 ~/.codex/auth.json 中的官方登录账号。",
              })}
            </p>
          </div>
          {toolbar}
        </div>
      )}

      <div className="rounded-lg border bg-card p-4">
        <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
          <Input
            value={label}
            onChange={(event) => setLabel(event.target.value)}
            placeholder={t("codexAccounts.labelPlaceholder", {
              defaultValue: "给当前账号起个名字，例如：Plus 个人号",
            })}
            className="sm:max-w-sm"
          />
          <Button
            onClick={() => captureMutation.mutate()}
            disabled={captureMutation.isPending}
          >
            {captureMutation.isPending ? (
              <Loader2 className="w-4 h-4 animate-spin" />
            ) : (
              <Save className="w-4 h-4" />
            )}
            {t("codexAccounts.captureCurrent", {
              defaultValue: "保存当前账号",
            })}
          </Button>
        </div>
      </div>

      {accountsQuery.isLoading ? (
        <div className="space-y-3">
          {[0, 1].map((index) => (
            <div
              key={index}
              className="h-24 rounded-lg border border-dashed bg-muted/40"
            />
          ))}
        </div>
      ) : accounts.length === 0 ? (
        <div className="rounded-lg border border-dashed px-6 py-10 text-center text-sm text-muted-foreground">
          {t("codexAccounts.empty", {
            defaultValue:
              "还没有保存的 Codex 账号。先登录 Codex，再点击“保存当前账号”。",
          })}
        </div>
      ) : (
        <div className="space-y-3">
          {accounts.map((account) => (
            <AccountRow
              key={account.accountKey}
              account={account}
              quota={quotasQuery.data?.[account.accountKey]}
              forecast={forecastsQuery.data?.[account.accountKey]}
              switchingKey={
                switchMutation.isPending ? switchMutation.variables : undefined
              }
              onSwitch={(accountKey) => switchMutation.mutate(accountKey)}
              renamingKey={
                renameMutation.isPending
                  ? renameMutation.variables?.accountKey
                  : undefined
              }
              onRename={(accountKey, profileName) =>
                renameMutation.mutate({ accountKey, profileName })
              }
            />
          ))}
        </div>
      )}
    </div>
  );
}

interface AccountRowProps {
  account: CodexAccountSummary;
  quota?: SubscriptionQuota;
  forecast?: CodexQuotaForecast;
  switchingKey?: string;
  onSwitch: (accountKey: string) => void;
  renamingKey?: string;
  onRename: (accountKey: string, profileName: string) => void;
}

function getUtilizationColor(utilization: number): string {
  if (utilization >= 90) return "text-red-500";
  if (utilization >= 70) return "text-orange-500";
  return "text-emerald-500";
}

function AccountRow({
  account,
  quota,
  forecast,
  switchingKey,
  onSwitch,
  renamingKey,
  onRename,
}: AccountRowProps) {
  const { t } = useTranslation();
  const [isEditing, setIsEditing] = useState(false);
  const [draftName, setDraftName] = useState(account.profileName);
  const isSwitching = switchingKey === account.accountKey;
  const isRenaming = renamingKey === account.accountKey;

  const tier5h = quota?.tiers.find((t) => t.name === "five_hour");
  const tier7d = quota?.tiers.find((t) => t.name === "seven_day");
  const saveRename = () => {
    const profileName = draftName.trim();
    if (!profileName) return;
    onRename(account.accountKey, profileName);
    setIsEditing(false);
  };
  const formatResetTime = (iso: string | null): string => {
    if (!iso) return "";
    const date = new Date(iso);
    const now = new Date();
    const diffMs = date.getTime() - now.getTime();
    if (diffMs <= 0) {
      return t("codexAccounts.resetSoon", {
        defaultValue: "Reset soon",
      });
    }

    const diffH = Math.floor(diffMs / (1000 * 60 * 60));
    const diffD = Math.floor(diffH / 24);
    if (diffD > 0) {
      return t("codexAccounts.resetInDays", {
        defaultValue: "Resets in {{count}}d",
        count: diffD,
      });
    }
    if (diffH > 0) {
      return t("codexAccounts.resetInHours", {
        defaultValue: "Resets in {{count}}h",
        count: diffH,
      });
    }
    return t("codexAccounts.resetSoon", {
      defaultValue: "Reset soon",
    });
  };

  return (
    <div
      className={cn(
        "rounded-lg border bg-card p-4 transition-colors",
        account.isActive && "border-emerald-500/60 bg-emerald-500/5",
      )}
    >
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="min-w-0 space-y-1">
          <div className="flex flex-wrap items-center gap-2">
            {isEditing ? (
              <Input
                value={draftName}
                onChange={(event) => setDraftName(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    saveRename();
                  }
                  if (event.key === "Escape") {
                    setDraftName(account.profileName);
                    setIsEditing(false);
                  }
                }}
                className="h-8 min-w-0 flex-1 sm:max-w-xs"
                autoFocus
              />
            ) : (
              <h3 className="truncate text-base font-medium tracking-normal">
                {account.profileName}
              </h3>
            )}
            {account.isActive && (
              <Badge
                variant="secondary"
                className="border-emerald-500/30 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400"
              >
                <Check className="mr-1 h-3 w-3" />
                {t("provider.inUse", { defaultValue: "使用中" })}
              </Badge>
            )}
            {account.plan && <Badge variant="outline">{account.plan}</Badge>}
          </div>
          <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
            <span>{account.emailMasked || account.authMode}</span>
            {account.lastUsedAt && (
              <span>
                {new Date(account.lastUsedAt * 1000).toLocaleString()}
              </span>
            )}
          </div>
          {quota && (
            <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs">
              {quota.success ? (
                <>
                  {tier5h && (
                    <span className="flex items-center gap-1">
                      <span className="text-muted-foreground">
                        {t("codexAccounts.fiveHourRemaining", {
                          defaultValue: "5h remaining:",
                        })}
                      </span>
                      <span className={getUtilizationColor(tier5h.utilization)}>
                        {Math.max(0, 100 - Math.round(tier5h.utilization))}%
                      </span>
                      {tier5h.resetsAt && (
                        <span className="text-muted-foreground">
                          · {formatResetTime(tier5h.resetsAt)}
                        </span>
                      )}
                    </span>
                  )}
                  {tier7d && (
                    <span className="flex items-center gap-1">
                      <span className="text-muted-foreground">
                        {t("codexAccounts.sevenDayRemaining", {
                          defaultValue: "7d remaining:",
                        })}
                      </span>
                      <span className={getUtilizationColor(tier7d.utilization)}>
                        {Math.max(0, 100 - Math.round(tier7d.utilization))}%
                      </span>
                      {tier7d.resetsAt && (
                        <span className="text-muted-foreground">
                          · {formatResetTime(tier7d.resetsAt)}
                        </span>
                      )}
                    </span>
                  )}
                  <CodexQuotaForecastBadge forecast={forecast} />
                </>
              ) : (
                <span className="text-muted-foreground">
                  {quota.error ||
                    t("codexAccounts.quotaUnavailable", {
                      defaultValue: "Unable to query usage",
                    })}
                </span>
              )}
            </div>
          )}
        </div>
        <div className="flex w-full shrink-0 items-center gap-1 sm:w-auto">
          {isEditing ? (
            <>
              <Button
                variant="outline"
                size="icon"
                className="h-9 w-9"
                onClick={saveRename}
                disabled={!draftName.trim() || isRenaming}
                aria-label={t("codexAccounts.saveName", {
                  defaultValue: "保存名称",
                })}
              >
                {isRenaming ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <Check className="h-4 w-4" />
                )}
              </Button>
              <Button
                variant="outline"
                size="icon"
                className="h-9 w-9"
                onClick={() => {
                  setDraftName(account.profileName);
                  setIsEditing(false);
                }}
                disabled={isRenaming}
                aria-label={t("common.cancel", { defaultValue: "取消" })}
              >
                <X className="h-4 w-4" />
              </Button>
            </>
          ) : (
            <Button
              variant="outline"
              size="icon"
              className="h-9 w-9"
              onClick={() => {
                setDraftName(account.profileName);
                setIsEditing(true);
              }}
              aria-label={t("codexAccounts.renameAccount", {
                defaultValue: "重命名账号",
              })}
            >
              <Pencil className="h-4 w-4" />
            </Button>
          )}
          <Button
            size="sm"
            disabled={account.isActive || isSwitching}
            onClick={() => onSwitch(account.accountKey)}
            className="flex-1 sm:flex-none"
          >
            {isSwitching ? (
              <Loader2 className="w-4 h-4 animate-spin" />
            ) : account.isActive ? (
              <Check className="w-4 h-4" />
            ) : (
              <RotateCcw className="w-4 h-4" />
            )}
            {account.isActive
              ? t("provider.inUse", { defaultValue: "使用中" })
              : t("codexAccounts.switchTo", { defaultValue: "切换" })}
          </Button>
        </div>
      </div>
    </div>
  );
}
