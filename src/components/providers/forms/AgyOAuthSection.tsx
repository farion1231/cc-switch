import React from "react";
import { useTranslation } from "react-i18next";
import { useManagedAuth } from "./hooks/useManagedAuth";
import {
  ManagedAccountRemoveDialog,
  type ManagedAccountRemoveTarget,
} from "./ManagedAccountRemoveDialog";
import {
  ManagedAccountsGroup,
  type GroupAccountRow,
} from "@/components/settings/auth/ManagedAccountsGroup";
import { signedInDate } from "@/components/settings/auth/accountDetails";

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
      users: [],
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
        users={[]}
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
