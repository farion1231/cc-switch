import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  CheckCircle2,
  Loader2,
  Plus,
  RefreshCw,
  Terminal,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { authApi, type ClaudeOfficialAccount } from "@/lib/api/auth";

interface ClaudeOfficialAuthSectionProps {
  className?: string;
  selectedAccountId?: string | null;
  onAccountSelect?: (accountId: string | null) => void;
  mode?: "provider" | "settings";
}

const queryKey = ["claude-official-accounts"];

function formatAccountDisplay(account: ClaudeOfficialAccount): string {
  if (!account.email) {
    return account.label;
  }
  if (account.label && account.label !== account.email) {
    return `${account.email} (${account.label})`;
  }
  return account.email;
}

function formatQuotaTierName(name: string): string {
  switch (name) {
    case "five_hour":
      return "5小时";
    case "seven_day":
      return "7天";
    case "seven_day_opus":
      return "7天 Opus";
    case "seven_day_sonnet":
      return "7天 Sonnet";
    default:
      return name.replace(/^seven_day_/, "7天 ").replace(/_/g, " ");
  }
}

function getTierTone(utilization: number): string {
  if (utilization >= 95) {
    return "border-red-200 bg-red-50 text-red-700";
  }
  if (utilization >= 80) {
    return "border-amber-200 bg-amber-50 text-amber-700";
  }
  return "border-emerald-200 bg-emerald-50 text-emerald-700";
}

function getProgressTone(utilization: number): string {
  if (utilization >= 95) {
    return "bg-red-500";
  }
  if (utilization >= 80) {
    return "bg-amber-500";
  }
  return "bg-emerald-500";
}

function getPrimaryQuotaTiers(account: ClaudeOfficialAccount) {
  const tiers = account.quota?.tiers ?? [];
  const preferred = ["five_hour", "seven_day", "seven_day_sonnet", "seven_day_opus"];

  return preferred
    .map((name) => tiers.find((tier) => tier.name === name))
    .filter(Boolean)
    .slice(0, 3) as NonNullable<ClaudeOfficialAccount["quota"]>["tiers"];
}

function getSecondaryQuotaTiers(account: ClaudeOfficialAccount) {
  const primaryNames = new Set(getPrimaryQuotaTiers(account).map((tier) => tier.name));
  return (account.quota?.tiers ?? []).filter((tier) => !primaryNames.has(tier.name));
}

function formatQuotaSummary(account: ClaudeOfficialAccount): string | null {
  const quota = account.quota;
  if (!quota?.success || quota.tiers.length === 0) {
    return null;
  }

  return quota.tiers
    .map((tier) => `${formatQuotaTierName(tier.name)} ${tier.utilization.toFixed(0)}%`)
    .join(" / ");
}

