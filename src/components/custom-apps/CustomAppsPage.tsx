import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { LayoutGrid } from "lucide-react";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  customAppsApi,
  customAppsKey,
  customProvidersKey,
  useCustomApps,
} from "@/lib/api/customApps";
import { useProxyStatus } from "@/hooks/useProxyStatus";

interface Props {
  appId: string | null;
  onSelectApp: (id: string | null) => void;
}

export function CustomAppsPage({ appId, onSelectApp }: Props) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const apps = useCustomApps();
  const app = apps.data?.find(({ id }) => id === appId);
  const {
    status: proxyStatus,
    startProxyServer,
    isStarting,
  } = useProxyStatus();
  const [newToken, setNewToken] = useState<{ id: string; token: string }>();
  const [busy, setBusy] = useState(false);
  const [confirmRotation, setConfirmRotation] = useState(false);
  const [editingId, setEditingId] = useState<string>();
  const [deleting, setDeleting] = useState<{ providerId?: string }>();
  useEffect(() => {
    setEditingId(undefined);
    setDeleting(undefined);
    setConfirmRotation(false);
    setError(undefined);
  }, [appId]);
  useEffect(() => {
    if (appId && apps.isSuccess && !app) onSelectApp(null);
  }, [appId, app, apps.isSuccess, onSelectApp]);
  const [error, setError] = useState<string>();
  const providers = useQuery({
    queryKey: customProvidersKey(appId ?? ""),
    queryFn: () => customAppsApi.providers(appId ?? ""),
    enabled: Boolean(app),
  });

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError(undefined);
    try {
      await action();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  };

  const onCreate = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const form = event.currentTarget;
    const data = new FormData(form);
    void run(async () => {
      const created = await customAppsApi.create(String(data.get("name")));
      await queryClient.invalidateQueries({ queryKey: customAppsKey });
      if (created.token) setNewToken({ id: created.id, token: created.token });
      form.reset();
      onSelectApp(created.id);
    });
  };

  const onAddProvider = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!app) return;
    const form = event.currentTarget;
    const data = new FormData(form);
    void run(async () => {
      const provider = {
        name: String(data.get("name")),
        baseUrl: String(data.get("baseUrl")),
        model: String(data.get("model")),
        apiKey: data.get("clearKey")
          ? ""
          : String(data.get("apiKey")) || (editingId ? undefined : ""),
      };
      if (editingId)
        await customAppsApi.updateProvider(app.id, editingId, provider);
      else await customAppsApi.addProvider(app.id, provider);
      setEditingId(undefined);
      await queryClient.invalidateQueries({
        queryKey: customProvidersKey(app.id),
      });
      form.reset();
    });
  };

  const address = proxyStatus?.address;
  // URL normalizes dotted IPv4-mapped IPv6 to hexadecimal before checking it.
  let loopback = false;
  if (address) {
    try {
      const host = new URL(
        `http://${address.includes(":") && !address.startsWith("[") ? `[${address}]` : address}`,
      ).hostname.replace(/^\[|\]$/g, "");
      loopback =
        host === "localhost" ||
        host === "::1" ||
        /^127(?:\.[0-9]{1,3}){3}$/.test(host) ||
        /^::ffff:7f[0-9a-f]{2}:[0-9a-f]{1,4}$/.test(host);
    } catch {
      /* Invalid listener addresses are never advertised as usable. */
    }
  }
  const editing = providers.data?.providers.find(({ id }) => id === editingId);
  const endpoint =
    app && proxyStatus?.running && loopback
      ? `http://${address?.includes(":") && !address.startsWith("[") ? `[${address}]` : address}:${proxyStatus.port}/apps/${app.id}/v1`
      : undefined;
  const failure = error ?? apps.error?.message ?? providers.error?.message;

  return (
    <>
      <AppPageHeader
        icon={<LayoutGrid className="h-5 w-5" />}
        title={app?.name ?? t("customApps.title")}
      />
      <div
        id="main-content"
        className="min-h-0 flex-1 overflow-y-auto scroll-stable"
      >
        <div className="mx-auto flex w-full max-w-3xl flex-col gap-5 p-5">
          {failure && (
            <p role="alert" className="text-sm text-destructive">
              {failure}
            </p>
          )}
          {apps.isPending ? (
            <p>{t("common.loading")}</p>
          ) : app ? (
            <>
              <form
                key={`rename-${app.id}-${app.name}`}
                className="flex items-end gap-3"
                onSubmit={(event) => {
                  event.preventDefault();
                  const name = String(
                    new FormData(event.currentTarget).get("name"),
                  );
                  void run(async () => {
                    await customAppsApi.rename(app.id, name);
                    await queryClient.invalidateQueries({
                      queryKey: customAppsKey,
                    });
                  });
                }}
              >
                <label className="grid flex-1 gap-1 text-sm">
                  {t("customApps.appName")}
                  <Input
                    name="name"
                    defaultValue={app.name}
                    required
                    maxLength={80}
                  />
                </label>
                <Button type="submit" disabled={busy}>
                  {t("customApps.renameApp")}
                </Button>
                <Button
                  type="button"
                  variant="neutral"
                  disabled={busy}
                  onClick={() => setDeleting({})}
                >
                  {t("customApps.deleteApp")}
                </Button>
              </form>
              <ConfirmDialog
                isOpen={Boolean(deleting)}
                title={t(
                  deleting?.providerId
                    ? "customApps.deleteProvider"
                    : "customApps.deleteApp",
                )}
                message={t(
                  deleting?.providerId
                    ? "customApps.confirmDeleteProvider"
                    : "customApps.confirmDeleteApp",
                )}
                pending={busy}
                onCancel={() => setDeleting(undefined)}
                onConfirm={() =>
                  void run(async () => {
                    if (deleting?.providerId) {
                      await customAppsApi.deleteProvider(
                        app.id,
                        deleting.providerId,
                      );
                      setEditingId(undefined);
                      await queryClient.invalidateQueries({
                        queryKey: customProvidersKey(app.id),
                      });
                    } else {
                      await customAppsApi.delete(app.id);
                      queryClient.removeQueries({
                        queryKey: customProvidersKey(app.id),
                      });
                      setNewToken(undefined);
                      onSelectApp(null);
                      await queryClient.invalidateQueries({
                        queryKey: customAppsKey,
                      });
                    }
                    setDeleting(undefined);
                  })
                }
              />
              {endpoint ? (
                <label className="grid gap-1 text-sm">
                  {t("customApps.endpoint")}
                  <Input
                    readOnly
                    value={endpoint}
                    onFocus={(event) => event.currentTarget.select()}
                  />
                </label>
              ) : proxyStatus?.running ? (
                <p className="text-sm">{t("customApps.loopbackRequired")}</p>
              ) : (
                <Button
                  className="self-start"
                  disabled={busy || isStarting}
                  onClick={() =>
                    void run(async () => {
                      await startProxyServer();
                    })
                  }
                >
                  {t("customApps.startProxy")}
                </Button>
              )}
              {newToken?.id === app.id && (
                <label className="grid gap-1 text-sm">
                  {t("customApps.token")}
                  <Input
                    readOnly
                    value={newToken.token}
                    onFocus={(event) => event.currentTarget.select()}
                  />
                </label>
              )}
              {newToken?.id !== app.id && (
                <Button
                  className="self-start"
                  variant="neutral"
                  disabled={busy}
                  onClick={() => setConfirmRotation(true)}
                >
                  {t("customApps.replaceToken")}
                </Button>
              )}
              <ConfirmDialog
                isOpen={confirmRotation}
                title={t("customApps.replaceToken")}
                message={t("customApps.confirmReplaceToken")}
                confirmText={t("customApps.replaceToken")}
                pending={busy}
                onCancel={() => setConfirmRotation(false)}
                onConfirm={() => {
                  void run(async () => {
                    const token = await customAppsApi.rotateToken(app.id);
                    setNewToken({ id: app.id, token });
                  }).finally(() => setConfirmRotation(false));
                }}
              />
              <ul className="grid gap-2">
                {providers.data?.providers.map((provider) => (
                  <li
                    key={provider.id}
                    className="flex items-center justify-between gap-3 rounded-md border p-3"
                  >
                    <div className="min-w-0">
                      <p className="break-words font-medium">{provider.name}</p>
                      <p className="break-words text-sm text-fg-3">
                        {provider.model}
                      </p>
                    </div>
                    <div className="flex flex-wrap gap-2">
                      <Button
                        variant="neutral"
                        disabled={busy}
                        onClick={() => setEditingId(provider.id)}
                      >
                        {t("customApps.editProvider")}
                      </Button>
                      <Button
                        variant="neutral"
                        disabled={busy}
                        onClick={() => setDeleting({ providerId: provider.id })}
                      >
                        {t("customApps.deleteProvider")}
                      </Button>
                      <Button
                        variant="neutral"
                        disabled={
                          busy || provider.id === providers.data?.selected
                        }
                        onClick={() =>
                          void run(async () => {
                            await customAppsApi.selectProvider(
                              app.id,
                              provider.id,
                            );
                            await queryClient.invalidateQueries({
                              queryKey: customProvidersKey(app.id),
                            });
                          })
                        }
                      >
                        {t(
                          provider.id === providers.data?.selected
                            ? "customApps.inUse"
                            : "customApps.switch",
                        )}
                      </Button>
                    </div>
                  </li>
                ))}
              </ul>
              <form
                key={`${app.id}-${editingId ?? "new"}`}
                className="grid gap-3 rounded-md border p-4"
                onSubmit={onAddProvider}
              >
                <label className="grid gap-1 text-sm">
                  {t("customApps.providerName")}
                  <Input name="name" defaultValue={editing?.name} required />
                </label>
                <label className="grid gap-1 text-sm">
                  {t("customApps.baseUrl")}
                  <Input
                    name="baseUrl"
                    defaultValue={editing?.baseUrl}
                    type="url"
                    placeholder="http://127.0.0.1:11434/v1"
                    required
                  />
                </label>
                <label className="grid gap-1 text-sm">
                  {t("customApps.model")}
                  <Input name="model" defaultValue={editing?.model} required />
                </label>
                <label className="grid gap-1 text-sm">
                  {t("customApps.apiKey")}
                  <Input name="apiKey" type="password" autoComplete="off" />
                </label>
                {editing && (
                  <label className="text-sm">
                    <input name="clearKey" type="checkbox" />{" "}
                    {t("customApps.clearKey")}
                  </label>
                )}
                {editing && (
                  <p className="text-sm">{t("customApps.keepKey")}</p>
                )}
                {editing && (
                  <Button
                    type="button"
                    variant="neutral"
                    disabled={busy}
                    onClick={() => setEditingId(undefined)}
                  >
                    {t("common.cancel")}
                  </Button>
                )}
                <Button
                  className="justify-self-start"
                  type="submit"
                  disabled={busy}
                >
                  {t(
                    editing
                      ? "customApps.saveProvider"
                      : "customApps.addProvider",
                  )}
                </Button>
              </form>
            </>
          ) : (
            <form
              className="grid gap-3 rounded-md border p-4"
              onSubmit={onCreate}
            >
              <label className="grid gap-1 text-sm">
                {t("customApps.appName")}
                <Input name="name" maxLength={80} required />
              </label>
              <Button
                className="justify-self-start"
                type="submit"
                disabled={busy}
              >
                {t("customApps.addApp")}
              </Button>
            </form>
          )}
        </div>
      </div>
    </>
  );
}
