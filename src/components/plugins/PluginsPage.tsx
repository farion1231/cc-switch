import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Download,
  FolderOpen,
  Loader2,
  Puzzle,
  RefreshCw,
  Store,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import {
  usePluginMarketplaces,
  usePluginMutation,
  usePlugins,
  usePluginStatuses,
} from "@/hooks/usePlugins";
import type {
  PluginApp,
  PluginMarketplace,
  PluginScope,
  UnifiedPlugin,
} from "@/lib/api/plugins";
import { settingsApi } from "@/lib/api";

const APPS: PluginApp[] = ["codex", "claude"];

type ConfirmAction =
  | { type: "uninstall"; plugin: UnifiedPlugin }
  | { type: "marketplace"; marketplace: PluginMarketplace };

export default function PluginsPage() {
  const { t } = useTranslation();
  const [tab, setTab] = useState<"installed" | "discover">("installed");
  const [appFilter, setAppFilter] = useState<"all" | PluginApp>("all");
  const [search, setSearch] = useState("");
  const [marketplacesOpen, setMarketplacesOpen] = useState(false);
  const [marketplaceApp, setMarketplaceApp] = useState<PluginApp>("codex");
  const [marketplaceSource, setMarketplaceSource] = useState("");
  const [installTarget, setInstallTarget] = useState<UnifiedPlugin | null>(
    null,
  );
  const [installScope, setInstallScope] = useState<PluginScope>("user");
  const [installProjectPath, setInstallProjectPath] = useState("");
  const [confirmAction, setConfirmAction] = useState<ConfirmAction | null>(
    null,
  );

  const statuses = usePluginStatuses();
  const codexInstalled = usePlugins("codex", false);
  const claudeInstalled = usePlugins("claude", false);
  const codexAvailable = usePlugins("codex", true, tab === "discover");
  const claudeAvailable = usePlugins("claude", true, tab === "discover");
  const codexMarketplaces = usePluginMarketplaces("codex", marketplacesOpen);
  const claudeMarketplaces = usePluginMarketplaces("claude", marketplacesOpen);
  const mutation = usePluginMutation();

  const sourceQueries =
    tab === "discover"
      ? [codexAvailable, claudeAvailable]
      : [codexInstalled, claudeInstalled];
  const loading = sourceQueries.some((query) => query.isLoading);

  const groups = useMemo(() => {
    const grouped = new Map<string, UnifiedPlugin[]>();
    const plugins =
      tab === "discover"
        ? [...(codexAvailable.data ?? []), ...(claudeAvailable.data ?? [])]
        : [...(codexInstalled.data ?? []), ...(claudeInstalled.data ?? [])];
    for (const plugin of plugins) {
      if (appFilter !== "all" && plugin.app !== appFilter) continue;
      if (tab === "installed" && !plugin.installed) continue;
      if (tab === "discover" && plugin.installed) continue;
      const haystack =
        `${plugin.name} ${plugin.pluginId} ${plugin.description ?? ""}`.toLowerCase();
      if (!haystack.includes(search.trim().toLowerCase())) continue;
      grouped.set(plugin.pluginId, [
        ...(grouped.get(plugin.pluginId) ?? []),
        plugin,
      ]);
    }
    return [...grouped.entries()].slice(0, 200);
  }, [
    appFilter,
    claudeAvailable.data,
    claudeInstalled.data,
    codexAvailable.data,
    codexInstalled.data,
    search,
    tab,
  ]);

  const run = async (request: Parameters<typeof mutation.mutateAsync>[0]) => {
    try {
      const result = await mutation.mutateAsync(request);
      toast.success(t("plugins.actionSuccess"), {
        description: result.requiresRestart
          ? t("plugins.restartHint")
          : result.commandSummary,
        closeButton: true,
      });
      return true;
    } catch (error) {
      toast.error(t("plugins.actionFailed"), {
        description: String(error),
        closeButton: true,
      });
      return false;
    }
  };

  const install = async (
    plugin: UnifiedPlugin,
    scope?: PluginScope,
    projectPath?: string,
  ) => {
    const ok = await run({
      action: "install",
      app: plugin.app,
      pluginId: plugin.pluginId,
      scope,
      projectPath,
    });
    if (ok) setInstallTarget(null);
  };

  const marketplaces =
    marketplaceApp === "codex"
      ? (codexMarketplaces.data ?? [])
      : (claudeMarketplaces.data ?? []);

  return (
    <div className="h-full overflow-y-auto px-6 pb-8 pt-4">
      <div className="mx-auto flex max-w-5xl flex-col gap-4">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <Tabs
            value={tab}
            onValueChange={(value) => setTab(value as typeof tab)}
          >
            <TabsList>
              {(["installed", "discover"] as const).map((value) => (
                <TabsTrigger key={value} value={value}>
                  {t(`plugins.tabs.${value}`)}
                </TabsTrigger>
              ))}
            </TabsList>
          </Tabs>
          <Button variant="outline" onClick={() => setMarketplacesOpen(true)}>
            <Store data-icon="inline-start" className="size-4" />
            {t("plugins.marketplaces.title")}
          </Button>
        </div>

        <div className="grid gap-2 sm:grid-cols-2">
          {APPS.map((app) => {
            const status = statuses.data?.find((item) => item.app === app);
            return (
              <div
                key={app}
                className="flex items-center justify-between rounded-lg border border-border-default bg-card px-4 py-3"
              >
                <div>
                  <div className="font-medium">{t(`plugins.apps.${app}`)}</div>
                  <div className="text-xs text-muted-foreground">
                    {status?.version ?? status?.error ?? t("plugins.detecting")}
                  </div>
                </div>
                <Badge variant={status?.available ? "secondary" : "outline"}>
                  {status?.available
                    ? t("plugins.available")
                    : t("plugins.unavailable")}
                </Badge>
              </div>
            );
          })}
        </div>

        <div className="flex flex-wrap gap-2">
          <Input
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            placeholder={t("plugins.search")}
            className="min-w-56 flex-1"
          />
          <div className="flex items-center gap-1 rounded-lg bg-muted p-1">
            {(["all", ...APPS] as const).map((app) => (
              <Button
                key={app}
                size="sm"
                variant={appFilter === app ? "secondary" : "ghost"}
                onClick={() => setAppFilter(app)}
              >
                {app === "all" ? t("common.all") : t(`plugins.apps.${app}`)}
              </Button>
            ))}
          </div>
        </div>

        {sourceQueries.map((query, index) =>
          query.error ? (
            <Alert key={APPS[index]} variant="destructive">
              <AlertDescription>
                {t(`plugins.apps.${APPS[index]}`)}: {String(query.error)}
              </AlertDescription>
            </Alert>
          ) : null,
        )}

        {loading ? (
          <div className="flex justify-center py-16">
            <Loader2 className="size-6 animate-spin text-muted-foreground" />
          </div>
        ) : groups.length === 0 ? (
          <div className="rounded-xl border border-dashed border-border-default py-16 text-center text-muted-foreground">
            <Puzzle className="mx-auto mb-3 size-8 opacity-50" />
            {t("plugins.empty")}
          </div>
        ) : (
          <div className="overflow-hidden rounded-xl border border-border-default bg-card">
            {groups.map(([pluginId, entries], index) => (
              <div
                key={pluginId}
                className={
                  index + 1 < groups.length
                    ? "border-b border-border-default"
                    : ""
                }
              >
                <div className="px-4 pb-2 pt-3">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="font-medium">{entries[0].name}</span>
                    <span className="text-xs text-muted-foreground">
                      {pluginId}
                    </span>
                  </div>
                  {entries[0].description && (
                    <p className="mt-1 line-clamp-2 text-sm text-muted-foreground">
                      {entries[0].description}
                    </p>
                  )}
                </div>
                {entries.map((plugin) => (
                  <div
                    key={`${plugin.pluginId}-${plugin.app}`}
                    className="flex flex-wrap items-center gap-3 bg-muted/20 px-4 py-2"
                  >
                    <Badge variant="outline">
                      {t(`plugins.apps.${plugin.app}`)}
                    </Badge>
                    <span className="text-xs text-muted-foreground">
                      {plugin.version ?? t("plugins.unknownVersion")} ·{" "}
                      {plugin.marketplaceName}
                      {plugin.scope ? ` · ${plugin.scope}` : ""}
                    </span>
                    <div className="ml-auto flex items-center gap-2">
                      {plugin.installed && (
                        <Switch
                          checked={plugin.enabled}
                          disabled={mutation.isPending}
                          aria-label={t("plugins.enabled")}
                          onCheckedChange={(enabled) =>
                            void run({
                              action: "setEnabled",
                              app: plugin.app,
                              pluginId: plugin.pluginId,
                              enabled,
                              scope: plugin.scope,
                              projectPath: plugin.projectPath,
                            })
                          }
                        />
                      )}
                      {plugin.supportedActions.update && (
                        <Button
                          size="sm"
                          variant="ghost"
                          disabled={mutation.isPending}
                          onClick={() =>
                            void run({
                              action: "update",
                              app: plugin.app,
                              pluginId: plugin.pluginId,
                              scope: plugin.scope,
                              projectPath: plugin.projectPath,
                            })
                          }
                        >
                          <RefreshCw
                            data-icon="inline-start"
                            className="size-4"
                          />
                          {t("plugins.update")}
                        </Button>
                      )}
                      {plugin.supportedActions.install && (
                        <Button
                          size="sm"
                          disabled={mutation.isPending}
                          onClick={() => {
                            if (plugin.app === "claude") {
                              setInstallScope("user");
                              setInstallProjectPath("");
                              setInstallTarget(plugin);
                            } else {
                              void install(plugin);
                            }
                          }}
                        >
                          <Download
                            data-icon="inline-start"
                            className="size-4"
                          />
                          {t("plugins.install")}
                        </Button>
                      )}
                      {plugin.supportedActions.uninstall && (
                        <Button
                          size="icon"
                          variant="ghost"
                          disabled={mutation.isPending}
                          title={t("plugins.uninstall")}
                          onClick={() =>
                            setConfirmAction({ type: "uninstall", plugin })
                          }
                        >
                          <Trash2 className="size-4 text-destructive" />
                        </Button>
                      )}
                    </div>
                  </div>
                ))}
              </div>
            ))}
          </div>
        )}

        {groups.length === 200 && (
          <p className="text-center text-xs text-muted-foreground">
            {t("plugins.resultLimit")}
          </p>
        )}
      </div>

      <Dialog open={marketplacesOpen} onOpenChange={setMarketplacesOpen}>
        <DialogContent className="max-w-2xl">
          <DialogHeader>
            <DialogTitle>{t("plugins.marketplaces.title")}</DialogTitle>
            <DialogDescription>
              {t("plugins.marketplaces.description")}
            </DialogDescription>
          </DialogHeader>
          <div className="flex flex-col gap-4 overflow-y-auto px-6 py-4">
            <div className="flex gap-2">
              {APPS.map((app) => (
                <Button
                  key={app}
                  size="sm"
                  variant={marketplaceApp === app ? "secondary" : "outline"}
                  onClick={() => setMarketplaceApp(app)}
                >
                  {t(`plugins.apps.${app}`)}
                </Button>
              ))}
            </div>
            <div className="flex gap-2">
              <Input
                value={marketplaceSource}
                onChange={(event) => setMarketplaceSource(event.target.value)}
                placeholder={t("plugins.marketplaces.sourcePlaceholder")}
              />
              <Button
                disabled={!marketplaceSource.trim() || mutation.isPending}
                onClick={async () => {
                  const ok = await run({
                    action: "addMarketplace",
                    app: marketplaceApp,
                    source: marketplaceSource.trim(),
                  });
                  if (ok) setMarketplaceSource("");
                }}
              >
                {t("plugins.marketplaces.add")}
              </Button>
            </div>
            <div className="divide-y divide-border-default rounded-lg border border-border-default">
              {marketplaces.map((marketplace) => (
                <div
                  key={marketplace.name}
                  className="flex items-center gap-3 px-3 py-2"
                >
                  <div className="min-w-0 flex-1">
                    <div className="font-medium">{marketplace.name}</div>
                    <div className="truncate text-xs text-muted-foreground">
                      {marketplace.source ??
                        marketplace.root ??
                        marketplace.sourceType}
                    </div>
                  </div>
                  <Button
                    size="icon"
                    variant="ghost"
                    title={t("plugins.marketplaces.refresh")}
                    onClick={() =>
                      void run({
                        action: "refreshMarketplace",
                        app: marketplace.app,
                        name: marketplace.name,
                      })
                    }
                  >
                    <RefreshCw className="size-4" />
                  </Button>
                  <Button
                    size="icon"
                    variant="ghost"
                    title={t("common.delete")}
                    onClick={() =>
                      setConfirmAction({ type: "marketplace", marketplace })
                    }
                  >
                    <Trash2 className="size-4 text-destructive" />
                  </Button>
                </div>
              ))}
            </div>
          </div>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setMarketplacesOpen(false)}
            >
              {t("common.close")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={Boolean(installTarget)}
        onOpenChange={(open) => !open && setInstallTarget(null)}
      >
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle>{t("plugins.scope.title")}</DialogTitle>
            <DialogDescription>{installTarget?.pluginId}</DialogDescription>
          </DialogHeader>
          <div className="px-6 py-4">
            <Select
              value={installScope}
              onValueChange={(value) => setInstallScope(value as PluginScope)}
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  {(["user", "project", "local"] as const).map((scope) => (
                    <SelectItem key={scope} value={scope}>
                      {t(`plugins.scope.${scope}`)}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
            {installScope !== "user" && (
              <div className="mt-3 flex gap-2">
                <Input
                  readOnly
                  value={installProjectPath}
                  placeholder={t("plugins.scope.projectPath")}
                />
                <Button
                  variant="outline"
                  onClick={async () => {
                    const path = await settingsApi.pickDirectory(
                      installProjectPath || undefined,
                    );
                    if (path) setInstallProjectPath(path);
                  }}
                >
                  <FolderOpen data-icon="inline-start" className="size-4" />
                  {t("plugins.scope.browse")}
                </Button>
              </div>
            )}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setInstallTarget(null)}>
              {t("common.cancel")}
            </Button>
            <Button
              disabled={mutation.isPending}
              onClick={() => {
                if (!installTarget) return;
                if (installScope !== "user" && !installProjectPath) {
                  toast.error(t("plugins.scope.projectPathRequired"));
                  return;
                }
                void install(
                  installTarget,
                  installScope,
                  installScope === "user" ? undefined : installProjectPath,
                );
              }}
            >
              {t("plugins.install")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {confirmAction && (
        <ConfirmDialog
          isOpen={true}
          title={
            confirmAction.type === "uninstall"
              ? t("plugins.uninstall")
              : t("plugins.marketplaces.remove")
          }
          message={
            confirmAction.type === "uninstall"
              ? t("plugins.uninstallConfirm", {
                  name: confirmAction.plugin.name,
                  app: t(`plugins.apps.${confirmAction.plugin.app}`),
                })
              : t("plugins.marketplaces.removeConfirm", {
                  name: confirmAction.marketplace.name,
                })
          }
          onCancel={() => setConfirmAction(null)}
          onConfirm={() => {
            const request =
              confirmAction.type === "uninstall"
                ? {
                    action: "uninstall" as const,
                    app: confirmAction.plugin.app,
                    pluginId: confirmAction.plugin.pluginId,
                    scope: confirmAction.plugin.scope,
                    projectPath: confirmAction.plugin.projectPath,
                  }
                : {
                    action: "removeMarketplace" as const,
                    app: confirmAction.marketplace.app,
                    name: confirmAction.marketplace.name,
                  };
            setConfirmAction(null);
            void run(request);
          }}
        />
      )}
    </div>
  );
}
