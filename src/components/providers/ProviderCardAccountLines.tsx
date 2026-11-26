import type { ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { AlertTriangle } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { Provider } from "@/types";
import {
  managedAuthStatusQuery,
  xaiAuthStatusQuery,
} from "@/lib/query/managedAuth";
import CodebuddyAccountQuotaFooter from "@/components/CodebuddyAccountQuotaFooter";
import { codebuddyRegion } from "@/components/providers/forms/CodebuddyLogin";
import type { CardChip } from "./presentation";

function useResolvedAgyAccount(boundAccountId: string | null) {
  const { data } = useQuery(managedAuthStatusQuery("agy_oauth"));
  const activeAccountId = data?.default_account_id;
  const targetAccountId = boundAccountId ?? activeAccountId;
  const account = targetAccountId
    ? data?.accounts.find((item) => item.id === targetAccountId)
    : undefined;
  return { account, activeAccountId };
}

/** Antigravity 官方卡标题旁的「agy 在用」徽标，仅在支持切号的 Antigravity 官方卡上挂载。 */
export function AgyInUseChip({
  boundAccountId,
  renderChip,
}: {
  boundAccountId: string | null;
  renderChip: (chip: CardChip) => ReactNode;
}) {
  const { t } = useTranslation();
  const { account, activeAccountId } = useResolvedAgyAccount(boundAccountId);

  if (!account || account.id !== activeAccountId) {
    return null;
  }

  return renderChip({
    key: "agy-login",
    label: t("authCenter.inUseBy", { app: "agy" }),
    tone: "success",
  });
}

/** Antigravity 官方卡副标题（账号邮箱 + 当前模型 + 套餐），未查到账号且无模型时回落默认副标题。 */
export function AgyOfficialSubtitle({
  boundAccountId,
  agyModel,
  planSuffix,
  fallback,
}: {
  boundAccountId: string | null;
  agyModel: string | null;
  planSuffix: ReactNode;
  fallback: ReactNode;
}) {
  const { account } = useResolvedAgyAccount(boundAccountId);

  if (!account && !agyModel) {
    return fallback;
  }

  return (
    <div className="mt-0.5 flex min-w-0 items-center gap-1.5 text-caption text-fg-2">
      {account && (
        <span className="min-w-0 truncate" title={account.login}>
          {account.login}
        </span>
      )}
      {account && agyModel && <span aria-hidden="true">·</span>}
      {agyModel && (
        <span className="min-w-0 truncate" title={agyModel}>
          {agyModel}
        </span>
      )}
      {planSuffix}
    </div>
  );
}

/** CodeBuddy 官方登录卡副标题：绑定账号 + 国内/国际版站点；缺失或报错时提供重新选择入口。 */
export function CodebuddyOfficialAccountLine({
  provider,
  accountId,
  manualNote,
  onEdit,
}: {
  provider: Provider;
  accountId: string | null;
  manualNote?: string;
  onEdit: (provider: Provider) => void;
}) {
  const { t } = useTranslation();
  const { data, isSuccess, isError } = useQuery(
    managedAuthStatusQuery("codebuddy_oauth"),
  );
  const account = accountId
    ? data?.accounts.find((item) => item.id === accountId)
    : undefined;
  const regionLabel = (() => {
    const region = codebuddyRegion(account?.github_domain);
    if (region === "mainland")
      return t("authCenter.codebuddy.mainland", { defaultValue: "国内版" });
    if (region === "global")
      return t("authCenter.codebuddy.global", { defaultValue: "国际版" });
    return account?.github_domain || null;
  })();

  return (
    <div className="mt-0.5 flex min-w-0 items-center gap-2 text-caption text-fg-2">
      {account ? (
        <>
          <span
            className="min-w-0 truncate"
            title={manualNote ?? account.login}
          >
            {manualNote ??
              [account.login, regionLabel].filter(Boolean).join(" · ")}
          </span>
          {account.requires_reauth && (
            <span className="inline-flex shrink-0 items-center gap-1 text-warning-text">
              <AlertTriangle className="h-3.5 w-3.5" />
              {t("authCenter.needsReauth", {
                defaultValue: "需要重新登录",
              })}
            </span>
          )}
        </>
      ) : isError ? (
        <span className="inline-flex min-w-0 items-center gap-1 text-warning-text">
          <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
          <span className="truncate">
            {t("authCenter.codebuddy.statusLoadFailed", {
              defaultValue: "无法读取 CodeBuddy 账号，请重试。",
            })}
          </span>
        </span>
      ) : isSuccess || !accountId ? (
        <>
          <span className="inline-flex min-w-0 items-center gap-1 text-warning-text">
            <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
            <span className="truncate">
              {accountId
                ? t("codebuddy.official.accountDeleted", {
                    defaultValue: "账号已删除，请重新选择",
                  })
                : t("codebuddy.official.unbound", {
                    defaultValue: "还没有选择账号，请编辑卡片选择一个",
                  })}
            </span>
          </span>
          <button
            type="button"
            className="shrink-0 text-body font-medium text-action-text hover:underline"
            onClick={() => onEdit(provider)}
          >
            {t("codex.chooseAccount", {
              defaultValue: "选择账号",
            })}
          </button>
        </>
      ) : (
        <span className="min-w-0 truncate">
          {t("codebuddy.official.statusLoading", {
            defaultValue: "正在读取账号…",
          })}
        </span>
      )}
    </div>
  );
}

/** CodeBuddy 官方登录卡右侧额度列，与副标题共用同一份 React Query 缓存。 */
export function CodebuddyOfficialQuota({
  accountId,
}: {
  accountId: string | null;
}) {
  const { data } = useQuery(managedAuthStatusQuery("codebuddy_oauth"));
  const account = accountId
    ? data?.accounts.find((item) => item.id === accountId)
    : undefined;

  if (!account || account.requires_reauth) {
    return null;
  }

  return <CodebuddyAccountQuotaFooter accountId={account.id} />;
}

/** 绑定托管 Codex OAuth 账号的卡片副标题。 */
export function CodexOauthAccountLine({
  provider,
  accountId,
  manualNote,
  planSuffix,
  onEdit,
}: {
  provider: Provider;
  accountId?: string;
  manualNote?: string;
  planSuffix: ReactNode;
  onEdit: (provider: Provider) => void;
}) {
  const { t } = useTranslation();
  const { data, isSuccess, isError } = useQuery(
    managedAuthStatusQuery("codex_oauth"),
  );
  const account = data?.accounts.find(
    (item) => item.id === (accountId ?? data?.default_account_id),
  );
  const providerNameIncludesAccountLogin = Boolean(
    account?.login &&
      (provider.name.trim() === account.login ||
        provider.name.trim() === `OpenAI Official (${account.login})`),
  );

  return (
    <div className="mt-0.5 flex min-w-0 items-center gap-2 text-caption text-fg-2">
      {account ? (
        <>
          <span
            className="min-w-0 truncate"
            title={manualNote ?? account.login}
          >
            {manualNote ??
              (providerNameIncludesAccountLogin
                ? t("codex.openAiAccount", {
                    defaultValue: "OpenAI 账号",
                  })
                : account.login)}
          </span>
          {planSuffix}
          {account.reauth_required && (
            <span className="inline-flex shrink-0 items-center gap-1 text-warning-text">
              <AlertTriangle className="h-3.5 w-3.5" />
              {t("codexOauth.reauthBadge", "需要重新登录")}
            </span>
          )}
        </>
      ) : isError ? (
        <span className="inline-flex min-w-0 items-center gap-1 text-warning-text">
          <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
          <span className="truncate">
            {t("codex.accountStatusUnavailable", {
              defaultValue: "无法读取账号信息",
            })}
          </span>
        </span>
      ) : isSuccess ? (
        <>
          <span className="inline-flex min-w-0 items-center gap-1 text-sm text-warning-text">
            <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
            <span className="truncate">
              {t("codex.boundAccountUnavailable", {
                defaultValue: "绑定的账号不可用",
              })}
            </span>
          </span>
          <button
            type="button"
            className="shrink-0 text-body font-medium text-action-text hover:underline"
            onClick={() => onEdit(provider)}
          >
            {t("codex.chooseAccount", {
              defaultValue: "选择账号",
            })}
          </button>
        </>
      ) : (
        <span className="min-w-0 truncate">
          {t("codex.accountLoading", {
            defaultValue: "正在加载账号…",
          })}
        </span>
      )}
    </div>
  );
}

/** Copilot 反代卡副标题：绑定或默认 GitHub 账号，读不到且未绑定时回落网址。 */
export function CopilotAccountSubtitle({
  providerName,
  accountId,
  manualNote,
  planSuffix,
  fallback,
}: {
  providerName: string;
  accountId?: string;
  manualNote?: string;
  planSuffix: ReactNode;
  fallback: ReactNode;
}) {
  const { t } = useTranslation();
  const { data, isSuccess } = useQuery(
    managedAuthStatusQuery("github_copilot"),
  );
  const account = data?.accounts.find(
    (item) => item.id === (accountId ?? data?.default_account_id),
  );
  const providerNameIncludesLogin = Boolean(
    account?.login && providerName.includes(account.login),
  );

  if (account) {
    return (
      <div className="mt-0.5 flex min-w-0 items-center gap-2 text-caption text-fg-2">
        <span className="min-w-0 truncate" title={manualNote ?? account.login}>
          {manualNote ??
            (providerNameIncludesLogin
              ? t("copilot.githubAccount", {
                  defaultValue: "GitHub 账号",
                })
              : account.login)}
        </span>
        {planSuffix}
      </div>
    );
  }

  if (accountId && isSuccess) {
    return (
      <div className="mt-0.5 flex min-w-0 items-center gap-1 text-caption text-warning-text">
        <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
        <span className="min-w-0 truncate">
          {t("copilot.statusUnavailable", {
            defaultValue: "状态不可用",
          })}
        </span>
      </div>
    );
  }

  return fallback;
}

/** xAI OAuth (SuperGrok 反代) 卡副标题，挂载期间按 15 秒轮询同步过期/默认号切换状态。 */
export function XaiAccountSubtitle({
  providerName,
  accountId,
  manualNote,
  planSuffix,
  fallback,
}: {
  providerName: string;
  accountId?: string;
  manualNote?: string;
  planSuffix: ReactNode;
  fallback: ReactNode;
}) {
  const { t } = useTranslation();
  const { data, isSuccess } = useQuery(xaiAuthStatusQuery);
  const account = data?.accounts.find(
    (item) => item.id === (accountId ?? data?.default_account_id),
  );
  const providerNameIncludesLogin = Boolean(
    account?.login && providerName.includes(account.login),
  );

  if (account) {
    return (
      <div className="mt-0.5 flex min-w-0 items-center gap-2 text-caption text-fg-2">
        <span className="min-w-0 truncate" title={manualNote ?? account.login}>
          {manualNote ??
            (providerNameIncludesLogin ? t("xaiOauth.account") : account.login)}
        </span>
        {planSuffix}
        {account.requires_reauth && (
          <span className="inline-flex shrink-0 items-center gap-1 text-warning-text">
            <AlertTriangle className="h-3.5 w-3.5" />
            {t("xaiOauth.reauthRequired")}
          </span>
        )}
      </div>
    );
  }

  if (accountId && isSuccess) {
    return (
      <div className="mt-0.5 flex min-w-0 items-center gap-1 text-caption text-warning-text">
        <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
        <span className="min-w-0 truncate">
          {t("copilot.statusUnavailable")}
        </span>
      </div>
    );
  }

  return fallback;
}
