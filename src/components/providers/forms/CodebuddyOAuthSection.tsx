import React from "react";
import { useTranslation } from "react-i18next";
import { useCodebuddyOauth } from "./hooks/useCodebuddyOauth";
import {
  ManagedAccountRemoveDialog,
  type ManagedAccountRemoveTarget,
} from "./ManagedAccountRemoveDialog";
import { useManagedAccountUsers } from "./hooks/useManagedAccountUsers";
import {
  ManagedAccountsGroup,
  type GroupAccountRow,
} from "@/components/settings/auth/ManagedAccountsGroup";
import { CodebuddyAccountQuota } from "@/components/settings/auth/AccountQuota";
import { signedInDate } from "@/components/settings/auth/accountDetails";

interface CodebuddyOAuthSectionProps {
  /** 授权中心里最后一组的「?」向上弹 */
  helpSide?: "top" | "bottom";
}

/**
 * CodeBuddy（国内站 copilot.tencent.com）账号区，只用于授权中心。
 *
 * 负责登录、多账号、默认账号、移除、重新登录和额度展示。
 */
export const CodebuddyOAuthSection: React.FC<CodebuddyOAuthSectionProps> = ({
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
  } = useCodebuddyOauth();
  const accountUsers = useManagedAccountUsers(
    "codebuddy_oauth",
    defaultAccountId,
  );
  const [removeTarget, setRemoveTarget] =
    React.useState<ManagedAccountRemoveTarget | null>(null);

  // 删账号先确认：确认框里列出在用它的供应商
  const confirmRemove = () => {
    const target = removeTarget;
    setRemoveTarget(null);
    if (!target) return;
    if (target.kind === "all") {
      logout();
      return;
    }
    removeAccount(target.accountId);
  };

  const removeDialog = (
    <ManagedAccountRemoveDialog
      target={removeTarget}
      serviceName="CodeBuddy"
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
      pending={isRemovingAccount}
      onConfirm={confirmRemove}
      onCancel={() => setRemoveTarget(null)}
    />
  );

  const rows: GroupAccountRow[] = accounts.map((account) => {
    const date = signedInDate(account.authenticated_at, i18n.language);
    const needsReauth = account.requires_reauth;
    const details: React.ReactNode[] = [];
    if (needsReauth) {
      details.push(
        t("authCenter.codebuddyReauthNote", {
          defaultValue: "登录凭据已失效，请重新登录 CodeBuddy 账号",
        }),
      );
    } else {
      if (date) {
        details.push(
          t("authCenter.signedInOn", {
            defaultValue: "{{date}}登录",
            date,
          }),
        );
      }
      // 企业名 / 邮箱后端能拿到才显示
      if (account.enterprise_name) details.push(account.enterprise_name);
      if (account.email) details.push(account.email);
    }
    return {
      id: account.id,
      login: account.login,
      details,
      isDefault: defaultAccountId === account.id,
      needsReauth,
      users: accountUsers([account.id]),
      quota: needsReauth ? undefined : (
        <CodebuddyAccountQuota accountId={account.id} login={account.login} />
      ),
    };
  });

  return (
    <ManagedAccountsGroup
      slug="codebuddy"
      name="CodeBuddy"
      iconName="codebuddy"
      help={t("authCenter.group.codebuddyHelp", {
        defaultValue:
          "用于 CodeBuddy（腾讯 copilot.tencent.com）订阅：登录账号并查看剩余额度。",
      })}
      helpSide={helpSide}
      accounts={rows}
      status={isStatusError ? "error" : isStatusSuccess ? "ready" : "loading"}
      statusErrorText={t("authCenter.codebuddyStatusLoadFailed", {
        defaultValue: "无法加载 CodeBuddy 账号状态，请重试。",
      })}
      onRetryStatus={() => void refetchStatus()}
      emptyText={t("authCenter.empty", {
        defaultValue: "还没有登录 {{service}} 账号。",
        service: "CodeBuddy",
      })}
      loginLabel={t("codebuddyOauth.login", "使用 CodeBuddy 登录")}
      onAdd={addAccount}
      canReauth
      // 后端登录总是按账号标识覆盖旧凭据：发起一次普通登录，用同一个 CodeBuddy 账号登录即可替换
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
      {removeDialog}
    </ManagedAccountsGroup>
  );
};

export default CodebuddyOAuthSection;
