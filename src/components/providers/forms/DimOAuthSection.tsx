import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { RefreshCw } from "lucide-react";
import { dimOAuthApi, type DimSite } from "@/lib/api/dim-oauth";
import { settingsApi } from "@/lib/api";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ManagedAccountsGroup } from "@/components/settings/auth/ManagedAccountsGroup";
import {
  ManagedAccountRemoveDialog,
  type ManagedAccountRemoveTarget,
} from "./ManagedAccountRemoveDialog";
import { useManagedAccountUsers } from "./hooks/useManagedAccountUsers";
import { useDimOauth } from "./hooks/useDimOauth";

export function DimAccountUsage({ accountId }: { accountId: string }) {
  const { t } = useTranslation();
  const query = useQuery({
    queryKey: ["dim_oauth", "usage", accountId],
    queryFn: () => dimOAuthApi.usage(accountId),
    staleTime: 60000,
  });
  return (
    <div className="flex min-w-0 flex-col gap-1 text-caption text-fg-2">
      <div className="flex items-center gap-2">
        <span>{query.data?.plan_name ?? t("dimOauth.usage")}</span>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-6 w-6"
          disabled={query.isFetching}
          onClick={() => void query.refetch()}
          aria-label={t("common.refresh")}
        >
          <RefreshCw size={12} />
        </Button>
      </div>
      {query.isError && (
        <span role="alert">
          {t("dimOauth.usageFailed")}: {String(query.error)}
        </span>
      )}
      {query.isPending && <span>{t("common.loading")}</span>}
      {query.data && (
        <>
          {query.data.remaining_units !== null && (
            <span>
              {t("dimOauth.remaining", {
                amount: query.data.remaining_units.toLocaleString(),
              })}
            </span>
          )}
          {query.data.windows.map((window, index) => (
            <div key={`${window.name}-${index}`}>
              <span>
                {window.name}: {window.used.toLocaleString()} /{" "}
                {window.total.toLocaleString()} {window.unit}
              </span>
              {window.resets_at && (
                <span className="ms-2">
                  {t("dimOauth.resetsAt", {
                    time: new Date(window.resets_at).toLocaleString(),
                  })}
                </span>
              )}
            </div>
          ))}
          <div className="flex flex-wrap gap-3">
            <button
              type="button"
              className="text-primary hover:underline"
              onClick={() =>
                void settingsApi.openExternal(query.data!.subscription_url)
              }
            >
              {t("dimOauth.subscription")}
            </button>
            <button
              type="button"
              className="text-primary hover:underline"
              onClick={() =>
                void settingsApi.openExternal(query.data!.pricing_url)
              }
            >
              {t("dimOauth.pricing")}
            </button>
          </div>
        </>
      )}
    </div>
  );
}

export function DimOAuthSection() {
  const { t } = useTranslation();
  const auth = useDimOauth();
  const [site, setSite] = useState<DimSite>("cn");
  const [removeTarget, setRemoveTarget] =
    useState<ManagedAccountRemoveTarget | null>(null);
  const usersFor = useManagedAccountUsers("dim_oauth", auth.defaultAccountId);
  const add = () => auth.start.mutate(site);
  return (
    <ManagedAccountsGroup
      slug="dimagent"
      name="DimAgent"
      iconName="dimagent"
      help={t("dimOauth.help")}
      helpSide="bottom"
      accounts={auth.accounts.map((account) => ({
        id: account.id,
        login: account.login,
        details: [
          t(`dimOauth.site.${account.site}`),
          new Date(account.authenticated_at * 1000).toLocaleDateString(),
        ],
        isDefault: account.id === auth.defaultAccountId,
        needsReauth: account.requires_reauth,
        users: usersFor([account.id]),
        quota: account.requires_reauth ? undefined : (
          <DimAccountUsage accountId={account.id} />
        ),
      }))}
      status={
        auth.status.isError
          ? "error"
          : auth.status.isSuccess
            ? "ready"
            : "loading"
      }
      statusErrorText={String(auth.status.error ?? "")}
      onRetryStatus={() => void auth.status.refetch()}
      emptyText={t("dimOauth.empty")}
      onAdd={add}
      canReauth
      onReauth={(id) => {
        const account = auth.accounts.find((a) => a.id === id);
        if (account) {
          setSite(account.site);
          auth.start.mutate(account.site);
        }
      }}
      loginControl={
        <Select
          value={site}
          onValueChange={(value: DimSite) => setSite(value)}
          disabled={!!auth.flow || auth.start.isPending}
        >
          <SelectTrigger className="w-36" aria-label={t("dimOauth.siteLabel")}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="cn">{t("dimOauth.site.cn")}</SelectItem>
            <SelectItem value="global">{t("dimOauth.site.global")}</SelectItem>
          </SelectContent>
        </Select>
      }
      onSetDefault={(id) => auth.setDefault.mutate(id)}
      settingDefault={auth.setDefault.isPending}
      onRemove={(id, login) =>
        setRemoveTarget({ kind: "one", accountId: id, login })
      }
      removing={auth.remove.isPending}
      notice={
        auth.remove.error || auth.setDefault.error ? (
          <p role="alert" className="text-destructive">
            {String(auth.remove.error ?? auth.setDefault.error)}
          </p>
        ) : undefined
      }
      login={{
        starting: auth.start.isPending && !auth.flow,
        polling: !!auth.flow,
        pollingState: auth.pollingState,
        deviceCode: auth.flow
          ? { user_code: "", verification_uri: auth.flow.auth_url }
          : null,
        error: auth.error,
        onCancel: auth.cancel,
        onRetry: add,
      }}
    >
      <ManagedAccountRemoveDialog
        target={removeTarget}
        serviceName="DimAgent"
        users={
          removeTarget?.kind === "one" ? usersFor([removeTarget.accountId]) : []
        }
        othersRemain={auth.accounts.length > 1}
        pending={auth.remove.isPending}
        onCancel={() => setRemoveTarget(null)}
        onConfirm={() => {
          if (removeTarget?.kind === "one")
            auth.remove.mutate(removeTarget.accountId, {
              onSuccess: () => setRemoveTarget(null),
            });
        }}
      />
    </ManagedAccountsGroup>
  );
}
