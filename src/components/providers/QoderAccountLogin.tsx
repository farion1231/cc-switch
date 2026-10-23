import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ExternalLink, Loader2, RefreshCw, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Notice } from "@/components/ui/notice";
import { settingsApi } from "@/lib/api";
import {
  qoderApi,
  qoderKeys,
  type QoderAppId,
  type QoderAccount,
} from "@/lib/api/qoder";
import { extractErrorMessage } from "@/utils/errorUtils";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { HoverTip } from "@/components/ui/hover-tip";
import { cn } from "@/lib/utils";
import { ProviderIcon } from "@/components/ProviderIcon";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

export function QoderAccountLogin({
  app,
  onAppChange,
}: {
  app: QoderAppId;
  onAppChange: (app: QoderAppId) => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const auth = useQuery({
    queryKey: qoderKeys.auth(app),
    queryFn: () => qoderApi.getAuthStatus(app),
    retry: false,
    staleTime: 30_000,
  });
  const [flowId, setFlowId] = useState<string | null>(null);
  const [working, setWorking] = useState(false);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState("");
  const [deleting, setDeleting] = useState<QoderAccount | null>(null);
  const [removing, setRemoving] = useState(false);
  const [deleteError, setDeleteError] = useState("");
  const generation = useRef(0);
  const activeFlow = useRef<string | null>(null);
  const openedFlow = useRef<string | null>(null);
  const completedFlow = useRef<string | null>(null);
  const progress = useQuery({
    queryKey: qoderKeys.login(app, flowId),
    queryFn: () => qoderApi.pollLogin(app, flowId!),
    enabled: Boolean(flowId),
    refetchInterval: 1500,
    retry: false,
    gcTime: 0,
  });

  useEffect(
    () => () => {
      generation.current += 1;
      const id = activeFlow.current;
      activeFlow.current = null;
      if (id) void qoderApi.cancelLogin(app, id).catch(() => {});
    },
    [app],
  );

  const openBrowser = async (url: string) => {
    try {
      await settingsApi.openExternal(url);
    } catch {
      setError(t("qoder.loginBrowserFailed"));
    }
  };

  useEffect(() => {
    if (!flowId) return;
    const result = progress.data;
    if (
      result?.authUrl &&
      !result.complete &&
      !result.error &&
      !progress.error &&
      openedFlow.current !== flowId
    ) {
      openedFlow.current = flowId;
      void openBrowser(result.authUrl);
    }
    if (result?.error || progress.error) {
      setError(result?.error || extractErrorMessage(progress.error));
      setFlowId(null);
      activeFlow.current = null;
      setWorking(false);
      void qoderApi.cancelLogin(app, flowId).catch(() => {});
      return;
    }
    if (result?.complete && completedFlow.current !== flowId) {
      completedFlow.current = flowId;
      const current = generation.current;
      setFlowId(null);
      void (async () => {
        try {
          await qoderApi.cancelLogin(app, flowId);
          activeFlow.current = null;
          const status = await qoderApi.getAuthStatus(app);
          if (current !== generation.current) return;
          queryClient.setQueryData(qoderKeys.auth(app), status);
          await queryClient.invalidateQueries({
            queryKey: qoderKeys.models(app),
          });
          if (
            !status.accounts?.some((account) => account.id === result.accountId)
          )
            throw new Error(t("qoder.loginNotConfirmed"));
        } catch (cause) {
          if (current === generation.current)
            setError(extractErrorMessage(cause));
        } finally {
          if (current === generation.current) setWorking(false);
        }
      })();
    }
  }, [app, flowId, progress.data, progress.error, queryClient, t]);

  const start = async (accountId?: string) => {
    if (working || removing) return;
    const current = ++generation.current;
    setWorking(true);
    setStarting(true);
    setError("");
    try {
      const id = await qoderApi.startLogin(app, accountId);
      if (current !== generation.current) {
        await qoderApi.cancelLogin(app, id);
        return;
      }
      activeFlow.current = id;
      setFlowId(id);
    } catch (cause) {
      if (current === generation.current) setError(extractErrorMessage(cause));
    } finally {
      if (current === generation.current) {
        setStarting(false);
        if (!activeFlow.current) setWorking(false);
      }
    }
  };

  const select = async (accountId?: string) => {
    if (working || removing) return;
    setWorking(true);
    setError("");
    try {
      if (accountId) {
        const status = await qoderApi.useAccount(app, accountId);
        queryClient.setQueryData(qoderKeys.auth(app), status);
      } else {
        await qoderApi.setModel(app, "auto");
      }
      await queryClient.invalidateQueries({ queryKey: qoderKeys.models(app) });
    } catch (cause) {
      setError(extractErrorMessage(cause));
    } finally {
      setWorking(false);
    }
  };

  const remove = async () => {
    if (!deleting || removing) return;
    setRemoving(true);
    setDeleteError("");
    try {
      const status = await qoderApi.deleteAccount(app, deleting.id);
      queryClient.setQueryData(qoderKeys.auth(app), status);
      await queryClient.invalidateQueries({ queryKey: qoderKeys.models(app) });
      setDeleting(null);
    } catch (cause) {
      setDeleteError(extractErrorMessage(cause));
    } finally {
      setRemoving(false);
    }
  };
  const accounts = auth.data?.accounts ?? [];
  const label = (account: QoderAccount) =>
    account.username || account.email || "Qoder";
  const addButton = (
    <Button
      type="button"
      variant="quiet"
      size="compact"
      disabled={working || removing || !auth.data?.installed}
      onClick={() => void start()}
    >
      {starting && <Loader2 className="h-4 w-4 animate-spin" />}
      {t(accounts.length ? "qoder.addAccount" : "qoder.loginWithBrowser")}
    </Button>
  );
  const cancel = async () => {
    generation.current += 1;
    const id = activeFlow.current;
    setFlowId(null);
    setStarting(false);
    try {
      if (id) await qoderApi.cancelLogin(app, id);
    } catch (cause) {
      setError(extractErrorMessage(cause));
    } finally {
      activeFlow.current = null;
      setWorking(false);
      void auth.refetch();
    }
  };

  return (
    <section
      aria-label={APP_DISPLAY_NAME[app]}
      className="space-y-4 rounded-panel border border-border bg-surface p-4"
    >
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-center gap-2">
          <ProviderIcon icon="qoder" name={APP_DISPLAY_NAME[app]} size={24} />
          <h3 className="text-strong text-fg-1">{APP_DISPLAY_NAME[app]}</h3>
        </div>
        {addButton}
      </div>
      <p className="text-body text-fg-2">{t("qoder.loginDescription")}</p>
      <div className="flex flex-wrap items-center gap-3">
        <Label htmlFor="qoder-auth-edition">{t("qoder.edition")}</Label>
        <Select
          value={app}
          disabled={working || removing}
          onValueChange={(value) => onAppChange(value as QoderAppId)}
        >
          <SelectTrigger id="qoder-auth-edition" className="w-40">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="qoder">{t("qoder.globalEdition")}</SelectItem>
            <SelectItem value="qoder-cn">{t("qoder.chinaEdition")}</SelectItem>
          </SelectContent>
        </Select>
        <span role="status" className="text-caption text-fg-2">
          {auth.isPending
            ? t("common.loading")
            : auth.isError
              ? t("qoder.statusFailed")
              : accounts.length
                ? t("qoder.accountCount", { count: accounts.length })
                : !auth.data.installed
                  ? t("common.notInstalled")
                  : auth.data.loggedIn
                    ? t("qoder.signedIn")
                    : t("qoder.signedOut")}
        </span>
        <Button
          type="button"
          variant="quiet"
          size="icon-compact"
          disabled={working || removing || auth.isFetching}
          aria-label={t("qoder.refreshAccounts")}
          onClick={() => void auth.refetch()}
        >
          <RefreshCw
            className={cn("h-4 w-4", auth.isFetching && "animate-spin")}
          />
        </Button>
      </div>
      {auth.isError && (
        <Notice
          tone="danger"
          title={extractErrorMessage(auth.error)}
          actions={
            <Button
              type="button"
              variant="neutral"
              size="compact"
              onClick={() => void auth.refetch()}
            >
              {t("common.retry")}
            </Button>
          }
        />
      )}
      {auth.data && !auth.data.installed && (
        <Notice tone="warning" title={t("qoder.loginInstallRequired")} />
      )}
      {auth.data?.loggedIn && accounts.length === 0 && (
        <Notice
          title={t("qoder.loggedInAs", {
            name: auth.data.email || auth.data.username || "Qoder",
          })}
        >
          {auth.data.plan}
        </Notice>
      )}
      {accounts.length > 0 && (
        <div role="list" className="space-y-2">
          {accounts.map((account) => (
            <div
              key={account.id}
              role="listitem"
              className={cn(
                "flex items-center gap-3 rounded-panel border px-4 py-3",
                account.isCurrent
                  ? "border-direct-border bg-direct-soft"
                  : "border-border bg-surface",
              )}
            >
              <div className="min-w-0 flex-1">
                <div className="flex min-w-0 items-center gap-2">
                  <span className="truncate text-strong text-fg-1">
                    {label(account)}
                  </span>
                  {account.isCurrent && (
                    <span className="inline-flex shrink-0 items-center gap-1.5 text-caption text-fg-2">
                      <span className="h-1.5 w-1.5 rounded-full bg-direct" />
                      {t("qoder.currentAccount")}
                    </span>
                  )}
                </div>
                <p className="truncate text-caption text-fg-2">
                  {[
                    account.email !== account.username ? account.email : null,
                    account.plan,
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                </p>
              </div>
              <Button
                type="button"
                variant="quiet"
                size="compact"
                disabled={working || removing || !auth.data?.installed}
                onClick={() => void start(account.id)}
              >
                {t("qoder.signInAgain")}
              </Button>
              <Button
                type="button"
                variant="neutral"
                size="compact"
                disabled={working || removing || !auth.data?.installed}
                onClick={() => void select(account.id)}
              >
                {t("qoder.useAccount")}
              </Button>
              <HoverTip
                content={t("qoder.deleteAccount", { name: label(account) })}
              >
                <Button
                  type="button"
                  variant="quiet"
                  size="icon-compact"
                  className="text-danger-text hover:bg-danger-soft hover:text-danger-text"
                  aria-label={t("qoder.deleteAccount", {
                    name: label(account),
                  })}
                  disabled={working || removing}
                  onClick={() => {
                    setDeleteError("");
                    setDeleting(account);
                  }}
                >
                  <Trash2 className="h-4 w-4" strokeWidth={1.5} />
                </Button>
              </HoverTip>
            </div>
          ))}
        </div>
      )}
      {error && (
        <p role="alert" className="text-body text-danger-text">
          {error}
        </p>
      )}
      {flowId && (
        <Notice title={t("qoder.loginWaiting")}>
          {progress.data?.authUrl && (
            <Button
              type="button"
              variant="quiet"
              size="compact"
              onClick={() => void openBrowser(progress.data!.authUrl!)}
            >
              <ExternalLink className="h-3.5 w-3.5" />
              {t("qoder.loginOpenBrowser")}
            </Button>
          )}
        </Notice>
      )}
      {((accounts.length === 0 && auth.data?.loggedIn) ||
        starting ||
        flowId) && (
        <div className="flex gap-2">
          {accounts.length === 0 && auth.data?.loggedIn && (
            <Button
              type="button"
              variant="solid"
              size="regular"
              disabled={working || removing || !auth.data?.installed}
              onClick={() => void select()}
            >
              {working && <Loader2 className="h-4 w-4 animate-spin" />}
              {t("qoder.useAccount")}
            </Button>
          )}
          {(starting || flowId) && (
            <Button
              type="button"
              variant="neutral"
              size="regular"
              onClick={() => void cancel()}
            >
              {t("common.cancel")}
            </Button>
          )}
        </div>
      )}
      <ConfirmDialog
        isOpen={Boolean(deleting)}
        title={t("qoder.deleteAccountTitle")}
        message={
          t(
            deleting?.isCurrent
              ? "qoder.deleteCurrentAccountConfirm"
              : "qoder.deleteAccountConfirm",
            { name: deleting ? label(deleting) : "" },
          ) + (deleteError ? `\n\n${deleteError}` : "")
        }
        confirmText={t("common.delete")}
        pending={removing}
        onConfirm={() => void remove()}
        onCancel={() => {
          if (!removing) setDeleting(null);
        }}
      />
    </section>
  );
}
