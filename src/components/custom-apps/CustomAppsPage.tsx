import { useState, type FormEvent } from "react";
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
  onSelectApp: (id: string) => void;
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
      await customAppsApi.addProvider(app.id, {
        name: String(data.get("name")),
        baseUrl: String(data.get("baseUrl")),
        model: String(data.get("model")),
        apiKey: String(data.get("apiKey")),
      });
      await queryClient.invalidateQueries({
        queryKey: customProvidersKey(app.id),
      });
      form.reset();
    });
  };

  const address = proxyStatus?.address;
  const loopback =
    address?.startsWith("127.") || address === "[::1]" || address === "::1";
  const endpoint =
    app && proxyStatus?.running && loopback
      ? `http://${address === "::1" ? "[::1]" : address}:${proxyStatus.port}/apps/${app.id}/v1`
      : undefined;
  const failure = error ?? apps.error?.message ?? providers.error?.message;

  return (
    <>
      <AppPageHeader
        icon={<LayoutGrid className="h-5 w-5" />}
        title={app?.name ?? t("customApps.title")}
      />
      <div className="mx-auto flex w-full max-w-3xl flex-col gap-5 p-5">
        {failure && (
          <p role="alert" className="text-sm text-destructive">
            {failure}
          </p>
        )}
        {app ? (
          <>
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
                  <Button
                    variant="neutral"
                    disabled={busy || provider.id === providers.data?.selected}
                    onClick={() =>
                      void run(async () => {
                        await customAppsApi.selectProvider(app.id, provider.id);
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
                </li>
              ))}
            </ul>
            <form
              key={app.id}
              className="grid gap-3 rounded-md border p-4"
              onSubmit={onAddProvider}
            >
              <label className="grid gap-1 text-sm">
                {t("customApps.providerName")}
                <Input name="name" required />
              </label>
              <label className="grid gap-1 text-sm">
                {t("customApps.baseUrl")}
                <Input
                  name="baseUrl"
                  type="url"
                  placeholder="http://127.0.0.1:11434/v1"
                  required
                />
              </label>
              <label className="grid gap-1 text-sm">
                {t("customApps.model")}
                <Input name="model" required />
              </label>
              <label className="grid gap-1 text-sm">
                {t("customApps.apiKey")}
                <Input name="apiKey" type="password" autoComplete="off" />
              </label>
              <Button
                className="justify-self-start"
                type="submit"
                disabled={busy}
              >
                {t("customApps.addProvider")}
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
    </>
  );
}
