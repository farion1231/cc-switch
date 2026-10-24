import React from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { authApi } from "@/lib/api";
import { toast } from "@/lib/toast";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { extractErrorMessage } from "@/utils/errorUtils";
import {
  useManagedAuth,
  type ManagedAuthOptions,
} from "./hooks/useManagedAuth";

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

/** 国内版 / 国际版的显示名；认不出时原样显示域名。 */
export function useCodebuddyRegionLabel() {
  const { t } = useTranslation();
  return React.useCallback(
    (domain: string | null | undefined) => {
      const region = codebuddyRegion(domain);
      if (region === "mainland")
        return t("authCenter.codebuddy.mainland", { defaultValue: "国内版" });
      if (region === "global")
        return t("authCenter.codebuddy.global", { defaultValue: "国际版" });
      return domain || null;
    },
    [t],
  );
}

/**
 * CodeBuddy 的「登录新账号」（官方登录卡表单和授权中心共用）。
 *
 * CodeBuddy 同时只能登录一个号，已经登着号时 CLI 不会进登录界面。所以已登录时先弹确认，
 * 继续后后端把现在的号收录、写一份登出标记（凭据文件不动、不调服务端），再打开终端跑
 * `codebuddy`；等待期间「恢复原来的登录」= 取消这次登录，后端删掉自己写的标记。
 * 没登录时直接打开终端。
 */
export function useCodebuddyLogin(options?: ManagedAuthOptions) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const onCompleteRef = React.useRef(options?.onLoginComplete);
  onCompleteRef.current = options?.onLoginComplete;
  const auth = useManagedAuth("codebuddy_oauth", undefined, {
    onLoginComplete: (account) => {
      void queryClient.invalidateQueries({ queryKey: CODEBUDDY_LIVE_KEY });
      onCompleteRef.current?.(account);
    },
  });
  const live = useQuery({
    queryKey: CODEBUDDY_LIVE_KEY,
    queryFn: authApi.codebuddyAccountLive,
    refetchInterval: 15_000,
  });
  const signedIn =
    live.data?.state === "collected" || live.data?.state === "uncollected";

  /** 确认框：要重新登录哪个账号（`undefined` 是加新号）；`null` 是没开 */
  const [confirming, setConfirming] = React.useState<{
    accountId?: string;
  } | null>(null);
  /** 这次登录开始时让 CodeBuddy 退出了原来的登录（等待块里给「恢复原来的登录」） */
  const [replacing, setReplacing] = React.useState(false);
  /** 退出前登着的是哪个号（恢复后核对用） */
  const originalIdRef = React.useRef<string | null>(null);

  const {
    addAccount,
    reauthAccount,
    cancelAuthAndWait,
    isAddingAccount,
    isPolling,
  } = auth;

  const begin = React.useCallback(
    (accountId: string | undefined, replace: boolean) => {
      setReplacing(replace);
      if (accountId) reauthAccount(accountId);
      else addAccount();
    },
    [addAccount, reauthAccount],
  );

  // 开始后（标记已写）和结束后刷新一次凭据状态
  React.useEffect(() => {
    void queryClient.invalidateQueries({ queryKey: CODEBUDDY_LIVE_KEY });
  }, [isPolling, queryClient]);

  /** 「登录新账号」「重新登录」：已登录时先确认副作用 */
  const requestLogin = React.useCallback(
    (accountId?: string) => {
      if (isAddingAccount) return;
      if (signedIn) setConfirming({ accountId });
      else begin(accountId, false);
    },
    [begin, isAddingAccount, signedIn],
  );

  const confirm = React.useCallback(() => {
    const target = confirming;
    setConfirming(null);
    if (!target) return;
    originalIdRef.current = live.data?.account_id ?? null;
    begin(target.accountId, true);
  }, [begin, confirming, live.data?.account_id]);

  /** 放弃这次登录；开始时退出过原来的登录就把它恢复 */
  const cancel = React.useCallback(async () => {
    const restoring = replacing;
    setReplacing(false);
    await cancelAuthAndWait();
    const after = await queryClient
      .fetchQuery({
        queryKey: CODEBUDDY_LIVE_KEY,
        queryFn: authApi.codebuddyAccountLive,
      })
      .catch(() => null);
    await queryClient.invalidateQueries({
      queryKey: ["managed-auth-status", "codebuddy_oauth"],
    });
    if (!restoring) return;
    if (after?.state === "logged_out" || after?.state === "none") {
      toast.warning(
        t("codebuddy.login.restoreFailed", {
          defaultValue:
            "没能恢复原来的登录（CodeBuddy 可能正在写登录文件）。可以在授权中心重新登录，或在终端里登录。",
        }),
      );
    } else if (
      after?.account_id &&
      after.account_id === originalIdRef.current
    ) {
      toast.success(
        t("codebuddy.login.restored", {
          defaultValue: "已恢复原来的 CodeBuddy 登录。",
        }),
      );
    }
  }, [cancelAuthAndWait, queryClient, replacing, t]);

  const reopenTerminal = React.useCallback(() => {
    authApi.codebuddyAccountOpenLoginTerminal().catch((e: unknown) => {
      toast.error(extractErrorMessage(e) || String(e));
    });
  }, []);

  // 只在这次登录还在等待时算数（登录完、出错后不再显示「恢复」）
  const replacingNow = replacing && isPolling;
  const pendingNote = replacingNow
    ? t("codebuddy.login.pendingReplacing", {
        defaultValue:
          "已让 CodeBuddy 暂时退出原来的登录，并在终端打开了 codebuddy。按提示登录新账号，完成后这里会自动收录。",
      })
    : t("authCenter.codebuddy.pendingNote", {
        defaultValue:
          "已在终端打开 codebuddy，按提示登录（没有出现登录界面就输入 /login），完成后这里会自动收录。",
      });
  const cancelLabel = replacingNow
    ? t("codebuddy.login.restore", { defaultValue: "恢复原来的登录" })
    : t("common.cancel", { defaultValue: "取消" });

  return {
    auth,
    live: live.data,
    signedIn,
    replacing: replacingNow,
    requestLogin,
    cancel,
    reopenTerminal,
    pendingNote,
    cancelLabel,
    confirmDialogProps: {
      open: confirming !== null,
      currentLabel: live.data?.label ?? null,
      currentDomain: live.data?.domain ?? null,
      onConfirm: confirm,
      onCancel: () => setConfirming(null),
    },
  };
}

