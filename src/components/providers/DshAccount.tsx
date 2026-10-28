import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2, RefreshCw } from "lucide-react";
import { toast } from "@/lib/toast";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { Button } from "@/components/ui/button";
import { Notice } from "@/components/ui/notice";
import {
  ManagedAccountsGroup,
  type GroupAccountRow,
} from "@/components/settings/auth/ManagedAccountsGroup";
import {
  dshApi,
  type DshAccountStatus,
  type DshLoginStart,
  type DshWallet,
} from "@/lib/api/dsh";
import { settingsApi } from "@/lib/api";
import { extractErrorMessage } from "@/utils/errorUtils";

/** 授权中心和两个 DeepSeek Harness 供应商页共用一份查询：登录本来就是 Web / Desktop 共用的 */
export const DSH_ACCOUNT_STATUS_KEY = ["dsh-account-status"] as const;

function useDshAccountStatus() {
  return useQuery({
    queryKey: DSH_ACCOUNT_STATUS_KEY,
    queryFn: dshApi.accountStatus,
    staleTime: 60_000,
  });
}

const CURRENCY_SYMBOL: Record<string, string> = { CNY: "¥", USD: "$" };

function formatWallet({ currency, balance }: DshWallet): string {
  const amount = Number(balance);
  const text = Number.isFinite(amount) ? amount.toFixed(2) : balance;
  const symbol = CURRENCY_SYMBOL[currency];
  return symbol ? `${symbol}${text}` : `${text} ${currency}`;
}

function accountName(status: DshAccountStatus, fallback: string): string {
  return status.profile?.name ?? status.profile?.contact ?? fallback;
}

/**
 * 右侧额度列：余额一行、赠金一行（DeepSeek Harness 也分开显示，免得把赠金当成充值），
 * 刷新按钮在它们右边、垂直居中。
 */
function DshBalance({
  status,
  loading,
  onRefresh,
}: {
  status: DshAccountStatus;
  loading: boolean;
  onRefresh: () => void;
}) {
  const { t } = useTranslation();
  const wallets = status.wallets.map(formatWallet).join(" · ");
  const bonus = status.bonusWallets.map(formatWallet).join(" · ");
  return (
    <div className="auth-quota-col flex shrink-0 items-center gap-1.5 text-caption">
      <div className="flex flex-col items-end gap-0.5">
        {status.error ? (
          <span className="text-fg-3">{t("dsh.account.balanceFailed")}</span>
        ) : (
          <>
            <span className="text-fg-1 tabular-nums">
              {t("dsh.account.balance", { amount: wallets || "—" })}
            </span>
            {bonus && (
              <span className="text-fg-2 tabular-nums">
                {t("dsh.account.bonus", { amount: bonus })}
              </span>
            )}
          </>
        )}
      </div>
      <Button
        type="button"
        variant="quiet"
        size="compact"
        aria-label={t("dsh.account.refresh")}
        disabled={loading}
        onClick={onRefresh}
      >
        {loading ? (
          <Loader2 className="h-3 w-3 animate-spin" />
        ) : (
          <RefreshCw className="h-3 w-3" />
        )}
      </Button>
    </div>
  );
}

const POLL_INTERVAL_MS = 2_000;

/**
 * 浏览器登录：后端起本机回调、向平台登记，这里打开授权页并轮询，直到登录写进共用的凭据文件。
 */
