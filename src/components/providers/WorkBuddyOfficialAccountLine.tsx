import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { AlertTriangle } from "lucide-react";
import { authApi } from "@/lib/api";
import type { Provider } from "@/types";
import { WORKBUDDY_ACCOUNT_AUTH_PROVIDER } from "@/config/workbuddyProviderPresets";
import { workbuddyPlanLabel } from "@/components/settings/auth/workbuddyPlan";
import {
  WORKBUDDY_STATUS_KEY,
  boundWorkBuddyAccountId,
} from "./workbuddyOfficial";

interface WorkBuddyOfficialAccountLineProps {
  provider: Provider;
  onEdit: (provider: Provider) => void;
}

/** WorkBuddy 官方登录卡的第二行：绑定账号的昵称和套餐；账号删了就提示重新选择。 */
export function WorkBuddyOfficialAccountLine({
  provider,
  onEdit,
}: WorkBuddyOfficialAccountLineProps) {
  const { t } = useTranslation();
  const bound = boundWorkBuddyAccountId(provider);
  const { data, isSuccess, isError } = useQuery({
    queryKey: WORKBUDDY_STATUS_KEY,
    queryFn: () => authApi.authGetStatus(WORKBUDDY_ACCOUNT_AUTH_PROVIDER),
    staleTime: 30_000,
  });
  const account = bound
    ? data?.accounts.find((item) => item.id === bound)
    : undefined;
  const note = provider.notes?.trim();

  if (account) {
    const parts = [
      account.login,
      ...(account.account_kind
        ? [workbuddyPlanLabel(t, account.account_kind)]
        : []),
    ].join(" · ");
    return (
      <div className="mt-0.5 flex min-w-0 items-center gap-2 text-caption text-fg-2">
        <span className="min-w-0 truncate" title={note ?? parts}>
          {note ?? parts}
        </span>
        {account.requires_reauth && (
          <span className="inline-flex shrink-0 items-center gap-1 text-warning-text">
            <AlertTriangle className="h-3.5 w-3.5" />
            {t("authCenter.needsReauth")}
          </span>
        )}
      </div>
    );
  }
  if (isError) {
    return (
      <div className="mt-0.5 flex min-w-0 items-center gap-1 text-caption text-warning-text">
        <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
        <span className="truncate">{t("codex.accountStatusUnavailable")}</span>
      </div>
    );
  }
  if (!isSuccess && bound) {
    return (
      <div className="mt-0.5 min-w-0 truncate text-caption text-fg-2">
        {t("codex.accountLoading")}
      </div>
    );
  }
  return (
    <div className="mt-0.5 flex min-w-0 items-center gap-2 text-caption">
      <span className="inline-flex min-w-0 items-center gap-1 text-warning-text">
        <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
        <span className="truncate">
          {bound
            ? t("workbuddy.official.accountDeleted")
            : t("workbuddy.official.accountUnset")}
        </span>
      </span>
      <button
        type="button"
        className="shrink-0 text-body font-medium text-action-text hover:underline"
        onClick={() => onEdit(provider)}
      >
        {t("codex.chooseAccount")}
      </button>
    </div>
  );
}
