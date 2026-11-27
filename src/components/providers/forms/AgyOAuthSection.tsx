import React from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { toast } from "@/lib/toast";
import { extractErrorMessage } from "@/utils/errorUtils";
import { authApi } from "@/lib/api";
import { agyAuthStatusQuery } from "@/lib/query/subscription";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useManagedAuth } from "./hooks/useManagedAuth";
import { useManagedAccountUsers } from "./hooks/useManagedAccountUsers";
import {
  ManagedAccountRemoveDialog,
  type ManagedAccountRemoveTarget,
} from "./ManagedAccountRemoveDialog";
import {
  ManagedAccountsGroup,
  type GroupAccountRow,
} from "@/components/settings/auth/ManagedAccountsGroup";
import { signedInDate } from "@/components/settings/auth/accountDetails";
import { useAccountQuotaPlans } from "@/lib/query/accountPlans";
import { AgyAccountQuota } from "@/components/settings/auth/AccountQuota";

interface AgyOAuthSectionProps {
  /** 授权中心里最后一组的「?」向上弹 */
  helpSide?: "top" | "bottom";
}

/**
 * 授权中心的 Antigravity 账号组。agy 只有一份全局登录（系统钥匙串），「默认」就是 agy 现在
 * 登着的那个号；「切换」把选中的号写进去。在这里删账号只删 CC Switch 的副本，不让 agy 登出。
 * 「添加账号」打开终端运行 agy，用户在 agy 里登录（CC Switch 不用 agy 的 OAuth 客户端），
 * 钥匙串里的登录换了就收录。额度只问得到 agy 当前登着的号。
 */
export const AgyOAuthSection: React.FC<AgyOAuthSectionProps> = ({
  helpSide,
}) => {
  const { t, i18n } = useTranslation();
  const {
    accounts,
    defaultAccountId,
    isStatusSuccess,
    isStatusError,
    pollingState,
    deviceCode,
    error,
    isPolling,
    isAddingAccount,
    isRemovingAccount,
    isSettingDefaultAccount,
    addAccount,
    removeAccount,
    setDefaultAccount,
    cancelAuth,
    refetchStatus,
  } = useManagedAuth("agy_oauth");
  const accountUsers = useManagedAccountUsers("agy_oauth", defaultAccountId);
  const [removeTarget, setRemoveTarget] =
    React.useState<ManagedAccountRemoveTarget | null>(null);

  const confirmRemove = () => {
    const target = removeTarget;
    setRemoveTarget(null);
    // 这组没有「删除全部」，只会是单个账号
    if (target?.kind === "one") removeAccount(target.accountId);
  };

  // 第二行的套餐（额度查到才有）
  const plans = useAccountQuotaPlans(
    "agy_oauth",
    accounts.map((account) => account.id),
  );
  const rows: GroupAccountRow[] = accounts.map((account) => {
    const date = signedInDate(account.authenticated_at, i18n.language);
    return {
      id: account.id,
      login: account.login,
      avatarUrl: account.avatar_url,
      details: account.requires_reauth
        ? [
            t("authCenter.agy.reauthNote", {
              defaultValue: "登录凭据已失效，切不过去",
            }),
          ]
        : [
            plans[account.id],
            date
              ? t("authCenter.agy.addedOn", {
                  defaultValue: "{{date}}添加",
                  date,
                })
              : null,
          ].filter(Boolean),
      isDefault: defaultAccountId === account.id,
      needsReauth: account.requires_reauth,
      users: accountUsers([account.id]),
      quota: account.requires_reauth ? undefined : (
        <AgyAccountQuota
          accountId={account.id}
          login={account.login}
          isCurrent={defaultAccountId === account.id}
        />
      ),
    };
  });

  return (
    <ManagedAccountsGroup
      slug="agy"
      name="Antigravity"
      iconName="antigravity"
      help={t("authCenter.group.agyHelp", {
        defaultValue:
          "Antigravity CLI（agy）的官方登录。agy 同时只能登录一个号，「切换」会把 agy 的登录换成选中的账号，之后新开的 agy 都用它。agy 里原来登着的号会自动收进这里，切走也不会丢。「添加账号」会打开终端运行 agy，在 agy 里登录后这里自动收录。额度用 agy 自己的 /usage 查，只查得到 agy 当前登着的号，其他号显示切走前最后一次查到的。",
      })}
      helpSide={helpSide}
      accounts={rows}
      status={isStatusError ? "error" : isStatusSuccess ? "ready" : "loading"}
      statusErrorText={t("authCenter.agy.statusLoadFailed", {
        defaultValue: "无法读取 Antigravity 账号，请重试。",
      })}
      onRetryStatus={() => void refetchStatus()}
      emptyText={t("authCenter.agy.empty", {
        defaultValue:
          "还没有 Antigravity 账号。agy 里已登录的号会自动出现在这里，也可以点「添加账号」在 agy 里登录。",
      })}
      onAdd={addAccount}
      defaultLabel={t("authCenter.inUseBy", {
        defaultValue: "{{app}} 在用",
        app: "agy",
      })}
      setDefaultLabel={t("authCenter.switchAppTo", {
        defaultValue: "切换 {{app}} 到这个账号",
        app: "agy",
      })}
      canReauth
      // 用同一个 Google 账号再登录一次就会替换掉旧凭据
      onReauth={() => addAccount()}
      onSetDefault={setDefaultAccount}
      settingDefault={isSettingDefaultAccount}
      onRemove={(accountId, login) =>
        setRemoveTarget({ kind: "one", accountId, login })
      }
      removing={isRemovingAccount}
      login={{
        starting: isAddingAccount && !isPolling,
        polling: isPolling,
        pollingState,
        deviceCode,
        error,
        onCancel: cancelAuth,
        onRetry: addAccount,
        pendingNote: t("authCenter.agy.pendingNote", {
          defaultValue:
            "已在终端打开 agy。没登录时按提示登录；已登录时先输入 /logout，再输入 /login 登录要添加的号。完成后这里会自动收录。",
        }),
        reopenLabel: t("authCenter.agy.reopenTerminal", {
          defaultValue: "重新打开终端",
        }),
        onReopen: () => {
          authApi.agyAccountOpenLoginTerminal().catch((e: unknown) => {
            toast.error(extractErrorMessage(e) || String(e));
          });
        },
      }}
    >
      <ManagedAccountRemoveDialog
        target={removeTarget}
        serviceName="Antigravity"
        users={
          removeTarget
            ? accountUsers(
                removeTarget.kind === "one"
                  ? [removeTarget.accountId]
                  : removeTarget.accountIds,
              )
            : []
        }
        othersRemain={accounts.length > 1}
        note={t("authCenter.agy.removeNote", {
          defaultValue: "不会让 agy 退出登录：agy 现在登着的号删掉后仍然可用。",
        })}
        pending={isRemovingAccount}
        onConfirm={confirmRemove}
        onCancel={() => setRemoveTarget(null)}
      />
    </ManagedAccountsGroup>
  );
};