function useDshLogin(onSignedIn: () => void) {
  const { t } = useTranslation();
  const [flow, setFlow] = useState<DshLoginStart | null>(null);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const flowRef = useRef<DshLoginStart | null>(null);
  flowRef.current = flow;

  const start = useCallback(async () => {
    setError(null);
    setStarting(true);
    try {
      const next = await dshApi.loginStart();
      setFlow(next);
      void settingsApi.openExternal(next.authUrl);
    } catch (e) {
      setFlow(null);
      setError(extractErrorMessage(e) || t("dsh.account.signInFailed"));
    } finally {
      setStarting(false);
    }
  }, [t]);

  const cancel = useCallback(() => {
    const current = flowRef.current;
    if (current) void dshApi.loginCancel(current.flowId).catch(() => {});
    setFlow(null);
    setError(null);
  }, []);

  useEffect(() => {
    if (!flow || error) return;
    const timer = window.setInterval(() => {
      dshApi
        .loginPoll(flow.flowId)
        .then((done) => {
          if (!done || flowRef.current?.flowId !== flow.flowId) return;
          setFlow(null);
          toast.success(t("dsh.account.signedIn"));
          onSignedIn();
        })
        .catch((e) =>
          setError(extractErrorMessage(e) || t("dsh.account.signInFailed")),
        );
    }, POLL_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, [flow, error, onSignedIn, t]);

  // 离开页面时撤下还在等的登录，本机回调端口随之关闭
  useEffect(
    () => () => {
      const current = flowRef.current;
      if (current) void dshApi.loginCancel(current.flowId).catch(() => {});
    },
    [],
  );

  return {
    start,
    login: {
      starting,
      polling: Boolean(flow) && !error,
      pollingState: error
        ? ("error" as const)
        : flow
          ? ("polling" as const)
          : ("idle" as const),
      // 没有验证码：等待块只显示「在浏览器里继续」和打开授权页的按钮
      deviceCode: flow
        ? { user_code: "", verification_uri: flow.authUrl }
        : null,
      error,
      onCancel: cancel,
      onRetry: () => void start(),
    },
  };
}

interface DshAccountLoginProps {
  helpSide?: "top" | "bottom";
}

/**
 * 授权中心的 DeepSeek 账号组。登录和 DeepSeek Harness Web 走同一套浏览器授权，结果写进
 * `$DSH_HOME/.credentials.yaml`，Web 与 Desktop 共用（运行中的 DeepSeek Harness 会自动重新读取）；
 * 这里显示账号和余额，可以退出（两端同时退出）。
 */
export function DshAccountLogin({ helpSide }: DshAccountLoginProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const status = useDshAccountStatus();
  const [confirmSignOut, setConfirmSignOut] = useState(false);
  const refreshStatus = useCallback(
    () =>
      void queryClient.invalidateQueries({ queryKey: DSH_ACCOUNT_STATUS_KEY }),
    [queryClient],
  );
  const { start, login } = useDshLogin(refreshStatus);
  const signOut = useMutation({
    mutationFn: dshApi.accountSignOut,
    onSuccess: async () => {
      setConfirmSignOut(false);
      await queryClient.invalidateQueries({ queryKey: DSH_ACCOUNT_STATUS_KEY });
    },
    onError: (error) =>
      toast.error(t("dsh.account.signOutFailed"), {
        description: extractErrorMessage(error) || undefined,
      }),
  });

  const data = status.data;
  const name = data ? accountName(data, t("dsh.account.unnamed")) : "";
  const rows: GroupAccountRow[] =
    data?.signedIn === true
      ? [
          {
            id: "default",
            login: name,
            avatarUrl: data.profile?.avatarUrl,
            details: data.expired
              ? [t("dsh.account.expired")]
              : [
                  data.profile?.name ? data.profile.contact : null,
                  t("dsh.account.shared"),
                ].filter(Boolean),
            isDefault: false,
            needsReauth: data.expired,
            users: [],
            quota: data.expired ? undefined : (
              <DshBalance
                status={data}
                loading={status.isFetching}
                onRefresh={() => void status.refetch()}
              />
            ),
            removeLabel: t("dsh.account.signOut"),
          },
        ]
      : [];

  return (
    <ManagedAccountsGroup
      slug="dsh"
      name="DeepSeek Harness"
      iconName="deepseek"
      help={t("dsh.account.help")}
      helpSide={helpSide}
      accounts={rows}
      status={status.isError ? "error" : status.isSuccess ? "ready" : "loading"}
      statusErrorText={t("dsh.account.statusFailed")}
      onRetryStatus={() => void status.refetch()}
      emptyText={t("dsh.account.empty")}
      onAdd={() => void start()}
      canReauth
      reauthOnlyWhenExpired
      onReauth={() => void start()}
      settingDefault={false}
      onRemove={() => setConfirmSignOut(true)}
      removing={signOut.isPending}
      login={login}
    >
      <ConfirmDialog
        isOpen={confirmSignOut}
        title={t("dsh.account.signOut")}
        message={t("dsh.account.signOutConfirm", { name })}
        pending={signOut.isPending}
        confirmText={t("dsh.account.signOut")}
        onConfirm={() => signOut.mutate()}
        onCancel={() => setConfirmSignOut(false)}
      />
    </ManagedAccountsGroup>
  );
}

/**
 * DeepSeek Harness / Desktop 供应商页顶部的一行：DeepSeek 账号（OAuth）的登录是两端共用的，
 * 不是这里的供应商卡片，单独说明当前登录了谁、余额多少。
 */
export function DshAccountNotice() {
  const { t } = useTranslation();
  const status = useDshAccountStatus();
  const data = status.data;
  if (!data) return null;
  if (!data.signedIn) {
    return (
      <Notice tone="neutral" title={t("dsh.account.noticeSignedOutTitle")}>
        {t("dsh.account.noticeSignedOut")}
      </Notice>
    );
  }
  if (data.expired) {
    return (
      <Notice tone="warning" title={t("dsh.account.noticeExpiredTitle")}>
        {t("dsh.account.noticeExpired")}
      </Notice>
    );
  }
  const balance = data.wallets.map(formatWallet).join(" · ");
  return (
    <Notice
      tone="neutral"
      title={t("dsh.account.noticeSignedIn", {
        name: accountName(data, t("dsh.account.unnamed")),
      })}
    >
      {balance && !data.error
        ? `${t("dsh.account.balance", { amount: balance })} · `
        : ""}
      {t("dsh.account.shared")}
    </Notice>
  );
}
