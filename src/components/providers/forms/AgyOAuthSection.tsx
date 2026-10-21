import React from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { authApi } from "@/lib/api";
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
import { AgyAccountQuota } from "@/components/settings/auth/AccountQuota";

interface AgyOAuthSectionProps {
  /** 授权中心里最后一组的「?」向上弹 */
  helpSide?: "top" | "bottom";
}

/**
 * 授权中心的 Antigravity 账号组。agy 只有一份全局登录（系统钥匙串），「默认」就是 agy 现在
 * 登着的那个号；「切换」把选中的号写进去。在这里删账号只删 CC Switch 的副本，不让 agy 登出。
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
    logout,
    refetchStatus,
  } = useManagedAuth("agy_oauth");
  const accountUsers = useManagedAccountUsers("agy_oauth", defaultAccountId);
  const [removeTarget, setRemoveTarget] =
    React.useState<ManagedAccountRemoveTarget | null>(null);

  const confirmRemove = () => {
    const target = removeTarget;
    setRemoveTarget(null);
    if (!target) return;
    if (target.kind === "all") logout();
    else removeAccount(target.accountId);
  };

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
        : date
          ? [
              t("authCenter.agy.addedOn", {
                defaultValue: "{{date}}添加",
                date,
              }),
            ]
          : [],
      isDefault: defaultAccountId === account.id,
      needsReauth: account.requires_reauth,
      users: accountUsers([account.id]),
      quota: account.requires_reauth ? undefined : (
        <AgyAccountQuota accountId={account.id} login={account.login} />
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
          "Antigravity CLI（agy）的官方登录。agy 同时只能登录一个号，「切换」会把 agy 的登录换成选中的账号，之后新开的 agy 都用它。agy 里原来登着的号会自动收进这里，切走也不会丢。",
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
          "还没有 Antigravity 账号。agy 里已登录的号会自动出现在这里。",
      })}
      loginLabel={t("authCenter.agy.login", {
        defaultValue: "用 Google 登录",
      })}
      onAdd={addAccount}
      defaultLabel={t("authCenter.agy.current", {
        defaultValue: "agy 在用",
      })}
      setDefaultLabel={t("authCenter.agy.switchTo", {
        defaultValue: "切换 agy 到这个账号",
      })}
      canReauth
      // 用同一个 Google 账号再登录一次就会替换掉旧凭据
      onReauth={() => addAccount()}
      onSetDefault={setDefaultAccount}
      settingDefault={isSettingDefaultAccount}
      onRemove={(accountId, login) =>
        setRemoveTarget({ kind: "one", accountId, login })
      }
      onRemoveAll={() =>
        setRemoveTarget({
          kind: "all",
          accountIds: accounts.map((account) => account.id),
        })
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

export default AgyOAuthSection;

interface AgyAccountSelectProps {
  value: string | null;
  onChange: (accountId: string | null) => void;
}

/**
 * Antigravity 官方卡的账号：不选就跟着 agy 当前的登录；选了的话「打开终端」前先把 agy
 * 切到这个号。账号在授权中心添加。
 */
export function AgyAccountSelect({ value, onChange }: AgyAccountSelectProps) {
  const { t } = useTranslation();
  const { data } = useQuery({
    queryKey: ["managed-auth-status", "agy_oauth"],
    queryFn: () => authApi.authGetStatus("agy_oauth"),
    staleTime: 30000,
  });
  const accounts = data?.accounts ?? [];
  // 绑的号在这台电脑上不存在（别的设备同步来的、或已删掉）时也要显示出来，别悄悄改成「跟随」
  const missing = value !== null && !accounts.some((a) => a.id === value);

  return (
    <div className="space-y-2">
      <Label htmlFor="agy-account">
        {t("provider.form.agy.account", { defaultValue: "账号" })}
      </Label>
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
            "选了账号的话，「打开终端」前会先把 agy 的登录切到它（全局生效，之后新开的 agy 都用它）。账号在授权中心添加。",
        })}
      </p>
    </div>
  );
}

const FOLLOW = "__follow__";