interface AgyAccountSelectProps {
  value: string | null;
  onChange: (accountId: string | null) => void;
}

/**
 * 绑定变更在保存时生效。没绑定账号的卡跟着 agy 当前登录。
 */
export function AgyAccountSelect({ value, onChange }: AgyAccountSelectProps) {
  const { t } = useTranslation();
  const { data } = useQuery(agyAuthStatusQuery);
  const accounts = data?.accounts ?? [];
  const activeAccount = accounts.find(
    (account) => account.id === data?.default_account_id,
  );
  const inUse = t("authCenter.inUseBy", { app: "agy" });
  const [initialValue] = React.useState(value);
  const willSwitch =
    value !== null && value !== initialValue && value !== activeAccount?.id;
  const { data: agyRunning } = useQuery({
    queryKey: ["agy-running"],
    queryFn: () => authApi.agyAccountIsRunning(),
    enabled: willSwitch,
    staleTime: 0,
  });
  // 绑的号在这台电脑上不存在（别的设备同步来的、或已删掉）时也要显示出来，别悄悄改成「跟随」
  const missing = value !== null && !accounts.some((a) => a.id === value);

  return (
    <div className="space-y-2">
      <div className="flex min-w-0 items-center justify-between gap-2">
        <Label htmlFor="agy-account">
          {t("provider.form.agy.account", { defaultValue: "账号" })}
        </Label>
        {activeAccount && (
          <span
            className="truncate text-caption text-fg-2"
            title={activeAccount.login}
          >
            {inUse} · {activeAccount.login}
          </span>
        )}
      </div>
      <Select
        value={value ?? FOLLOW}
        onValueChange={(next) => {
          // Radix Select 在表单里可能回报空串，忽略
          if (!next) return;
          onChange(next === FOLLOW ? null : next);
        }}
      >
        <SelectTrigger id="agy-account">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={FOLLOW}>
            {t("provider.form.agy.accountFollow", {
              defaultValue: "跟随 agy 当前登录",
            })}
          </SelectItem>
          {accounts.map((account) => (
            <SelectItem
              key={account.id}
              value={account.id}
              disabled={account.requires_reauth}
            >
              {account.login}
            </SelectItem>
          ))}
          {missing && (
            <SelectItem value={value}>
              {t("provider.form.agy.accountMissing", {
                defaultValue: "不在这台电脑上的账号",
              })}
            </SelectItem>
          )}
        </SelectContent>
      </Select>
      <p className="text-caption text-fg-2">
        {t("provider.form.agy.accountHelp", {
          defaultValue:
            "保存新的绑定账号后，新开的 agy 会使用它。用这张卡打开终端时也会切回绑定账号。账号在授权中心添加。",
        })}
      </p>
      {willSwitch && agyRunning && (
        <p className="text-caption text-warning-text">
          {t("provider.openTerminalDialog.agySwitchRunning")}
        </p>
      )}
    </div>
  );
}

const FOLLOW = "__follow__";
