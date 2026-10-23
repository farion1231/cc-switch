import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Copy,
  ExternalLink,
  Loader2,
  MoreHorizontal,
  RefreshCw,
} from "lucide-react";
import { ProviderIcon } from "@/components/ProviderIcon";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Label } from "@/components/ui/label";
import { Notice } from "@/components/ui/notice";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { copyText } from "@/lib/clipboard";
import { cn } from "@/lib/utils";
import { settingsApi } from "@/lib/api";
import type { KimiOAuthAccount, KimiRegion } from "@/lib/api/kimiOAuth";
import { extractErrorMessage } from "@/utils/errorUtils";
import { toast } from "@/lib/toast";
import { KimiAccountQuota } from "@/components/settings/auth/AccountQuota";
import { useKimiOAuth } from "./forms/hooks/useKimiOAuth";

// 与授权中心其他账号组的标签同款
const PILL =
  "inline-flex h-[18px] shrink-0 items-center rounded-full px-1.5 text-badge";

export function KimiAccountLogin() {
  const { t } = useTranslation();
  const auth = useKimiOAuth();
  const [region, setRegion] = useState<KimiRegion | null>(null);
  const [confirmRemove, setConfirmRemove] = useState<KimiOAuthAccount | null>(
    null,
  );
  const selectedRegion = region ?? "mainland-cn";
  const accounts = auth.status.data?.accounts ?? [];
  const busy =
    auth.starting ||
    Boolean(auth.flow) ||
    auth.remove.isPending ||
    auth.useAccount.isPending;

  useEffect(() => {
    if (auth.remove.isSuccess) setConfirmRemove(null);
  }, [auth.remove.isSuccess]);

  const openPage = async (url: string) => {
    try {
      await settingsApi.openExternal(url);
    } catch (e) {
      toast.error(extractErrorMessage(e));
    }
  };

  const loginButton = (
    <Button
      type="button"
      variant="quiet"
      size="compact"
      disabled={busy || auth.status.isPending}
      onClick={() => void auth.start(selectedRegion)}
    >
      {auth.starting && <Loader2 className="h-4 w-4 animate-spin" />}
      {accounts.length ? t("kimi.oauth.addAccount") : t("kimi.oauth.signIn")}
    </Button>
  );

  return (
    <section
      aria-label={t("kimi.oauth.title")}
      className="space-y-4 rounded-panel border border-border bg-surface p-4"
    >
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-center gap-2">
          <ProviderIcon icon="kimi" name="Kimi Code" size={24} />
          <h3 className="text-strong text-fg-1">Kimi Code</h3>
        </div>
        {loginButton}
      </div>
      <p className="text-body text-fg-2">{t("kimi.oauth.description")}</p>
      <div className="flex flex-wrap items-center gap-3">
        <Label htmlFor="kimi-oauth-region">{t("kimi.oauth.region")}</Label>
        <Select
          value={selectedRegion}
          disabled={busy}
          onValueChange={(value) => setRegion(value as KimiRegion)}
        >
          <SelectTrigger id="kimi-oauth-region" className="w-40">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="mainland-cn">
              {t("kimi.oauth.mainland-cn")}
            </SelectItem>
            <SelectItem value="global">{t("kimi.oauth.global")}</SelectItem>
          </SelectContent>
        </Select>
        <span role="status" className="text-caption text-fg-2">
          {auth.status.isPending
            ? t("common.loading")
            : auth.status.isError
              ? t("kimi.oauth.statusFailed")
              : accounts.length
                ? t("kimi.oauth.accountCount", { count: accounts.length })
                : auth.status.data?.primaryHidden
                  ? t("kimi.oauth.primaryHidden")
                  : t("kimi.oauth.signedOut")}
        </span>
        <Button
          type="button"
          variant="quiet"
          size="icon-compact"
          disabled={busy || auth.status.isFetching}
          aria-label={t("kimi.oauth.refresh")}
          onClick={() => void auth.status.refetch()}
        >
          <RefreshCw className="h-4 w-4" />
        </Button>
      </div>
      {accounts.length > 0 && (
        <div role="list" className="space-y-2">
          {accounts.map((account) => {
            const current = auth.status.data?.currentAccountId === account.id;
            // 正在用的分开账号要先切走才能删（后端也会拒）；Kimi 自己的登录随时可移出列表
            const removeBlocked = current && !account.primary;
            const status = account.authenticated
              ? t("kimi.oauth.signedIn", {
                  region: t(`kimi.oauth.${account.region}`),
                })
              : t("kimi.oauth.signedOut");
            const detail = account.primary
              ? `${status} · ${t("kimi.oauth.primaryHint")}`
              : status;
            return (
              <div
                key={account.id}
                role="listitem"
                className={cn(
                  "flex min-h-[56px] items-center gap-3 rounded-panel border py-2 pe-2.5 ps-4",
                  current
                    ? "border-direct-border bg-direct-soft"
                    : "border-border bg-surface",
                )}
              >
                <ProviderIcon icon="kimi" name={account.name} size={24} />
                <div className="flex min-w-0 flex-1 flex-col">
                  <div className="flex min-w-0 items-center gap-1.5">
                    <span
                      title={account.name}
                      className="min-w-0 truncate text-body font-medium text-fg-1"
                    >
                      {account.name}
                    </span>
                    {current && (
                      <span className={cn(PILL, "bg-direct text-direct-on")}>
                        {t("providerCard.status.current")}
                      </span>
                    )}
                    <span
                      className={cn(
                        PILL,
                        "border border-border-strong text-fg-2",
                      )}
                    >
                      {t("providerCard.chip.official")}
                    </span>
                  </div>
                  <span
                    title={detail}
                    className="min-w-0 truncate text-caption text-fg-2"
                  >
                    {detail}
                  </span>
                  {account.error && (
                    <span className="text-caption text-danger-text [overflow-wrap:anywhere]">
                      {account.error}
                    </span>
                  )}
                </div>
                {account.authenticated && (
                  <KimiAccountQuota
                    accountId={account.id}
                    login={account.name}
                  />
                )}
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button
                      type="button"
                      variant="quiet"
                      size="icon-compact"
                      aria-label={t("authCenter.accountMenu", {
                        defaultValue: "{{login}} 的更多操作",
                        login: account.name,
                      })}
                    >
                      <MoreHorizontal className="h-[15px] w-[15px]" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent
                    align="end"
                    className="min-w-[148px] rounded-panel bg-surface p-1 shadow-v7-md"
                  >
                    {!current && (
                      <DropdownMenuItem
                        disabled={busy || !account.authenticated}
                        onSelect={() => {
                          void auth.useAccount
                            .mutateAsync(account.id)
                            .catch(() => {});
                        }}
                        className="h-[30px] rounded-control px-2.5 text-body"
                      >
                        {t("kimi.oauth.useAccount")}
                      </DropdownMenuItem>
                    )}
                    <DropdownMenuItem
                      disabled={busy}
                      onSelect={() =>
                        void auth.start(account.region, account.id)
                      }
                      className="h-[30px] rounded-control px-2.5 text-body"
                    >
                      {t("kimi.oauth.signInAgain")}
                    </DropdownMenuItem>
                    <DropdownMenuItem
                      disabled={busy || removeBlocked}
                      onSelect={() => setConfirmRemove(account)}
                      className="min-h-[30px] flex-col items-start justify-center gap-0 rounded-control px-2.5 text-body text-danger-text focus:text-danger-text data-[disabled]:opacity-100"
                    >
                      <span className={cn(removeBlocked && "text-fg-3")}>
                        {account.primary
                          ? t("kimi.oauth.dismissAccount")
                          : t("kimi.oauth.removeAccount")}
                      </span>
                      {removeBlocked && (
                        <span className="text-caption text-fg-3">
                          {t("kimi.oauth.removeCurrentHint")}
                        </span>
                      )}
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              </div>
            );
          })}
        </div>
      )}
      {auth.starting && (
        <Button
          type="button"
          variant="quiet"
          size="compact"
          onClick={auth.cancel}
        >
          {t("common.cancel")}
        </Button>
      )}
      {auth.flow && (
        <div
          className="flex flex-wrap items-center gap-3 rounded-panel border border-border bg-subtle px-4 py-3"
          aria-live="polite"
        >
          <Loader2 aria-hidden="true" className="h-4 w-4 animate-spin" />
          <span className="text-body">{t("kimi.oauth.waiting")}</span>
          <code className="rounded-control border border-border bg-surface px-2.5 py-1 font-mono text-section tracking-widest">
            {auth.flow.userCode}
          </code>
          <Button
            type="button"
            variant="quiet"
            size="icon-compact"
            aria-label={t("kimi.oauth.copyCode")}
            onClick={() => {
              void copyText(auth.flow!.userCode)
                .then(() => toast.success(t("kimi.oauth.copied")))
                .catch((e) => toast.error(extractErrorMessage(e)));
            }}
          >
            <Copy className="h-4 w-4" />
          </Button>
          <Button
            type="button"
            variant="neutral"
            size="compact"
            onClick={() => void openPage(auth.flow!.verificationUri)}
          >
            <ExternalLink className="h-4 w-4" />
            {t("kimi.oauth.openPage")}
          </Button>
          <Button
            type="button"
            variant="quiet"
            size="compact"
            onClick={auth.cancel}
          >
            {t("common.cancel")}
          </Button>
          <p className="basis-full text-caption text-fg-2">
            {t("kimi.oauth.manualHint")}
          </p>
          <a
            className="min-w-0 basis-full break-all text-caption text-fg-2 underline"
            href={auth.flow.verificationUri}
            onClick={(event) => {
              event.preventDefault();
              void openPage(auth.flow!.verificationUri);
            }}
          >
            {auth.flow.verificationUri}
          </a>
        </div>
      )}
      {(auth.error || auth.status.isError) && (
        <div role="alert">
          <Notice
            tone="danger"
            title={t("kimi.oauth.error")}
            actions={
              auth.retryable ? (
                <Button
                  type="button"
                  variant="neutral"
                  size="compact"
                  onClick={auth.retry}
                >
                  {t("common.retry")}
                </Button>
              ) : undefined
            }
          >
            {auth.error || extractErrorMessage(auth.status.error)}
          </Notice>
        </div>
      )}
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
          { name: confirmRemove?.name },
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
    </section>
  );
}
