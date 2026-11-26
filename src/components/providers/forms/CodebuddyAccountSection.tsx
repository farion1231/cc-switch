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
import { useManagedAccountUsers } from "./hooks/useManagedAccountUsers";
import {
  ManagedAccountsGroup,
  type GroupAccountRow,
} from "@/components/settings/auth/ManagedAccountsGroup";
import { signedInDate } from "@/components/settings/auth/accountDetails";
import {
  CodebuddyAccountQuota,
  codebuddyQuotaLang,
} from "@/components/settings/auth/AccountQuota";
import { useQuotaPlansByKey } from "@/lib/query/accountPlans";
import { useCodebuddyAccountSwitch } from "@/lib/query/codebuddy";

/** 账号 id 是 `<uid>@<domain>`。 */
const uidOf = (id: string) => {
  const at = id.lastIndexOf("@");
  return at < 0 ? id : id.slice(0, at);
};

interface CodebuddyAccountSectionProps {
  helpSide?: "top" | "bottom";
}

/**
 * 授权中心的 CodeBuddy 账号组。CodeBuddy 只有一份全局登录（凭据文件），「在用」就是它现在
 * 登着的那个号。切换账号：账号 ⋯ 里的「切换 CodeBuddy 到这个账号」，或供应商列表里「添加」
 * 绑定了这个账号的「CodeBuddy 官方登录」卡（两边落到同一个 `switch_to`）。删账号只删 CC Switch 的副本。
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
  const accountUsers = useManagedAccountUsers(
    "codebuddy_oauth",
    defaultAccountId,
  );
  const { switchToAccount, switching } = useCodebuddyAccountSwitch();
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

  // 授权中心第二行的套餐（额度查到才有；企业账号没有套餐名）。查询键带语言，和
  // `useCodebuddyAccountQuota` 一致
  const quotaLang = codebuddyQuotaLang(i18n.language);
  const plans = useQuotaPlansByKey(
    accounts.map((account) => [
      account.id,
      ["codebuddy_oauth", "quota", account.id, quotaLang],
    ]),
    (data) => (data as { planName?: string | null }).planName,
  );

  // UID 只用来区分同名账号（比如两个号昵称一样），和 WorkBuddy 一样平时不显示
  const loginCounts = new Map<string, number>();
  for (const account of accounts) {
    loginCounts.set(account.login, (loginCounts.get(account.login) ?? 0) + 1);
  }

  const rows: GroupAccountRow[] = accounts.map((account) => {
    const uid = uidOf(account.id);
    const date = signedInDate(account.authenticated_at, i18n.language);
    const details: React.ReactNode[] = [
      account.requires_reauth ? null : plans[account.id],
      regionLabel(account.github_domain),
    ];
    if (account.login !== uid && (loginCounts.get(account.login) ?? 0) > 1) {
      details.push(`UID ${uid}`);
    }
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
      users: accountUsers([account.id]),
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
      name="CodeBuddy"
      iconName="codebuddy"
      help={t("authCenter.group.codebuddyHelp", {
        defaultValue:
          "CodeBuddy 的官方登录（国内版、国际版都在这里）。要切换账号，在账号的「…」里选「切换 CodeBuddy 到这个账号」，或在供应商列表里添加绑定了它的「CodeBuddy 官方登录」卡。「添加账号」会打开终端运行 codebuddy，登录后这里自动收录；已登录时会先让 CodeBuddy 暂时退出原来的登录，放弃可恢复。令牌由 CodeBuddy 自己续期。",
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
          "还没有 CodeBuddy 账号。点「添加账号」登录，或收录 CodeBuddy 当前登录的号。",
      })}
      onAdd={() => login.requestLogin()}
      defaultLabel={t("authCenter.codebuddy.current", {
        defaultValue: "CodeBuddy 在用",
      })}
      setDefaultLabel={t("authCenter.switchAppTo", {
        defaultValue: "切换 {{app}} 到这个账号",
        app: "CodeBuddy",
      })}
      onSetDefault={(accountId) => {
        const login =
          accounts.find((account) => account.id === accountId)?.login ??
          accountId;
        switchToAccount(accountId, login);
      }}
      settingDefault={switching}
      canReauth
      // 在终端里用同一个号重新登录，收录时替换旧副本
      onReauth={(accountId) => login.requestLogin(accountId)}
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
      <p className="m-0 border-t border-border py-2.5 pe-3 ps-4 text-caption text-fg-2">
        {t("authCenter.codebuddy.localOnly", {
          defaultValue:
            "账号副本只保存在这台电脑上，不会随云同步，换电脑后需要重新收录。",
        })}
      </p>
      <CodebuddyLoginConfirmDialog {...login.confirmDialogProps} />
      <ManagedAccountRemoveDialog
        target={removeTarget}
        serviceName="CodeBuddy"
        users={
          removeTarget?.kind === "one"
            ? accountUsers([removeTarget.accountId])
            : []
        }
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
