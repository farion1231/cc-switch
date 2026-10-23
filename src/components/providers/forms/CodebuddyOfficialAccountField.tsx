import React from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Loader2 } from "lucide-react";
import { authApi } from "@/lib/api";
import { toast } from "@/lib/toast";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Notice } from "@/components/ui/notice";
import { CodebuddyAccountQuota } from "@/components/settings/auth/AccountQuota";
import { extractErrorMessage } from "@/utils/errorUtils";
import { useManagedAuth } from "./hooks/useManagedAuth";
import { CODEBUDDY_LIVE_KEY, codebuddyRegion } from "./CodebuddyAccountSection";

interface CodebuddyOfficialAccountFieldProps {
  /** 卡片绑定的账号 ID（`<uid>@<domain>`） */
  accountId: string | null;
  /** 选中账号；第二个参数是账号显示名，用来给卡片起默认名字 */
  onAccountChange: (accountId: string, login: string) => void;
  /** 新建卡片时没选账号，就默认选 CodeBuddy 现在登着的号 */
  preselectCurrent?: boolean;
}

/**
 * 「CodeBuddy 官方登录」卡的账号区：列出授权中心收录的 CodeBuddy 账号供绑定，
 * 没有账号或要加新号时在终端登录（登录完自动收录并选中）或收录当前登录。
 * 切换在供应商列表的卡片上做，这里只选绑定哪个号。
 */
