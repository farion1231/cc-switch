import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Copy, ExternalLink, Loader2, RefreshCw } from "lucide-react";
import { ProviderIcon } from "@/components/ProviderIcon";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { Button } from "@/components/ui/button";
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
import { settingsApi } from "@/lib/api";
import type { KimiOAuthAccount, KimiRegion } from "@/lib/api/kimiOAuth";
import { extractErrorMessage } from "@/utils/errorUtils";
import { toast } from "@/lib/toast";
import { useKimiOAuth } from "./forms/hooks/useKimiOAuth";
export function KimiAccountLogin() {
  const { t } = useTranslation();
  const auth = useKimiOAuth();
  const [region, setRegion] = useState<KimiRegion | null>(null);
  const [confirmLogout, setConfirmLogout] = useState<KimiOAuthAccount | null>(
    null,
  );
  const selectedRegion = region ?? "mainland-cn";
  const accounts = auth.status.data?.accounts ?? [];
  const busy =
    auth.starting ||
    Boolean(auth.flow) ||
    auth.logout.isPending ||
    auth.useAccount.isPending;

  useEffect(() => {
    if (auth.logout.isSuccess) setConfirmLogout(null);
  }, [auth.logout.isSuccess]);

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
          {accounts.map((account) => (
            <div
              key={account.id}
              role="listitem"
              className="flex items-center gap-3 rounded-panel border border-direct-border bg-direct-soft px-4 py-3"
            >
              <ProviderIcon icon="kimi" name={account.name} size={24} />
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="text-strong text-fg-1">{account.name}</span>
                  {auth.status.data?.currentAccountId === account.id && (
                    <span className="text-badge text-direct">
                      {t("providerCard.status.current")}
                    </span>
                  )}
                  <span className="rounded-full border border-border-strong px-1.5 text-badge text-fg-2">
                    {t("providerCard.chip.official")}
                  </span>
                </div>
                <p className="text-caption text-fg-2">
                  {account.authenticated
                    ? t("kimi.oauth.signedIn", {
                        region: t(`kimi.oauth.${account.region}`),
                      })
                    : t("kimi.oauth.signedOut")}
                </p>
                {account.error && (
                  <p className="text-caption text-danger">{account.error}</p>
                )}
              </div>
              <Button
                type="button"
                variant="neutral"
                size="compact"
                disabled={busy || !account.authenticated}
                onClick={() => {
                  void auth.useAccount.mutateAsync(account.id).catch(() => {});
                }}
              >
                {t("kimi.oauth.useAccount")}
              </Button>
              <Button
                type="button"
                variant="quiet"
                size="compact"
                disabled={busy}
                onClick={() => void auth.start(account.region, account.id)}
              >
                {t("kimi.oauth.signInAgain")}
              </Button>
              <Button
                type="button"
                variant="quiet"
                size="compact"
                disabled={busy || !account.authenticated}
                onClick={() => setConfirmLogout(account)}
              >
                {t("kimi.oauth.signOut")}
              </Button>
            </div>
          ))}
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
        isOpen={confirmLogout !== null}
        title={t("kimi.oauth.signOut")}
        message={t("kimi.oauth.signOutConfirm", { name: confirmLogout?.name })}
        pending={auth.logout.isPending}
        confirmText={t("kimi.oauth.signOut")}
        onConfirm={() => {
          if (confirmLogout) auth.logout.mutate(confirmLogout.id);
        }}
        onCancel={() => setConfirmLogout(null)}
      />
    </section>
  );
}
