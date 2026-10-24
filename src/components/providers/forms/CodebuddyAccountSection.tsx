import React from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { authApi } from "@/lib/api";
import { toast } from "@/lib/toast";
import { Button } from "@/components/ui/button";
import { Notice } from "@/components/ui/notice";
import {
  CODEBUDDY_LIVE_KEY,
  CodebuddyLoginConfirmDialog,
  useCodebuddyLogin,
  useCodebuddyRegionLabel,
} from "./CodebuddyLogin";
import {
  ManagedAccountRemoveDialog,
  type ManagedAccountRemoveTarget,
} from "./ManagedAccountRemoveDialog";
import {
  ManagedAccountsGroup,
  type GroupAccountRow,
} from "@/components/settings/auth/ManagedAccountsGroup";
import { signedInDate } from "@/components/settings/auth/accountDetails";
import { CodebuddyAccountQuota } from "@/components/settings/auth/AccountQuota";

/** 账号 id 是 `<uid>@<domain>`。 */
const uidOf = (id: string) => {
  const at = id.lastIndexOf("@");
  return at < 0 ? id : id.slice(0, at);
};

interface CodebuddyAccountSectionProps {
  helpSide?: "top" | "bottom";
}

/**
 * 授权中心的 CodeBuddy Code 账号组。CodeBuddy 只有一份全局登录（凭据文件），「在用」就是它现在
 * 登着的那个号。这里只管账号（收录、终端登录、删除、额度）；切换在供应商列表里：给账号建一张
 * 「CodeBuddy 官方登录」卡，点卡片上的「添加」。删账号只删 CC Switch 的副本。
 * 「添加账号」在已登录时会先确认，再让 CodeBuddy 暂时退出登录（见 `useCodebuddyLogin`）。
 */
export const CodebuddyAccountSection: React.FC<
  CodebuddyAccountSectionProps