interface CodebuddyLoginConfirmDialogProps {
  open: boolean;
  currentLabel: string | null;
  currentDomain: string | null;
  onConfirm: () => void;
  onCancel: () => void;
}

/** 已登录时「登录新账号」前的确认：说明会暂时退出登录、正在运行的会话会变成未登录。 */
export function CodebuddyLoginConfirmDialog({
  open,
  currentLabel,
  currentDomain,
  onConfirm,
  onCancel,
}: CodebuddyLoginConfirmDialogProps) {
  const { t } = useTranslation();
  const regionLabel = useCodebuddyRegionLabel();
  const current = [currentLabel, regionLabel(currentDomain)]
    .filter(Boolean)
    .join(" · ");
  return (
    <ConfirmDialog
      isOpen={open}
      variant="info"
      title={t("codebuddy.login.confirmTitle", {
        defaultValue: "登录新的 CodeBuddy 账号",
      })}
      message={t("codebuddy.login.confirmMessage", {
        defaultValue:
          "CodeBuddy 现在登着 {{login}}，同一时间只能登录一个账号。继续后会先把它收录到授权中心，再让 CodeBuddy 暂时退出登录（不删除登录信息、不通知服务器），然后打开终端登录新账号。\n\n写入后，正在运行的 CodeBuddy 会话会暂时变为未登录，直到你登录完成。中途放弃可以一键恢复原来的登录。",
        login: current,
      })}
      confirmText={t("codebuddy.login.confirm", {
        defaultValue: "继续登录",
      })}
      onConfirm={() => onConfirm()}
      onCancel={onCancel}
    />
  );
}