export function CodebuddyOfficialAccountField({
  accountId,
  onAccountChange,
  preselectCurrent = false,
}: CodebuddyOfficialAccountFieldProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const {
    accounts,
    defaultAccountId,
    isStatusSuccess,
    isStatusError,
    isPolling,
    isAddingAccount,
    error,
    addAccount,
    cancelAuth,
    refetchStatus,
  } = useManagedAuth("codebuddy_oauth");

  const { data: live } = useQuery({
    queryKey: CODEBUDDY_LIVE_KEY,
    queryFn: authApi.codebuddyAccountLive,
    refetchInterval: 15_000,
  });

  const onChangeRef = React.useRef(onAccountChange);
  onChangeRef.current = onAccountChange;

  // 在终端登录完成后，新收录的号自动选上
  const knownIds = React.useRef<Set<string> | null>(null);
  const waitingForLogin = React.useRef(false);
  React.useEffect(() => {
    if (!isStatusSuccess) return;
    const ids = new Set(accounts.map((account) => account.id));
    if (waitingForLogin.current && knownIds.current) {
      const added = accounts.find(
        (account) => !knownIds.current?.has(account.id),
      );
      if (added) {
        waitingForLogin.current = false;
        onChangeRef.current(added.id, added.login);
      }
    }
    knownIds.current = ids;
  }, [accounts, isStatusSuccess]);
  React.useEffect(() => {
    if (!isAddingAccount) return;
    waitingForLogin.current = true;
  }, [isAddingAccount]);

  // 新建时默认绑 CodeBuddy 现在登着的号
  const preselected = React.useRef(false);
  React.useEffect(() => {
    if (!preselectCurrent || preselected.current || accountId) return;
    if (!isStatusSuccess) return;
    preselected.current = true;
    const current = accounts.find((account) => account.id === defaultAccountId);
    if (current) onChangeRef.current(current.id, current.login);
  }, [
    preselectCurrent,
    accountId,
    isStatusSuccess,
    accounts,
    defaultAccountId,
  ]);

  const importCurrent = useMutation({
    mutationFn: authApi.codebuddyAccountImportCurrent,
    onSuccess: async (account) => {
      toast.success(
        t("authCenter.codebuddy.imported", {
          defaultValue: "已收录 {{login}}。",
          login: account.login,
        }),
      );
      onChangeRef.current(account.id, account.login);
      await refetchStatus();
      await queryClient.invalidateQueries({ queryKey: CODEBUDDY_LIVE_KEY });
    },
    onError: (e) => {
      toast.error(extractErrorMessage(e) || String(e));
    },
  });

  const regionLabel = (domain: string | null | undefined) => {
    const region = codebuddyRegion(domain);
    if (region === "mainland")
      return t("authCenter.codebuddy.mainland", { defaultValue: "国内版" });
    if (region === "global")
      return t("authCenter.codebuddy.global", { defaultValue: "国际版" });
    return domain || null;
  };

  const boundMissing =
    isStatusSuccess &&
    Boolean(accountId) &&
    !accounts.some((account) => account.id === accountId);
  const canImport = live?.state === "uncollected";

  return (
    <div className="space-y-3">
      <div className="space-y-1">
        <Label className="text-sm font-medium text-fg-1">
          {t("codebuddy.official.account", { defaultValue: "登录账号" })}
        </Label>
        <p className="text-xs text-fg-2">
          {t("codebuddy.official.accountHint", {
            defaultValue:
              "选择这张卡要用的 CodeBuddy 账号。在供应商列表里点卡片上的「切换」，CodeBuddy 的登录就会换成它，正在运行的 CodeBuddy 也会自动换过去。",
          })}
        </p>
      </div>

      {boundMissing && (
        <Notice
          tone="warning"
          title={t("codebuddy.official.accountDeleted", {
            defaultValue: "账号已删除，请重新选择",
          })}
        />
      )}

      {isStatusError ? (
        <Notice
          tone="danger"
          title={t("authCenter.codebuddy.statusLoadFailed", {
            defaultValue: "无法读取 CodeBuddy 账号，请重试。",
          })}
          actions={
            <Button
              type="button"
              variant="neutral"
              size="compact"
              onClick={() => void refetchStatus()}
            >
              {t("codexOauth.retry", { defaultValue: "重试" })}
            </Button>
          }
        />
      ) : !isStatusSuccess ? (
        <div className="flex items-center gap-2 text-sm text-fg-2">
          <Loader2 className="h-4 w-4 animate-spin" />
          {t("codebuddy.official.statusLoading", {
            defaultValue: "正在读取账号…",
          })}
        </div>
      ) : accounts.length === 0 ? (
        <div className="rounded-panel border border-dashed border-border px-4 py-3 text-caption text-fg-2">
          {t("authCenter.codebuddy.empty", {
            defaultValue:
              "还没有 CodeBuddy 账号。在终端里登录，或收录 CodeBuddy 当前登录的号。",
          })}
        </div>
      ) : (
        <div
          role="radiogroup"
          aria-label={t("codebuddy.official.account", {
            defaultValue: "登录账号",
          })}
          className="overflow-hidden rounded-panel border border-border"
        >
          {accounts.map((account, index) => {
            const selected = account.id === accountId;
            const details = [regionLabel(account.github_domain)];
            if (account.requires_reauth) {
              details.push(
                t("authCenter.codebuddy.expiredNote", {
                  defaultValue: "登录已过期，需要重新登录",
                }),
              );
            }
            return (
              <div
                key={account.id}
                className={cn(
                  "flex min-h-[52px] items-center gap-3 pe-2.5 ps-3",
                  index > 0 && "border-t border-border",
                  selected && "bg-direct-soft",
                )}
              >
                <button
                  type="button"
                  role="radio"
                  aria-checked={selected}
                  onClick={() => onAccountChange(account.id, account.login)}
                  className="flex min-w-0 flex-1 items-center gap-3 py-2 text-start"
                >
                  <span
                    aria-hidden
                    className={cn(
                      "flex h-4 w-4 shrink-0 items-center justify-center rounded-full border",
                      selected
                        ? "border-direct-text bg-direct-text text-white"
                        : "border-border-strong",
                    )}
                  >
                    {selected && <Check className="h-3 w-3" strokeWidth={3} />}
                  </span>
                  <span className="flex min-w-0 flex-1 flex-col">
                    <span className="flex min-w-0 items-center gap-1.5">
                      <span
                        className="min-w-0 truncate text-body font-medium text-fg-1"
                        title={account.login}
                      >
                        {account.login}
                      </span>
                      {account.id === defaultAccountId && (
                        <span className="inline-flex h-[18px] shrink-0 items-center rounded-full border border-border-strong px-1.5 text-badge text-fg-2">
                          {t("authCenter.codebuddy.current", {
                            defaultValue: "CodeBuddy 在用",
                          })}
                        </span>
                      )}
                    </span>
                    <span
                      className={cn(
                        "truncate text-caption",
                        account.requires_reauth
                          ? "text-warning-text"
                          : "text-fg-2",
                      )}
                    >
                      {details.filter(Boolean).join(" · ")}
                    </span>
                  </span>
                </button>
                {!account.requires_reauth && (
                  <CodebuddyAccountQuota
                    accountId={account.id}
                    login={account.login}
                  />
                )}
              </div>
            );
          })}
        </div>
      )}

      {isPolling ? (
        <Notice
          title={t("authCenter.codebuddy.pendingNote", {
            defaultValue:
              "已在终端打开 codebuddy，在里面输入 /login 完成登录后这里会自动收录。",
          })}
          actions={
            <>
              <Button
                type="button"
                variant="neutral"
                size="compact"
                onClick={() => {
                  authApi
                    .codebuddyAccountOpenLoginTerminal()
                    .catch((e: unknown) =>
                      toast.error(extractErrorMessage(e) || String(e)),
                    );
                }}
              >
                {t("authCenter.codebuddy.reopenTerminal", {
                  defaultValue: "重新打开终端",
                })}
              </Button>
              <Button
                type="button"
                variant="quiet"
                size="compact"
                onClick={() => {
                  waitingForLogin.current = false;
                  cancelAuth();
                }}
              >
                {t("common.cancel", { defaultValue: "取消" })}
              </Button>
            </>
          }
        />
      ) : (
        <div className="flex flex-wrap items-center gap-2">
          <Button
            type="button"
            variant="neutral"
            size="compact"
            disabled={isAddingAccount}
            onClick={addAccount}
          >
            {isAddingAccount && (
              <Loader2 className="me-1 h-3.5 w-3.5 animate-spin" />
            )}
            {t("authCenter.codebuddy.login", { defaultValue: "在终端登录" })}
          </Button>
          {canImport && (
            <Button
              type="button"
              variant="neutral"
              size="compact"
              disabled={importCurrent.isPending}
              onClick={() => importCurrent.mutate()}
            >
              {t("codebuddy.official.importCurrent", {
                defaultValue: "收录当前登录（{{login}}）",
                login: [live?.label, regionLabel(live?.domain)]
                  .filter(Boolean)
                  .join(" · "),
              })}
            </Button>
          )}
        </div>
      )}
      {error && !isPolling && (
        <p role="alert" className="text-xs text-danger-text">
          {error}
        </p>
      )}
    </div>
  );
}

export default CodebuddyOfficialAccountField;
