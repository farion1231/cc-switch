import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { Button } from "@/components/ui/button";
import { SegmentedControl } from "@/components/ui/segmented-control";
import {
  ManagedAccountsGroup,
  type GroupAccountRow,
} from "@/components/settings/auth/ManagedAccountsGroup";
import type { KimiOAuthAccount, KimiRegion } from "@/lib/api/kimiOAuth";
import { KimiAccountQuota } from "@/components/settings/auth/AccountQuota";
import { useKimiOAuth } from "./forms/hooks/useKimiOAuth";

interface KimiAccountLoginProps {
  helpSide?: "top" | "bottom";
}

/**
 * 授权中心的 Kimi Code 账号组（和其他服务同一套画法）。每个账号在 Kimi Code 里有自己的供应商
 * 和模型，「在用」就是 default_model 指向的那个号；添加账号先选登录区域。Kimi Code 自己的
 * 登录删除时只移出列表，不替它登出。
 */
export function KimiAccountLogin({ helpSide }: KimiAccountLoginProps) {
  const { t } = useTranslation();
  const auth = useKimiOAuth();
  const [choosing, setChoosing] = useState(false);
  const [region, setRegion] = useState<KimiRegion>("mainland-cn");
  // 失败后「重试」重新发起的那次登录（轮询中途出错时则在原流程上续）
  const [lastStart, setLastStart] = useState<{
    region: KimiRegion;
    accountId?: string;
  } | null>(null);
  const [confirmRemove, setConfirmRemove] = useState<KimiOAuthAccount | null>(
    null,
  );
  const accounts = auth.status.data?.accounts ?? [];
  const currentId = auth.status.data?.currentAccountId ?? null;

  useEffect(() => {
    if (auth.remove.isSuccess) setConfirmRemove(null);
  }, [auth.remove.isSuccess]);

  const start = (next: KimiRegion, accountId?: string) => {
    setChoosing(false);
    setLastStart({ region: next, accountId });
    void auth.start(next, accountId);
  };

  const rows: GroupAccountRow[] = accounts.map((account) => {
    const current = account.id === currentId;
    return {
      id: account.id,
      login: account.nickname ?? account.name,
      avatarUrl: account.avatarUrl,
      details: account.authenticated
        ? [
            t(`kimi.oauth.${account.region}`),
            account.primary ? t("kimi.oauth.primaryShort") : null,
          ].filter(Boolean)
        : [account.error ?? t("kimi.oauth.signedOut")],
      isDefault: current,
      needsReauth: !account.authenticated,
      users: [],
      quota: account.authenticated ? (
        <KimiAccountQuota accountId={account.id} login={account.name} />
      ) : undefined,
      removeLabel: account.primary
        ? t("kimi.oauth.dismissAccount")
        : t("kimi.oauth.removeAccount"),
      // 正在用的分开账号要先切走才能删（后端也会拒）；Kimi 自己的登录随时可移出列表
      removeDisabledReason:
        current && !account.primary
          ? t("kimi.oauth.removeCurrentHint")
          : undefined,
    };
  });

  const chooser = choosing && (
    <div className="mx-3 mb-3 flex flex-col gap-2.5 rounded-[8px] border border-border px-3.5 py-3">
      <div className="flex flex-wrap items-center gap-3">
        <span className="text-caption font-medium text-fg-2">
          {t("kimi.oauth.region")}
        </span>
        <SegmentedControl<KimiRegion>
          size="sm"
          aria-label={t("kimi.oauth.region")}
          value={region}
          onValueChange={setRegion}
          items={[
            { value: "mainland-cn", label: t("kimi.oauth.mainland-cn") },
            { value: "global", label: t("kimi.oauth.global") },
          ]}
        />
      </div>
      <div className="flex justify-end gap-2">
        <Button
          type="button"
          variant="neutral"
          size="compact"
          onClick={() => setChoosing(false)}
        >
          {t("common.cancel")}
        </Button>
        <Button
          type="button"
          variant="solid"
          size="compact"
          onClick={() => start(region)}
        >
          {t("kimi.oauth.signIn")}
        </Button>
      </div>
    </div>
  );

  return (
    <ManagedAccountsGroup
      slug="kimi"
      name="Kimi Code"
      iconName="kimi"
      help={t("kimi.oauth.help")}
      helpSide={helpSide}
      accounts={rows}
      status={
        auth.status.isError
          ? "error"
          : auth.status.isSuccess
            ? "ready"
            : "loading"
      }
      statusErrorText={t("kimi.oauth.statusFailed")}
      onRetryStatus={() => void auth.status.refetch()}
      emptyText={
        auth.status.data?.primaryHidden
          ? t("kimi.oauth.primaryHidden")
          : t("kimi.oauth.empty")
      }
      onAdd={() => setChoosing(true)}
      chooser={chooser || undefined}
      defaultLabel={t("kimi.oauth.current")}
      setDefaultLabel={t("kimi.oauth.useAccount")}
      canReauth
      onReauth={(accountId) => {
        const account = accounts.find((a) => a.id === accountId);
        if (account) start(account.region, account.id);
      }}
      onSetDefault={(accountId) => {
        void auth.useAccount.mutateAsync(accountId).catch(() => {});
      }}
      settingDefault={auth.useAccount.isPending}
      onRemove={(accountId) =>
        setConfirmRemove(accounts.find((a) => a.id === accountId) ?? null)
      }
      removing={auth.remove.isPending}
      login={{
        starting: auth.starting,
        // 轮询中途出错时流程还在，但要让出位置给错误提示和「重试」
        polling: Boolean(auth.flow) && !auth.error,
        pollingState: auth.error ? "error" : auth.flow ? "polling" : "idle",
        deviceCode: auth.flow
          ? {
              user_code: auth.flow.userCode,
              verification_uri: auth.flow.verificationUri,
            }
          : null,
        error: auth.error,
        onCancel: auth.cancel,
        onRetry: () => {
          if (auth.retryable) auth.retry();
          else if (lastStart) start(lastStart.region, lastStart.accountId);
        },
      }}
    >
      <ConfirmDialog
        isOpen={confirmRemove !== null}
        title={
          confirmRemove?.primary
            ? t("kimi.oauth.dismissAccount")
            : t("kimi.oauth.removeAccount")
        }
        message={t(
          confirmRemove?.primary
            ? "kimi.oauth.dismissConfirm"
            : "kimi.oauth.removeConfirm",
          { name: confirmRemove?.nickname ?? confirmRemove?.name },
        )}
        pending={auth.remove.isPending}
        confirmText={
          confirmRemove?.primary
            ? t("kimi.oauth.dismissAccount")
            : t("kimi.oauth.removeAccount")
        }
        onConfirm={() => {
          if (confirmRemove) auth.remove.mutate(confirmRemove.id);
        }}
        onCancel={() => setConfirmRemove(null)}
      />
    </ManagedAccountsGroup>
  );
}
