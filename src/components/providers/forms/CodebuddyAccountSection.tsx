import React from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { authApi } from "@/lib/api";
import { toast } from "@/lib/toast";
import { Button } from "@/components/ui/button";
import { Notice } from "@/components/ui/notice";
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

export const CODEBUDDY_LIVE_KEY = ["codebuddy-account-live"] as const;

/** `auth.domain` 属于国内版还是国际版；认不出时为 `null`（界面直接显示域名）。 */
export function codebuddyRegion(
  domain: string | null | undefined,
): "mainland" | "global" | null {
  const value = (domain ?? "").trim().toLowerCase();
  if (!value) return null;
  if (value.includes("codebuddy.ai")) return "global";
  if (
    value.includes("codebuddy.cn") ||
    value.includes("tencent.com") ||
    value.endsWith(".cn")
  )
    return "mainland";
  return null;
}

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
 * 登着的那个号；「切换」把选中的号写进去，运行中的 CLI 会自己换号。添加账号是在终端里跑
 * `codebuddy` 执行 `/login`，或者直接收录当前登录。删账号只删 CC Switch 的副本。
 */
export const CodebuddyAccountSection: React.FC<
  CodebuddyAccountSectionProps
> = ({ helpSide }) => {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
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
  } = useManagedAuth("codebuddy_oauth");
  const [removeTarget, setRemoveTarget] =
    React.useState<ManagedAccountRemoveTarget | null>(null);

  const { data: live } = useQuery({
    queryKey: CODEBUDDY_LIVE_KEY,
    queryFn: authApi.codebuddyAccountLive,
    refetchInterval: 15_000,
  });
  // 账号表变了（终端登录收录、切换、删除）时凭据文件的状态也跟着变
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

  const reopenTerminal = () => {
    authApi.codebuddyAccountOpenLoginTerminal().catch((e: unknown) => {
      toast.error(e instanceof Error ? e.message : String(e));
    });
  };

  const regionLabel = (domain: string | null | undefined) => {
    const region = codebuddyRegion(domain);
    if (region === "mainland")
      return t("authCenter.codebuddy.mainland", { defaultValue: "国内版" });
    if (region === "global")
      return t("authCenter.codebuddy.global", { defaultValue: "国际版" });
    return domain || null;
  };

  const confirmRemove = () => {
    const target = removeTarget;
    setRemoveTarget(null);
    if (!target) return;
    if (target.kind === "all") logout();
    else removeAccount(target.accountId);
  };

  const rows: GroupAccountRow[] = accounts.map((account) => {
    const uid = uidOf(account.id);
    const date = signedInDate(account.authenticated_at, i18n.language);
    const details: React.ReactNode[] = [regionLabel(account.github_domain)];
    if (account.login !== uid) details.push(`UID ${uid}`);
    if (account.requires_reauth) {
      details.push(
        t("authCenter.codebuddy.expiredNote", {
          defaultValue: "登录已过期，切不过去",
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
          "CodeBuddy Code 的官方登录（国内版、国际版都在这里）。CodeBuddy 同时只能登录一个号，「切换」会把它的登录换成选中的账号，正在运行的 CodeBuddy 也会自动换过去。添加账号会打开终端运行 codebuddy，在里面执行 /login 登录后这里自动收录；令牌由 CodeBuddy 自己续期。",
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
        defaultValue: "在终端登录",
      })}
      onAdd={addAccount}
      defaultLabel={t("authCenter.codebuddy.current", {
        defaultValue: "CodeBuddy 在用",
      })}
      setDefaultLabel={t("authCenter.codebuddy.switchTo", {
        defaultValue: "切换 CodeBuddy 到这个账号",
      })}
      canReauth
      // 在终端里用同一个号重新 /login，收录时替换旧副本
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
        pendingNote: t("authCenter.codebuddy.pendingNote", {
          defaultValue:
            "已在终端打开 codebuddy，在里面输入 /login 完成登录后这里会自动收录。",
        }),
        reopenLabel: t("authCenter.codebuddy.reopenTerminal", {
          defaultValue: "重新打开终端",
        }),
        onReopen: reopenTerminal,
      }}
    >
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
