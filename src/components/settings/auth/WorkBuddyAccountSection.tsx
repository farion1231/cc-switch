import React from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ExternalLink } from "lucide-react";
import { toast } from "@/lib/toast";
import { authApi } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Notice } from "@/components/ui/notice";
import { useManagedAuth } from "@/components/providers/forms/hooks/useManagedAuth";
import {
  ManagedAccountRemoveDialog,
  type ManagedAccountRemoveTarget,
} from "@/components/providers/forms/ManagedAccountRemoveDialog";
import {
  ManagedAccountsGroup,
  type GroupAccountRow,
} from "@/components/settings/auth/ManagedAccountsGroup";
import { signedInDate } from "@/components/settings/auth/accountDetails";
import { workbuddyPlanLabel } from "@/components/settings/auth/workbuddyPlan";
import { useManagedAccountUsers } from "@/components/providers/forms/hooks/useManagedAccountUsers";
import { WORKBUDDY_LIVE_KEY } from "@/components/providers/workbuddyOfficial";
import { WorkBuddyAccountQuota } from "@/components/settings/auth/AccountQuota";

const PROVIDER = "workbuddy_account";
const LIVE_KEY = WORKBUDDY_LIVE_KEY;

/**
 * 已经提示过的「重新登录」结果（按 id）。后端会把结果留一会儿，供应商列表等别处也会读到
 * 同一份状态，这里保证同一次结果只弹一次。
 */
const notifiedReauth = new Set<number>();

/** 添加账号后要给用户看的下一步：当前号已收录（想加另一个）、或 WorkBuddy 没登录。 */
type AddHint = "already" | "signedOut" | null;

interface WorkBuddyAccountSectionProps {
  helpSide?: "top" | "bottom";
}

const message = (error: unknown) =>
  error instanceof Error ? error.message : String(error);

/**
 * 授权中心的 WorkBuddy 账号组。WorkBuddy 的登录由桌面端完成，CC Switch 只能把当前登录的那组
 * 凭据文件收录成快照。切换账号在供应商列表里：「添加」绑定了这个账号的「WorkBuddy 官方登录」卡。
 * 「WorkBuddy 在用」就是它现在登着的号。快照只在本机有效；删账号只删 CC Switch 的副本，
 * 不让 WorkBuddy 登出。
 */
