import { useEffect } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ExternalLink } from "lucide-react";
import { toast } from "@/lib/toast";
import { authApi } from "@/lib/api";
import type { ManagedAuthAccount } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Notice } from "@/components/ui/notice";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { extractErrorMessage } from "@/utils/errorUtils";
import { WORKBUDDY_ACCOUNT_AUTH_PROVIDER } from "@/config/workbuddyProviderPresets";
import {
  WORKBUDDY_LIVE_KEY,
  WORKBUDDY_STATUS_KEY,
} from "@/components/providers/workbuddyOfficial";
import { openWorkBuddyApp } from "@/components/providers/useWorkBuddyOfficial";
import { workbuddyPlanLabel } from "@/components/settings/auth/workbuddyPlan";
import { WorkBuddyAccountQuota } from "@/components/settings/auth/AccountQuota";

interface WorkBuddyOfficialSectionProps {
  /** 绑定的账号 id；null 表示还没选 */
  value: string | null;
  onChange: (account: ManagedAuthAccount) => void;
}

/**
 * 「WorkBuddy 官方登录」卡的账号区：从授权中心已收录的 WorkBuddy 账号里选一个绑定。没有账号
 * 或要加新号时，「登录另一个账号」让 WorkBuddy 下次打开停在登录页，登录后会自动收录进列表。
 */
export function WorkBuddyOfficialSection({
  value,
  onChange,
}: WorkBuddyOfficialSectionProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  // 读状态时后端会自动收录 WorkBuddy 当前登录的号：轮询让刚登录的号出现在下拉里
  const { data: status, isSuccess } = useQuery({
    queryKey: WORKBUDDY_STATUS_KEY,
    queryFn: () => authApi.authGetStatus(WORKBUDDY_ACCOUNT_AUTH_PROVIDER),
    staleTime: 30_000,
    refetchInterval: 5_000,
  });
  const { data: live } = useQuery({
    queryKey: WORKBUDDY_LIVE_KEY,
    queryFn: authApi.workbuddyAccountLiveState,
    refetchInterval: 5_000,
  });
  const accounts = status?.accounts ?? [];
  const currentId = status?.default_account_id ?? null;
  const missing =
    value !== null && isSuccess && !accounts.some((a) => a.id === value);
  const selected = accounts.find((account) => account.id === value) ?? null;

  // 新卡还没选账号时默认选 WorkBuddy 现在登着的号（没有就选第一个能用的）
  useEffect(() => {
    if (value !== null || accounts.length === 0) return;
    const usable = accounts.filter((account) => !account.requires_reauth);
    const pick =
      usable.find((account) => account.id === currentId) ?? usable[0];
    if (pick) onChange(pick);
  }, [value, accounts, currentId, onChange]);

  const refresh = () =>
    Promise.all([
      queryClient.invalidateQueries({ queryKey: WORKBUDDY_STATUS_KEY }),
      queryClient.invalidateQueries({ queryKey: WORKBUDDY_LIVE_KEY }),
    ]);

  const prepare = useMutation({
    mutationFn: authApi.workbuddyAccountPrepareNewLogin,
    onSuccess: async () => {
      toast.success(t("workbuddy.official.preparedNewLogin"), {
        closeButton: true,
      });
      await refresh();
      await openWorkBuddyApp();
    },
    onError: async (error) => {
      toast.error(extractErrorMessage(error) || String(error));
      await refresh();
    },
  });

  const accountLabel = (account: ManagedAuthAccount) =>
    account.account_kind
      ? `${account.login} · ${workbuddyPlanLabel(t, account.account_kind)}`
      : account.login;

  return (
    <div className="space-y-3">
      <div className="space-y-2">
        <Label htmlFor="workbuddy-official-account">
          {t("workbuddy.official.account")}
        </Label>
        <Select
          value={value ?? undefined}
          onValueChange={(next) => {
            // Radix Select 在表单里可能回报空串，忽略
            const account = accounts.find((item) => item.id === next);
            if (account) onChange(account);
          }}
          disabled={accounts.length === 0 && !missing}
        >
          <SelectTrigger id="workbuddy-official-account">
            <SelectValue
              placeholder={
                isSuccess && accounts.length === 0
                  ? t("workbuddy.official.noAccounts")
                  : t("workbuddy.official.chooseAccount")
              }
            />
          </SelectTrigger>
          <SelectContent>
            {accounts.map((account) => (
              <SelectItem
                key={account.id}
                value={account.id}
                disabled={account.requires_reauth}
              >
                {accountLabel(account)}
                {account.id === currentId
                  ? ` (${t("authCenter.workbuddy.current")})`
                  : ""}
              </SelectItem>
            ))}
            {missing && value && (
              <SelectItem value={value} disabled>
                {t("workbuddy.official.accountDeleted")}
              </SelectItem>
            )}
          </SelectContent>
        </Select>
        <p
          className={
            missing
              ? "text-caption text-warning-text"
              : "text-caption text-fg-2"
          }
        >
          {missing
            ? t("workbuddy.official.accountDeleted")
            : t("workbuddy.official.accountHelp")}
        </p>
        {selected && (
          <div className="flex items-center gap-3 rounded-control border border-border px-3 py-2">
            <span className="min-w-0 flex-1 truncate text-caption text-fg-2">
              {t("workbuddy.quota.title")}
            </span>
            <WorkBuddyAccountQuota
              accountId={selected.id}
              login={selected.login}
            />
          </div>
        )}
      </div>

      <Notice
        tone="neutral"
        title={
          isSuccess && accounts.length === 0
            ? t("workbuddy.official.emptyTitle")
            : t("workbuddy.official.addAnotherTitle")
        }
        actions={
          <>
            <Button
              type="button"
              variant="neutral"
              size="compact"
              disabled={prepare.isPending || live?.running === true}
              onClick={() => prepare.mutate()}
            >
              {t("authCenter.workbuddy.prepareNewLogin")}
            </Button>
            <Button
              type="button"
              variant="quiet"
              size="compact"
              className="gap-1"
              onClick={() => void openWorkBuddyApp()}
            >
              <ExternalLink className="h-3.5 w-3.5" strokeWidth={1.5} />
              {t("authCenter.workbuddy.openApp")}
            </Button>
          </>
        }
      >
        {live?.running
          ? t("workbuddy.official.addAnotherRunning")
          : isSuccess && accounts.length === 0
            ? t("workbuddy.official.emptyBody")
            : t("workbuddy.official.addAnotherBody")}
      </Notice>
    </div>
  );
}
