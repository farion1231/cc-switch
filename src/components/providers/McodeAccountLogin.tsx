import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { Button } from "@/components/ui/button";
import { SegmentedControl } from "@/components/ui/segmented-control";
import {
  ManagedAccountsGroup,
  type GroupAccountRow,
} from "@/components/settings/auth/ManagedAccountsGroup";
import type { McodeOAuthAccount, McodeRegion } from "@/lib/api/mcodeOAuth";
import { McodeAccountQuota } from "@/components/settings/auth/AccountQuota";
import { useAccountQuotaPlans } from "@/lib/query/accountPlans";
import { useMcodeOAuth } from "./forms/hooks/useMcodeOAuth";

interface McodeAccountLoginProps {
  helpSide?: "top" | "bottom";
}

export function McodeAccountLogin({ helpSide }: McodeAccountLoginProps) {
  const { t } = useTranslation();
  const auth = useMcodeOAuth();
  const [choosing, setChoosing] = useState(false);
  const [region, setRegion] = useState<McodeRegion>("cn");
  const [lastStart, setLastStart] = useState<{
    region: McodeRegion;
    accountId?: string;
  } | null>(null);
  const [confirmRemove, setConfirmRemove] = useState<McodeOAuthAccount | null>(
    null,
  );
  const accounts = auth.status.data?.accounts ?? [];
  const currentId = auth.status.data?.currentAccountId ?? null;

  useEffect(() => {
    if (auth.remove.isSuccess) setConfirmRemove(null);
  }, [auth.remove.isSuccess]);

  const start = (next: McodeRegion, accountId?: string) => {
    setChoosing(false);
    setLastStart({ region: next, accountId });
    void auth.start(next, accountId);
  };

  const plans = useAccountQuotaPlans(
    "mcode_oauth",
    accounts.map((account) => account.id),
  );
  const rows: GroupAccountRow[] = accounts.map((account) => {
    const current = account.id === currentId;
    return {
      id: account.id,
      login: account.name,
      avatarUrl: account.avatarUrl,
      details: account.authenticated
        ? [
            plans[account.id],
            account.email && account.email !== account.name
              ? account.email
              : null,
            t(`mcode.oauth.${account.region}`),
          ].filter(Boolean)
        : [
            account.error ??
              t("mcode.oauth.signedOut", { defaultValue: "登录已失效" }),
          ],
      isDefault: current,
      needsReauth: !account.authenticated,
      users: [],
      quota: account.authenticated ? (
        <McodeAccountQuota accountId={account.id} login={account.name} />
      ) : undefined,
      removeLabel: t("mcode.oauth.removeAccount", {
        defaultValue: "移除账号",
      }),
    };
  });

  const chooser = choosing && (
    <div className="mx-3 mb-3 flex flex-col gap-2.5 rounded-[8px] border border-border px-3.5 py-3">
      <div className="flex flex-wrap items-center gap-3">
        <span className="text-caption font-medium text-fg-2">
          {t("mcode.oauth.region", { defaultValue: "账号区域" })}
        </span>
        <SegmentedControl<McodeRegion>
          size="sm"
          aria-label={t("mcode.oauth.region", { defaultValue: "账号区域" })}
          value={region}
          onValueChange={setRegion}
          items={[
            {
              value: "cn",
              label: t("mcode.oauth.cn", { defaultValue: "中国大陆" }),
            },
            {
              value: "en",
              label: t("mcode.oauth.en", { defaultValue: "国际" }),
            },
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
          {t("mcode.oauth.signIn", { defaultValue: "登录" })}
        </Button>
      </div>
    </div>
  );

  return (
    <ManagedAccountsGroup
      slug="mcode"
      name="MiniMax Code"
      iconName="minimax"
      help={t("mcode.oauth.help", {
        defaultValue:
          "MiniMax Code 官方 OAuth 登录，会自动收录 mcode 本机已登录账号，并支持多账号直连切换与 Token Plan 用量查询。",
      })}
      helpSide={helpSide}
      accounts={rows}
      status={
        auth.status.isError
          ? "error"
          : auth.status.isSuccess
            ? "ready"
            : "loading"
      }
      statusErrorText={t("mcode.oauth.statusFailed", {
        defaultValue: "无法读取 MiniMax Code 账号状态，请重试。",
      })}
      onRetryStatus={() => void auth.status.refetch()}
      emptyText={t("mcode.oauth.empty", {
        defaultValue:
          "还没有登录 MiniMax Code 账号，点击右上角添加或在 mcode 终端登录后会自动收录。",
      })}
      onAdd={() => setChoosing(true)}
      chooser={chooser || undefined}
      defaultLabel={t("authCenter.inUseBy", {
        defaultValue: "{{app}} 在用",
        app: "MiniMax Code",
      })}
      setDefaultLabel={t("authCenter.switchAppTo", {
        defaultValue: "切换 {{app}} 到这个账号",
        app: "MiniMax Code",
      })}
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
        title={t("mcode.oauth.removeAccount", { defaultValue: "移除账号" })}
        message={t("mcode.oauth.removeConfirm", {
          defaultValue:
            "确定从 CC Switch 中移除 MiniMax Code 账号「{{name}}」吗？如果正在使用，会同时清除本机登录态。",
          name: confirmRemove?.name,
        })}
        pending={auth.remove.isPending}
        confirmText={t("mcode.oauth.removeAccount", {
          defaultValue: "移除账号",
        })}
        onConfirm={() => {
          if (confirmRemove) auth.remove.mutate(confirmRemove.id);
        }}
        onCancel={() => setConfirmRemove(null)}
      />
    </ManagedAccountsGroup>
  );
}