export function WorkBuddyAccountSection({
  helpSide,
}: WorkBuddyAccountSectionProps) {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const {
    accounts,
    defaultAccountId,
    isStatusSuccess,
    isStatusError,
    isRemovingAccount,
    removeAccount,
    refetchStatus,
  } = useManagedAuth(PROVIDER);
  const accountUsers = useManagedAccountUsers(PROVIDER, defaultAccountId);
  const { data: live } = useQuery({
    queryKey: LIVE_KEY,
    queryFn: authApi.workbuddyAccountLiveState,
    refetchInterval: 5000,
  });
  const [addHint, setAddHint] = React.useState<AddHint>(null);
  const [removeTarget, setRemoveTarget] =
    React.useState<ManagedAccountRemoveTarget | null>(null);

  const refresh = async () => {
    await refetchStatus();
    await queryClient.invalidateQueries({ queryKey: LIVE_KEY });
  };

  const importMutation = useMutation({
    mutationFn: authApi.workbuddyAccountImportCurrent,
    onSuccess: async (result) => {
      if (result.created) {
        setAddHint(null);
        toast.success(
          t("authCenter.toast.added", {
            defaultValue: "已添加 {{service}} 账号。",
            service: "WorkBuddy",
          }),
        );
      } else {
        setAddHint("already");
      }
      await refresh();
    },
    onError: async (error) => {
      // 没登录时换成带「打开 WorkBuddy」的提示，其他错误照常弹出
      const state = await authApi.workbuddyAccountLiveState().catch(() => null);
      if (state && !state.signedIn) setAddHint("signedOut");
      else toast.error(message(error));
      await refresh();
    },
  });

  const openApp = () =>
    authApi.workbuddyAccountOpenApp().catch((error) => {
      toast.error(message(error));
    });

  const prepareMutation = useMutation({
    mutationFn: () => authApi.workbuddyAccountPrepareNewLogin(),
    onSuccess: async () => {
      setAddHint(null);
      toast.success(
        t("authCenter.workbuddy.preparedNewLogin", {
          defaultValue:
            "已收录当前账号，WorkBuddy 打开后会停在登录页。登录另一个账号后回到这里点「添加账号」。",
        }),
      );
      await refresh();
      await openApp();
    },
    onError: (error) => {
      toast.error(message(error));
    },
  });

  const accountLogin = (accountId: string) =>
    accounts.find((account) => account.id === accountId)?.login ?? accountId;

  // 重新登录：走「登录另一个账号」的流程，并让后端记住目标账号。之后登回它时替换它的旧快照，
  // 登进了别的号就按普通收录处理；结果经 live 状态的 `reauth` 回来。
  const reauthMutation = useMutation({
    mutationFn: (accountId: string) =>
      authApi.workbuddyAccountPrepareNewLogin(accountId),
    onSuccess: async (_captured, accountId) => {
      setAddHint(null);
      toast.success(
        t("authCenter.workbuddy.reauthPrepared", {
          defaultValue:
            "WorkBuddy 打开后会停在登录页。请登录 {{login}}，登录后 CC Switch 会用新的登录替换它的快照。",
          login: accountLogin(accountId),
        }),
      );
      await refresh();
      await openApp();
    },
    onError: async (error, accountId) => {
      // WorkBuddy 在运行时没法让它停在登录页：说明可以在 WorkBuddy 里直接重新登录（目标已记下）
      const state = await authApi.workbuddyAccountLiveState().catch(() => null);
      if (state?.running) {
        toast.error(
          t("authCenter.workbuddy.reauthRunningTitle", {
            defaultValue: "WorkBuddy 正在运行，没法让它停在登录页",
          }),
          {
            description: t("authCenter.workbuddy.reauthRunningBody", {
              defaultValue:
                "可以先完全退出 WorkBuddy（含菜单栏/托盘）再点「重新登录」；也可以直接在 WorkBuddy 里退出当前账号、重新登录 {{login}}，CC Switch 会自动替换它的快照。",
              login: accountLogin(accountId),
            }),
            closeButton: true,
          },
        );
      } else {
        toast.error(message(error));
      }
      await refresh();
    },
  });

  const reauthOutcome = live?.reauth ?? null;
  React.useEffect(() => {
    if (!reauthOutcome || notifiedReauth.has(reauthOutcome.id)) return;
    notifiedReauth.add(reauthOutcome.id);
    if (reauthOutcome.sameAccount) {
      toast.success(
        t("authCenter.toast.reauthed", {
          defaultValue: "{{login}} 已重新登录。",
          login: reauthOutcome.login,
        }),
      );
    } else {
      const values = {
        target: reauthOutcome.targetLogin,
        login: reauthOutcome.login,
      };
      toast.warning(
        reauthOutcome.created
          ? t("authCenter.workbuddy.reauthOtherAdded", {
              defaultValue:
                "登录的不是 {{target}}，已把 {{login}} 作为新账号收录。",
              ...values,
            })
          : t("authCenter.workbuddy.reauthOtherUpdated", {
              defaultValue:
                "登录的不是 {{target}}，{{login}} 已在列表里，副本已更新。",
              ...values,
            }),
      );
    }
    void refetchStatus();
  }, [reauthOutcome, refetchStatus, t]);

  const confirmRemove = () => {
    const target = removeTarget;
    setRemoveTarget(null);
    if (target?.kind === "one") removeAccount(target.accountId);
  };

  const rows: GroupAccountRow[] = accounts.map((account) => {
    const date = signedInDate(account.authenticated_at, i18n.language);
    const details: React.ReactNode[] = account.requires_reauth
      ? [
          t("authCenter.workbuddy.expiredNote", {
            defaultValue:
              "登录已过期，点「重新登录」后在 WorkBuddy 里登录这个号",
          }),
        ]
      : [
          ...(account.account_kind
            ? [workbuddyPlanLabel(t, account.account_kind)]
            : []),
          ...(date
            ? [
                t("authCenter.workbuddy.addedOn", {
                  defaultValue: "{{date}}收录",
                  date,
                }),
              ]
            : []),
        ];
    return {
      id: account.id,
      login: account.login,
      details,
      isDefault: defaultAccountId === account.id,
      needsReauth: account.requires_reauth,
      users: accountUsers([account.id]),
      quota: (
        <WorkBuddyAccountQuota accountId={account.id} login={account.login} />
      ),
    };
  });

  const liveNotice = live?.running ? (
    <Notice
      tone="neutral"
      title={t("authCenter.workbuddy.runningTitle", {
        defaultValue: "WorkBuddy 正在运行",
      })}
    >
      {t("authCenter.workbuddy.runningBody", {
        defaultValue:
          "切换账号或登录另一个账号前，请先完全退出 WorkBuddy（含菜单栏/托盘），否则它会把当前登录写回去。",
      })}
    </Notice>
  ) : live?.signedIn && !live.recognized ? (
    <Notice
      tone="warning"
      title={t("authCenter.workbuddy.unrecognizedTitle", {
        defaultValue: "认不出 WorkBuddy 当前登录的账号",
      })}
    >
      {t("authCenter.workbuddy.unrecognizedBody", {
        defaultValue:
          "登录文件的格式可能变了，暂时不能收录。在供应商列表切换到别的账号前会把它原样暂存。",
      })}
    </Notice>
  ) : live?.signedIn &&
    live.recognized &&
    !live.imported &&
    accounts.length > 0 ? (
    <Notice
      tone="neutral"
      title={t("authCenter.workbuddy.notImportedTitle", {
        defaultValue: "WorkBuddy 当前登录的账号还没收录",
      })}
      actions={
        <Button
          type="button"
          variant="neutral"
          size="compact"
          disabled={importMutation.isPending}
          onClick={() => importMutation.mutate()}
        >
          {t("authCenter.workbuddy.importNow", { defaultValue: "收录" })}
        </Button>
      }
    />
  ) : null;

  const addHintPanel = addHint && (
    <div className="mx-3 mb-3">
      <Notice
        tone="neutral"
        title={
          addHint === "already"
            ? t("authCenter.workbuddy.alreadyTitle", {
                defaultValue: "当前登录的账号已在列表里（副本已更新）",
              })
            : t("authCenter.workbuddy.signedOutTitle", {
                defaultValue: "WorkBuddy 当前没有登录",
              })
        }
        onDismiss={() => setAddHint(null)}
        dismissLabel={t("common.close", { defaultValue: "关闭" })}
        actions={
          <>
            {addHint === "already" && (
              <Button
                type="button"
                variant="neutral"
                size="compact"
                disabled={prepareMutation.isPending}
                onClick={() => prepareMutation.mutate()}
              >
                {t("authCenter.workbuddy.prepareNewLogin", {
                  defaultValue: "登录另一个账号",
                })}
              </Button>
            )}
            <Button
              type="button"
              variant="quiet"
              size="compact"
              className="gap-1"
              onClick={() => void openApp()}
            >
              <ExternalLink className="h-3.5 w-3.5" strokeWidth={1.5} />
              {t("authCenter.workbuddy.openApp", {
                defaultValue: "打开 WorkBuddy",
              })}
            </Button>
          </>
        }
      >
        {addHint === "already"
          ? t("authCenter.workbuddy.alreadyBody", {
              defaultValue:
                "要添加另一个账号：先完全退出 WorkBuddy，点「登录另一个账号」（CC Switch 只在本机让 WorkBuddy 停在登录页，不会在服务端登出当前账号），在 WorkBuddy 里登录另一个账号，再回来点「添加账号」。",
            })
          : t("authCenter.workbuddy.signedOutBody", {
              defaultValue:
                "在 WorkBuddy 里登录后，回到这里点「添加账号」收录。",
            })}
      </Notice>
    </div>
  );

  return (
    <ManagedAccountsGroup
      slug="workbuddy"
      name="WorkBuddy"
      iconName="workbuddy"
      help={t("authCenter.group.workbuddyHelp", {
        defaultValue:
          "WorkBuddy 桌面端的官方登录。CC Switch 不能替 WorkBuddy 登录，只能收录它当前登着的账号（整组登录文件的快照）。要切换账号，在供应商列表里添加绑定了它的「WorkBuddy 官方登录」卡（同一时间只有一张在「已添加」）。快照只在这台电脑上有效，不会随云同步。",
      })}
      helpSide={helpSide}
      accounts={rows}
      status={isStatusError ? "error" : isStatusSuccess ? "ready" : "loading"}
      statusErrorText={t("authCenter.workbuddy.statusLoadFailed", {
        defaultValue: "无法读取 WorkBuddy 账号，请重试。",
      })}
      onRetryStatus={() => void refresh()}
      notice={liveNotice}
      emptyText={t("authCenter.workbuddy.empty", {
        defaultValue:
          "还没有 WorkBuddy 账号。先在 WorkBuddy 里登录，再收录当前登录的账号。",
      })}
      loginLabel={t("authCenter.workbuddy.importCurrent", {
        defaultValue: "收录当前登录",
      })}
      onAdd={() => importMutation.mutate()}
      chooser={addHintPanel || undefined}
      defaultLabel={t("authCenter.workbuddy.current", {
        defaultValue: "WorkBuddy 在用",
      })}
      canReauth
      onReauth={(accountId) => {
        if (!reauthMutation.isPending) reauthMutation.mutate(accountId);
      }}
      settingDefault={false}
      onRemove={(accountId, login) =>
        setRemoveTarget({ kind: "one", accountId, login })
      }
      removing={isRemovingAccount}
      login={{
        starting: false,
        polling: false,
        pollingState: "idle",
        deviceCode: null,
        error: null,
        onCancel: () => {},
        onRetry: () => importMutation.mutate(),
      }}
    >
      <p className="m-0 border-t border-border py-2.5 pe-3 ps-4 text-caption text-fg-2">
        {t("authCenter.workbuddy.localOnly", {
          defaultValue:
            "快照只在这台电脑上有效（登录凭据绑定本机），不会随云同步，换电脑后需要重新收录。",
        })}
      </p>
      <ManagedAccountRemoveDialog
        target={removeTarget}
        serviceName="WorkBuddy"
        users={
          removeTarget?.kind === "one"
            ? accountUsers([removeTarget.accountId])
            : []
        }
        othersRemain={accounts.length > 1}
        note={t("authCenter.workbuddy.removeNote", {
          defaultValue:
            "只删除 CC Switch 里的快照，不会让 WorkBuddy 退出登录。",
        })}
        pending={isRemovingAccount}
        onConfirm={confirmRemove}
        onCancel={() => setRemoveTarget(null)}
      />
    </ManagedAccountsGroup>
  );
}

export default WorkBuddyAccountSection;