> = ({ helpSide }) => {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const login = useCodebuddyLogin();
  const { live } = login;
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
    removeAccount,
    refetchStatus,
  } = login.auth;
  const [removeTarget, setRemoveTarget] =
    React.useState<ManagedAccountRemoveTarget | null>(null);
  // 账号表变了（终端登录收录、删除）或 CodeBuddy 换了号时凭据文件的状态也跟着变
  const accountKey = accounts.map((account) => account.id).join(",");
  React.useEffect(() => {
    void queryClient.invalidateQueries({ queryKey: CODEBUDDY_LIVE_KEY });
  }, [accountKey, defaultAccountId, queryClient]);

  const importCurrent = useMutation({
    mutationFn: authApi.codebuddyAccountImportCurrent,
    onSuccess: async (account) => {
      toast.success(
        t("authCenter.codebuddy.imported", {
          defaultValue: "已收录 {{login}}。",
          login: account.login,
        }),
      );
      await refetchStatus();
      await queryClient.invalidateQueries({ queryKey: CODEBUDDY_LIVE_KEY });
    },
    onError: (e) => {
      toast.error(e instanceof Error ? e.message : String(e));
    },
  });

  const regionLabel = useCodebuddyRegionLabel();

  const confirmRemove = () => {
    const target = removeTarget;
    setRemoveTarget(null);
    if (target?.kind === "one") removeAccount(target.accountId);
  };

  const rows: GroupAccountRow[] = accounts.map((account) => {
    const uid = uidOf(account.id);
    const date = signedInDate(account.authenticated_at, i18n.language);
    const details: React.ReactNode[] = [regionLabel(account.github_domain)];
    if (account.login !== uid) details.push(`UID ${uid}`);
    if (account.requires_reauth) {
      details.push(
        t("authCenter.codebuddy.expiredNote", {
          defaultValue: "登录已过期，需要重新登录",
        }),
      );
    } else if (date) {
      details.push(
        t("authCenter.codebuddy.addedOn", {
          defaultValue: "{{date}}收录",
          date,
        }),
      );
    }
    return {
      id: account.id,
      login: account.login,
      avatarUrl: null,
      details: details.filter(Boolean),
      isDefault: defaultAccountId === account.id,
      needsReauth: account.requires_reauth,
      users: [],
      quota: (
        <CodebuddyAccountQuota accountId={account.id} login={account.login} />
      ),
    };
  });

  const notice =
    live?.state === "uncollected" ? (
      <Notice
        title={t("authCenter.codebuddy.uncollected", {
          defaultValue: "CodeBuddy 现在登着 {{login}}，还没收录到这里。",
          login: [live.label, regionLabel(live.domain)]
            .filter(Boolean)
            .join(" · "),
        })}
        actions={
          <Button
            type="button"
            variant="neutral"
            size="compact"
            disabled={importCurrent.isPending}
            onClick={() => importCurrent.mutate()}
          >
            {t("authCenter.codebuddy.importCurrent", {
              defaultValue: "收录当前账号",
            })}
          </Button>
        }
      />
    ) : live?.state === "unrecognized" ? (
      <Notice
        tone="warning"
        title={t("authCenter.codebuddy.unrecognized", {
          defaultValue: "CodeBuddy 的登录文件格式认不出",
        })}
      >
        {t("authCenter.codebuddy.unrecognizedNote", {
          defaultValue:
            "可能由 WorkBuddy 加密托管。CC Switch 不会改动它，也不能在这里切换账号。",
        })}
      </Notice>
    ) : undefined;

  return (
    <ManagedAccountsGroup
      slug="codebuddy"
      name="CodeBuddy Code"
      iconName="codebuddy"
      help={t("authCenter.group.codebuddyHelp", {
        defaultValue:
          "CodeBuddy Code 的官方登录（国内版、国际版都在这里）。要切换账号，在供应商列表里给账号建一张「CodeBuddy 官方登录」卡并点「添加」。「登录新账号」会打开终端运行 codebuddy，登录后这里自动收录；已登录时会先让 CodeBuddy 暂时退出原来的登录，放弃可恢复。令牌由 CodeBuddy 自己续期。",
      })}
      helpSide={helpSide}
      accounts={rows}
      status={isStatusError ? "error" : isStatusSuccess ? "ready" : "loading"}
      statusErrorText={t("authCenter.codebuddy.statusLoadFailed", {
        defaultValue: "无法读取 CodeBuddy 账号，请重试。",
      })}
      onRetryStatus={() => void refetchStatus()}
      notice={notice}
      emptyText={t("authCenter.codebuddy.empty", {
        defaultValue:
          "还没有 CodeBuddy 账号。在终端里登录，或收录 CodeBuddy 当前登录的号。",
      })}
      loginLabel={t("authCenter.codebuddy.login", {
        defaultValue: "登录新账号",
      })}
      onAdd={() => login.requestLogin()}
      defaultLabel={t("authCenter.codebuddy.current", {
        defaultValue: "CodeBuddy 在用",
      })}
      canReauth
      // 在终端里用同一个号重新登录，收录时替换旧副本
      onReauth={(accountId) => login.requestLogin(accountId)}
      settingDefault={false}
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
        onCancel: () => void login.cancel(),
        cancelLabel: login.cancelLabel,
        onRetry: () => login.requestLogin(),
        pendingNote: login.pendingNote,
        reopenLabel: t("authCenter.codebuddy.reopenTerminal", {
          defaultValue: "重新打开终端",
        }),
        onReopen: login.reopenTerminal,
      }}
    >
      <CodebuddyLoginConfirmDialog {...login.confirmDialogProps} />
      <ManagedAccountRemoveDialog
        target={removeTarget}
        serviceName="CodeBuddy"
        users={[]}
        othersRemain={accounts.length > 1}
        note={t("authCenter.codebuddy.removeNote", {
          defaultValue:
            "只删除 CC Switch 里的副本，不会让 CodeBuddy 退出登录。",
        })}
        pending={isRemovingAccount}
        onConfirm={confirmRemove}
        onCancel={() => setRemoveTarget(null)}
      />
    </ManagedAccountsGroup>
  );
};

export default CodebuddyAccountSection;