export function ClaudeOfficialAuthSection({
  className,
  selectedAccountId,
  onAccountSelect,
  mode = "provider",
}: ClaudeOfficialAuthSectionProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [label, setLabel] = useState("");
  const [activatedAccountId, setActivatedAccountId] = useState<string | null>(
    null,
  );

  const { data: accounts = [], isLoading } = useQuery<ClaudeOfficialAccount[]>({
    queryKey,
    queryFn: authApi.claudeOfficialListAccounts,
    staleTime: 30000,
  });

  const loginMutation = useMutation({
    mutationFn: authApi.claudeOfficialStartLogin,
    onSuccess: () => {
      toast.success(
        t("claudeOfficial.loginStarted", {
          defaultValue: "已打开终端并发送 Claude /login，登录完成后再保存当前登录。",
        }),
      );
    },
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : String(error));
    },
  });

  const captureMutation = useMutation({
    mutationFn: () => authApi.claudeOfficialCaptureCurrentAccount(label),
    onSuccess: async (account) => {
      setLabel("");
      onAccountSelect?.(account.id);
      setActivatedAccountId(account.id);
      await queryClient.invalidateQueries({ queryKey });
      toast.success(
        t("claudeOfficial.captureSuccess", {
          defaultValue: "已保存并激活：{{account}}",
          account: account.email || account.label,
        }),
      );
    },
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : String(error));
    },
  });

  const activateMutation = useMutation({
    mutationFn: (accountId: string) =>
      authApi.claudeOfficialActivateAccount(accountId),
    onSuccess: async (_, accountId) => {
      setActivatedAccountId(accountId);
      await queryClient.invalidateQueries({ queryKey });
      const account = accounts.find((item) => item.id === accountId);
      toast.success(
        t("claudeOfficial.activateSuccess", {
          defaultValue: "已激活 Claude 官方账号：{{account}}",
          account: account?.email || account?.label || accountId,
        }),
      );
    },
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : String(error));
    },
  });

  const removeMutation = useMutation({
    mutationFn: (accountId: string) =>
      authApi.claudeOfficialRemoveAccount(accountId),
    onSuccess: async (_, accountId) => {
      if (selectedAccountId === accountId) {
        onAccountSelect?.(null);
      }
      await queryClient.invalidateQueries({ queryKey });
      toast.success(
        t("claudeOfficial.removeSuccess", {
          defaultValue: "已移除 Claude 官方账号快照",
        }),
      );
    },
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : String(error));
    },
  });

  const refreshMutation = useMutation({
    mutationFn: (accountId: string) =>
      authApi.claudeOfficialRefreshAccountQuota(accountId),
    onSuccess: async (account) => {
      await queryClient.invalidateQueries({ queryKey });
      toast.success(
        t("claudeOfficial.refreshSuccess", {
          defaultValue: "已更新 {{email}} 的官方用量",
          email: account.email || account.label,
        }),
      );
    },
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : String(error));
    },
  });

  const selected = selectedAccountId ?? "";
  const selectedAccount = accounts.find((account) => account.id === selected);
  const activatedAccount = accounts.find(
    (account) => account.id === activatedAccountId,
  );

  const renderAccountRows = () => (
    <div className="space-y-2 rounded-xl border bg-muted/20 p-2">
      <div className="flex items-center justify-between px-2 pb-1 text-xs text-muted-foreground">
        <span className="font-medium">
          {t("claudeOfficial.emailColumn", { defaultValue: "邮箱" })}
        </span>
        <span>
          {t("claudeOfficial.accountCount", {
            defaultValue: "{{count}} 个账号",
            count: accounts.length,
          })}
        </span>
      </div>
      {accounts.length === 0 ? (
        <div className="rounded-lg bg-background px-3 py-4 text-sm text-muted-foreground">
          {isLoading
            ? t("common.loading", { defaultValue: "加载中..." })
            : t("claudeOfficial.emptyAccounts", {
                defaultValue: "暂无 Claude 官方账号，请先保存并激活当前登录。",
              })}
        </div>
      ) : (
        accounts.map((account) => {
          const primaryTiers = getPrimaryQuotaTiers(account);
          const secondaryTiers = getSecondaryQuotaTiers(account);

          return (
          <div
            key={account.id}
            className="group rounded-lg border bg-background/90 p-3 shadow-sm transition-colors hover:bg-background"
          >
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0 space-y-2">
                <div>
                  <div className="truncate text-sm font-semibold text-foreground">
                    {account.email || account.label}
                  </div>
                  {account.email && account.label && account.label !== account.email && (
                    <div className="truncate text-xs text-muted-foreground">
                      {account.label}
                    </div>
                  )}
                </div>

                {primaryTiers.length > 0 ? (
                  <div className="flex flex-wrap gap-2">
                    {primaryTiers.map((tier) => (
                      <div
                        key={tier.name}
                        className={`min-w-[104px] rounded-md border px-2 py-1 ${getTierTone(
                          tier.utilization,
                        )}`}
                      >
                        <div className="flex items-center justify-between gap-2 text-[11px] font-medium">
                          <span>{formatQuotaTierName(tier.name)}</span>
                          <span>{tier.utilization.toFixed(0)}%</span>
                        </div>
                        <div className="mt-1 h-1.5 overflow-hidden rounded-full bg-current/15">
                          <div
                            className={`h-full rounded-full ${getProgressTone(
                              tier.utilization,
                            )}`}
                            style={{
                              width: `${Math.min(Math.max(tier.utilization, 0), 100)}%`,
                            }}
                          />
                        </div>
                      </div>
                    ))}
                  </div>
                ) : (
                  <div className="text-xs text-muted-foreground">
                    {t("claudeOfficial.noQuota", {
                      defaultValue: "尚未查询用量",
                    })}
                  </div>
                )}

                {secondaryTiers.length > 0 && (
                  <div className="flex flex-wrap gap-1.5 text-[11px] text-muted-foreground">
                    {secondaryTiers.map((tier) => (
                      <span key={tier.name} className="rounded-full bg-muted px-2 py-0.5">
                        {formatQuotaTierName(tier.name)} {tier.utilization.toFixed(0)}%
                      </span>
                    ))}
                  </div>
                )}
              </div>

              <div className="flex shrink-0 items-center gap-1.5">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={refreshMutation.isPending}
                  onClick={() => refreshMutation.mutate(account.id)}
                  className="h-8 gap-1.5 px-2.5"
                >
                  {refreshMutation.isPending ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : (
                    <RefreshCw className="h-3.5 w-3.5" />
                  )}
                  {t("claudeOfficial.query", { defaultValue: "查询" })}
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="icon"
                  disabled={removeMutation.isPending}
                  onClick={() => removeMutation.mutate(account.id)}
                  title={t("claudeOfficial.remove", { defaultValue: "删除" })}
                  className="h-8 w-8 text-muted-foreground hover:text-destructive"
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
            </div>
          </div>
          );
        })
      )}
    </div>
  );

  return (
    <div className={className ?? "space-y-3 rounded-lg border p-4"}>
      <div className="space-y-1">
        <div className="flex items-center gap-2 text-sm font-medium">
          <CheckCircle2 className="h-4 w-4 text-primary" />
          {t("claudeOfficial.title", {
            defaultValue: "Claude 官方账号快照",
          })}
        </div>
        <p className="text-xs text-muted-foreground">
          {t("claudeOfficial.description", {
            defaultValue:
              "先打开 Claude 登录并完成 /login。保存时会校验邮箱和官方用量，成功后才加入账号列表并自动激活。",
          })}
        </p>
      </div>

      <div className="flex gap-2">
        <Input
          value={label}
          onChange={(event) => setLabel(event.target.value)}
          placeholder={t("claudeOfficial.labelPlaceholder", {
            defaultValue: "账号备注，例如 Max 主号",
          })}
          autoComplete="off"
        />
        <Button
          type="button"
          variant="outline"
          onClick={() => loginMutation.mutate()}
          disabled={loginMutation.isPending}
          className="shrink-0 gap-1.5"
        >
          {loginMutation.isPending ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <Terminal className="h-4 w-4" />
          )}
          {t("claudeOfficial.startLogin", {
            defaultValue: "打开 Claude 登录",
          })}
        </Button>
        <Button
          type="button"
          onClick={() => captureMutation.mutate()}
          disabled={captureMutation.isPending}
          className="shrink-0 gap-1.5"
        >
          {captureMutation.isPending ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <Plus className="h-4 w-4" />
          )}
          {t("claudeOfficial.captureCurrent", {
            defaultValue: "保存并激活",
          })}
        </Button>
      </div>

      {mode === "settings" ? (
        renderAccountRows()
      ) : (
        <div className="flex gap-2">
          <Select
            value={selected || undefined}
            onValueChange={(value) => onAccountSelect?.(value)}
            disabled={isLoading || accounts.length === 0}
          >
            <SelectTrigger className="flex-1">
              <SelectValue
                placeholder={
                  isLoading
                    ? t("common.loading", { defaultValue: "加载中..." })
                    : t("claudeOfficial.selectPlaceholder", {
                        defaultValue: "选择一个官方账号快照",
                      })
                }
              />
            </SelectTrigger>
            <SelectContent>
              {accounts.map((account) => (
                <SelectItem key={account.id} value={account.id}>
                  <span className="flex min-w-0 flex-col">
                    <span className="truncate">{formatAccountDisplay(account)}</span>
                    {formatQuotaSummary(account) && (
                      <span className="truncate text-xs text-muted-foreground">
                        {formatQuotaSummary(account)}
                      </span>
                    )}
                  </span>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button
            type="button"
            variant="outline"
            disabled={!selected || activateMutation.isPending}
            onClick={() => selected && activateMutation.mutate(selected)}
          >
            {activateMutation.isPending ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              t("claudeOfficial.activate", { defaultValue: "激活" })
            )}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="icon"
            disabled={!selected || removeMutation.isPending}
            onClick={() => selected && removeMutation.mutate(selected)}
            title={t("claudeOfficial.remove", { defaultValue: "移除" })}
          >
            <Trash2 className="h-4 w-4" />
          </Button>
        </div>
      )}

      {activatedAccount?.email && (
        <p className="text-xs font-medium text-primary">
          {t("claudeOfficial.activeAccount", {
            defaultValue: "已激活：{{email}}",
            email: activatedAccount.email,
          })}
        </p>
      )}

      {selectedAccount && (
        <div className="space-y-1 text-xs text-muted-foreground">
          {formatQuotaSummary(selectedAccount) && (
            <p>
              {t("claudeOfficial.quotaSummary", {
                defaultValue: "官方用量：{{summary}}",
                summary: formatQuotaSummary(selectedAccount),
              })}
            </p>
          )}
          <p>
            {t("claudeOfficial.storageKind", {
              defaultValue: "来源：{{kind}}",
              kind: selectedAccount.storageKind,
            })}
          </p>
        </div>
      )}
    </div>
  );
}

export default ClaudeOfficialAuthSection;
