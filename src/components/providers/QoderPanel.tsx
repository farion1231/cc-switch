import { useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2, Plus, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  qoderApi,
  qoderKeys,
  type QoderAppId,
  type QoderModels,
  type QoderProvider,
} from "@/lib/api/qoder";
import { toast } from "@/lib/toast";
import { extractErrorMessage } from "@/utils/errorUtils";
import { QoderProviderDialog } from "./QoderProviderDialog";
import { ProviderEmptyState } from "./ProviderEmptyState";
import { ProviderCard } from "./ProviderCard";
import type { CardPresentation } from "./presentation";
import { settingsApi } from "@/lib/api";
import UsageScriptModal from "@/components/UsageScriptModal";
import type { Provider } from "@/types";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { AuthSettingsPanel } from "./AuthSettingsPanel";
import type { AuthCenterTarget } from "@/lib/api";

export function QoderPanel({
  app,
  actionSlot,
}: {
  app: QoderAppId;
  actionSlot?: HTMLElement | null;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: qoderKeys.models(app),
    queryFn: () => qoderApi.getModels(app),
  });
  const auth = useQuery({
    queryKey: qoderKeys.auth(app),
    queryFn: () => qoderApi.getAuthStatus(app),
    retry: false,
    staleTime: 30_000,
  });
  const [editing, setEditing] = useState<{
    id?: string;
    provider?: QoderProvider;
  } | null>(null);
  const [authTarget, setAuthTarget] = useState<AuthCenterTarget | null>(null);
  const [deleting, setDeleting] = useState<{
    id: string;
    provider: QoderProvider;
  } | null>(null);
  const mutation = useMutation({
    mutationFn: (operation: () => Promise<QoderModels>) => operation(),
    onSuccess: (models) => {
      queryClient.setQueryData(qoderKeys.models(app), models);
      toast.success(t("qoder.saved"));
    },
  });
  const [usageProvider, setUsageProvider] = useState<Provider | null>(null);
  const run = (operation: () => Promise<QoderModels>) =>
    mutation.mutateAsync(operation);
  const runAction = async (operation: () => Promise<QoderModels>) => {
    try {
      await run(operation);
    } catch (error) {
      toast.error(extractErrorMessage(error));
    }
  };
  const providers = query.data?.providers ?? {};
  const current = query.data?.currentModel ?? "auto";
  const models = Object.entries(providers).flatMap(([id, provider]) => {
    const declared = provider.models?.length
      ? [...provider.models]
      : provider.model
        ? [{ model: provider.model }]
        : [];
    if (
      provider.model &&
      !declared.some((model) => model.model === provider.model)
    )
      declared.push({ model: provider.model });
    return declared.map((model) => ({
      key: `${id}/${model.model}`,
      provider: id,
      name: model.displayName || model.model,
    }));
  });
  const busy = mutation.isPending;
  const importCurrent = async () => {
    const [result, account] = await Promise.all([
      query.refetch(),
      auth.refetch(),
    ]);
    if (result.error) {
      toast.error(extractErrorMessage(result.error));
      return;
    }
    toast.info(
      t(
        Object.keys(result.data?.providers ?? {}).length ||
          account.data?.loggedIn
          ? "qoder.imported"
          : "qoder.nothingToImport",
      ),
    );
  };
  const actions = (
    <>
      <Button
        variant="quiet"
        size="icon-compact"
        disabled={query.isFetching || auth.isFetching || busy}
        aria-label={t("common.refresh")}
        onClick={() => void Promise.all([query.refetch(), auth.refetch()])}
      >
        <RefreshCw
          className={
            query.isFetching || auth.isFetching
              ? "h-4 w-4 animate-spin"
              : "h-4 w-4"
          }
        />
      </Button>
      <Button
        variant="solid"
        size="regular"
        disabled={!query.data || busy}
        onClick={() => setEditing({})}
      >
        <Plus className="h-4 w-4" />
        {t("provider.addProvider")}
      </Button>
    </>
  );
  return (
    <div
      id="main-content"
      className="min-h-0 flex-1 overflow-y-auto scroll-stable px-6 pb-12 pt-4"
    >
      {actionSlot ? (
        createPortal(actions, actionSlot)
      ) : actionSlot === undefined ? (
        <div className="mb-4 flex justify-end gap-2">{actions}</div>
      ) : null}
      <div className="space-y-4">
        {query.isLoading && (
          <Loader2
            role="status"
            aria-label={t("common.loading")}
            className="h-5 w-5 animate-spin text-fg-2"
          />
        )}
        {query.isError && (
          <p role="alert" className="text-body text-danger-text">
            {extractErrorMessage(query.error)}
          </p>
        )}
        {query.data && (
          <>
            {Object.keys(providers).length === 0 && !auth.data?.loggedIn && (
              <ProviderEmptyState
                appId={app}
                onCreate={() => setEditing({})}
                onImport={() => void importCurrent()}
              />
            )}
            {auth.data?.loggedIn &&
              (() => {
                const active = !Object.keys(providers).some((id) =>
                  current.startsWith(`${id}/`),
                );
                const websiteUrl =
                  app === "qoder-cn"
                    ? "https://qoder.com.cn"
                    : "https://qoder.com";
                return (
                  <ProviderCard
                    provider={{
                      id: "_qoder_official",
                      name: APP_DISPLAY_NAME[app],
                      category: "official",
                      icon: "qoder",
                      settingsConfig: {},
                      notes: [
                        auth.data.email || auth.data.username,
                        auth.data.plan,
                      ]
                        .filter(Boolean)
                        .join(" · "),
                      websiteUrl,
                    }}
                    appId={app}
                    isCurrent={active}
                    presentation={{
                      tone: active ? "direct" : undefined,
                      status: active
                        ? {
                            label: t("providerCard.status.inUse"),
                            dot: "direct",
                          }
                        : undefined,
                      chips: [
                        {
                          key: "official",
                          label: t("providerCard.chip.official"),
                          tone: "outline",
                        },
                      ],
                      buttons: active
                        ? []
                        : [
                            {
                              key: "switch",
                              label: t("providerCard.action.switch"),
                              onClick: () =>
                                void runAction(() =>
                                  qoderApi.setModel(app, "auto"),
                                ),
                              disabledReason: busy
                                ? t("common.loading")
                                : undefined,
                            },
                          ],
                      editDisabledReason: busy
                        ? t("common.loading")
                        : undefined,
                      deleteDisabledReason: t("qoder.accountManagedByQoder"),
                    }}
                    onEdit={() =>
                      setAuthTarget(
                        app === "qoder-cn" ? "qoder_cn_oauth" : "qoder_oauth",
                      )
                    }
                    onDelete={() => {}}
                    onOpenWebsite={(url) => void settingsApi.openExternal(url)}
                  />
                );
              })()}
            {Object.entries(providers).map(([id, provider]) => {
              const providerModels = models.filter(
                (model) => model.provider === id,
              );
              const active = current.startsWith(`${id}/`);
              const defaultModel = provider.model
                ? `${id}/${provider.model}`
                : providerModels[0]?.key;
              const buttons: CardPresentation["buttons"] = [];
              const menuItems: CardPresentation["menuItems"] = [];
              if (!active) {
                buttons.push({
                  key: "switch",
                  label: t("providerCard.action.switch"),
                  onClick: () => {
                    if (defaultModel)
                      void runAction(() =>
                        qoderApi.setModel(app, defaultModel),
                      );
                  },
                  disabledReason: busy
                    ? t("common.loading")
                    : !defaultModel
                      ? t("providerCard.chip.noModels")
                      : undefined,
                });
              }
              if (providerModels.length) {
                menuItems.push({
                  key: "setDefault",
                  label: t("qoder.selectModelAction"),
                  onSelect: () => {},
                  disabledReason: busy ? t("common.loading") : undefined,
                  submenu: {
                    title: t("qoder.selectModelAction"),
                    options: providerModels.map((model) => ({
                      key: model.key,
                      label: model.name,
                      detail: model.key,
                      onSelect: () =>
                        void runAction(() => qoderApi.setModel(app, model.key)),
                    })),
                  },
                });
              }
              if (active) {
                menuItems.push({
                  key: "autoModel",
                  label: t("qoder.autoModel"),
                  disabledReason: busy ? t("common.loading") : undefined,
                  onSelect: () =>
                    void runAction(() => qoderApi.setModel(app, "auto")),
                });
              }
              const presentation: CardPresentation = {
                tone: active ? "direct" : undefined,
                status: active
                  ? { label: t("providerCard.status.inUse"), dot: "direct" }
                  : undefined,
                chips: [
                  {
                    key: "protocol",
                    label: provider.protocol || "openai",
                    tone: "outline",
                  },
                ],
                buttons,
                editDisabledReason: busy ? t("common.loading") : undefined,
                deleteDisabledReason: busy ? t("common.loading") : undefined,
                menuItems,
              };
              return (
                <ProviderCard
                  key={id}
                  provider={{
                    id,
                    name: provider.displayName || id,
                    settingsConfig: provider,
                    icon: "qoder",
                    category: "custom",
                    meta: query.data?.metadata?.[id],
                  }}
                  appId={app}
                  isCurrent={active}
                  onConfigureUsage={setUsageProvider}
                  presentation={presentation}
                  onEdit={() => {
                    if (!busy) setEditing({ id, provider });
                  }}
                  onDelete={() => {
                    if (!busy) setDeleting({ id, provider });
                  }}
                  onOpenWebsite={(url) => void settingsApi.openExternal(url)}
                />
              );
            })}
          </>
        )}
      </div>
      {usageProvider && (
        <UsageScriptModal
          provider={usageProvider}
          appId={app}
          isOpen
          onClose={() => setUsageProvider(null)}
          onSave={(script) =>
            void runAction(() =>
              qoderApi.saveUsageScript(app, usageProvider.id, script),
            )
          }
        />
      )}
      {editing && (
        <QoderProviderDialog
          app={app}
          id={editing.id}
          provider={editing.provider}
          providers={providers}
          currentModel={current}
          onClose={() => setEditing(null)}
          onSave={async (id, provider, makeDefault) => {
            await run(() =>
              qoderApi.saveProvider(
                app,
                id,
                provider,
                editing.provider ?? null,
                makeDefault,
              ),
            );
          }}
        />
      )}
      <AuthSettingsPanel
        target={authTarget}
        onClose={() => setAuthTarget(null)}
      />
      <Dialog
        open={Boolean(deleting)}
        onOpenChange={(open) => {
          if (!open && !busy) setDeleting(null);
        }}
      >
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>{t("qoder.deleteProviderTitle")}</DialogTitle>
            <DialogDescription>
              {t("qoder.deleteProviderDescription", {
                name: deleting?.provider.displayName || deleting?.id,
              })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="neutral"
              size="regular"
              disabled={busy}
              onClick={() => setDeleting(null)}
            >
              {t("common.cancel")}
            </Button>
            <Button
              variant="destructive"
              size="regular"
              disabled={busy}
              onClick={async () => {
                if (!deleting) return;
                try {
                  await run(() =>
                    qoderApi.deleteProvider(
                      app,
                      deleting.id,
                      deleting.provider,
                    ),
                  );
                  setDeleting(null);
                } catch (error) {
                  toast.error(extractErrorMessage(error));
                }
              }}
            >
              {t("common.delete")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
