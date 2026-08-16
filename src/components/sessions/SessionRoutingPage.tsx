import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Activity,
  Route,
  RefreshCw,
  Trash2,
  Loader2,
  AlertTriangle,
  Settings2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { ScrollArea } from "@/components/ui/scroll-area";
import { ToggleRow } from "@/components/ui/toggle-row";
import { ProviderIcon } from "@/components/ProviderIcon";
import {
  useSessionRoutingConfig,
  useUpdateSessionRoutingConfig,
  useActiveSessionRoutes,
  useDeleteSessionRoute,
  useSessionProviderLoad,
  useCleanupExpiredRoutes,
} from "@/lib/query/sessionRoutes";
import { extractErrorMessage } from "@/utils/errorUtils";
import { toast } from "sonner";

const APP_TYPES = ["claude", "codex", "gemini"] as const;
type AppType = (typeof APP_TYPES)[number];

export function SessionRoutingPage() {
  const { t } = useTranslation();
  const [selectedApp, setSelectedApp] = useState<AppType>("claude");

  const { data: config, isLoading: configLoading } =
    useSessionRoutingConfig(selectedApp);
  const updateConfig = useUpdateSessionRoutingConfig();
  const { data: routes = [], isLoading: routesLoading } =
    useActiveSessionRoutes(selectedApp);
  const { data: providerLoad = {} } = useSessionProviderLoad(selectedApp);
  const deleteRoute = useDeleteSessionRoute();
  const cleanupExpired = useCleanupExpiredRoutes();

  const handleToggle = async (enabled: boolean) => {
    if (!config) return;
    updateConfig.mutate({
      appType: selectedApp,
      config: { ...config, enabled },
    });
  };

  const handleStrategyChange = async (strategy: "round_robin" | "least_loaded") => {
    if (!config) return;
    updateConfig.mutate({
      appType: selectedApp,
      config: { ...config, strategy },
    });
  };

  const handleTtlChange = async (ttlSeconds: number) => {
    if (!config || ttlSeconds < 60) return;
    updateConfig.mutate({
      appType: selectedApp,
      config: { ...config, sessionTtlSeconds: ttlSeconds },
    });
  };

  const handleDeleteRoute = (sessionId: string) => {
    deleteRoute.mutate({ sessionId, appType: selectedApp });
  };

  const handleCleanup = () => {
    if (!config) return;
    cleanupExpired.mutate({
      appType: selectedApp,
      ttlSeconds: config.sessionTtlSeconds,
    });
  };

  const formatTime = (ms: number) => {
    const diff = Date.now() - ms;
    const mins = Math.floor(diff / 60000);
    if (mins < 1) return t("sessionRoutes.justNow");
    if (mins < 60) return t("sessionRoutes.minutesAgo", { count: mins });
    const hours = Math.floor(mins / 60);
    return t("sessionRoutes.hoursAgo", { count: hours });
  };

  return (
    <div className="space-y-6 p-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Route className="w-5 h-5" />
          <h2 className="text-lg font-semibold">
            {t("sessionRoutes.title")}
          </h2>
        </div>
        <Select
          value={selectedApp}
          onValueChange={(v) => setSelectedApp(v as AppType)}
        >
          <SelectTrigger className="w-32">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {APP_TYPES.map((app) => (
              <SelectItem key={app} value={app}>
                {t(`app.${app}`)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {/* Config Card */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <Settings2 className="w-4 h-4" />
            {t("sessionRoutes.config")}
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          {configLoading ? (
            <div className="flex justify-center py-4">
              <Loader2 className="w-5 h-5 animate-spin" />
            </div>
          ) : config ? (
            <>
              <ToggleRow
                label={t("sessionRoutes.enable")}
                description={t("sessionRoutes.enableDesc")}
              >
                <Switch
                  checked={config.enabled}
                  onCheckedChange={handleToggle}
                />
              </ToggleRow>

              <div className="flex items-center justify-between">
                <div>
                  <Label>{t("sessionRoutes.strategy")}</Label>
                  <p className="text-sm text-muted-foreground">
                    {t("sessionRoutes.strategyDesc")}
                  </p>
                </div>
                <Select
                  value={config.strategy}
                  onValueChange={handleStrategyChange}
                >
                  <SelectTrigger className="w-40">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="round_robin">
                      {t("sessionRoutes.roundRobin")}
                    </SelectItem>
                    <SelectItem value="least_loaded">
                      {t("sessionRoutes.leastLoaded")}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>

              <div className="flex items-center justify-between">
                <div>
                  <Label>{t("sessionRoutes.ttl")}</Label>
                  <p className="text-sm text-muted-foreground">
                    {t("sessionRoutes.ttlDesc")}
                  </p>
                </div>
                <div className="flex items-center gap-2">
                  <Input
                    type="number"
                    className="w-20"
                    min={60}
                    step={60}
                    value={config.sessionTtlSeconds}
                    onChange={(e) => {
                      const val = parseInt(e.target.value, 10);
                      if (!isNaN(val)) handleTtlChange(val);
                    }}
                  />
                  <span className="text-sm text-muted-foreground">
                    {t("sessionRoutes.seconds")}
                  </span>
                </div>
              </div>
            </>
          ) : (
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <AlertTriangle className="w-4 h-4" />
              {t("sessionRoutes.noConfig")}
            </div>
          )}
        </CardContent>
      </Card>

      {/* Provider Load Card */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <Activity className="w-4 h-4" />
            {t("sessionRoutes.providerLoad")}
          </CardTitle>
        </CardHeader>
        <CardContent>
          {Object.keys(providerLoad).length === 0 ? (
            <p className="text-sm text-muted-foreground">
              {t("sessionRoutes.noLoadData")}
            </p>
          ) : (
            <div className="space-y-2">
              {Object.entries(providerLoad).map(([providerId, count]) => (
                <div
                  key={providerId}
                  className="flex items-center justify-between"
                >
                  <div className="flex items-center gap-2">
                    <ProviderIcon providerId={providerId} />
                    <span className="text-sm">{providerId}</span>
                  </div>
                  <div className="flex items-center gap-2">
                    <div className="w-24 h-2 bg-secondary rounded-full overflow-hidden">
                      <div
                        className="h-full bg-primary rounded-full transition-all"
                        style={{
                          width: `${Math.min((count / Math.max(Object.values(providerLoad).length, 1)) * 100, 100)}%`,
                        }}
                      />
                    </div>
                    <span className="text-xs text-muted-foreground w-4 text-right">
                      {count}
                    </span>
                  </div>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      {/* Active Sessions Card */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center justify-between">
            <div className="flex items-center gap-2 text-base">
              <Activity className="w-4 h-4" />
              {t("sessionRoutes.activeSessions")}
              <Badge variant="secondary">{routes.length}</Badge>
            </div>
            <div className="flex items-center gap-2">
              <Button
                variant="outline"
                size="sm"
                onClick={handleCleanup}
                disabled={cleanupExpired.isPending}
              >
                {cleanupExpired.isPending ? (
                  <Loader2 className="w-3 h-3 animate-spin mr-1" />
                ) : (
                  <Trash2 className="w-3 h-3 mr-1" />
                )}
                {t("sessionRoutes.cleanup")}
              </Button>
            </div>
          </CardTitle>
        </CardHeader>
        <CardContent>
          {routesLoading ? (
            <div className="flex justify-center py-8">
              <Loader2 className="w-5 h-5 animate-spin" />
            </div>
          ) : routes.length === 0 ? (
            <div className="flex flex-col items-center py-8 text-muted-foreground">
              <Route className="w-8 h-8 mb-2 opacity-50" />
              <p className="text-sm">{t("sessionRoutes.noSessions")}</p>
            </div>
          ) : (
            <ScrollArea className="max-h-96">
              <div className="space-y-1">
                {routes.map((route) => (
                  <div
                    key={route.sessionId}
                    className="flex items-center justify-between p-2 rounded-lg hover:bg-muted/50 transition-colors"
                  >
                    <div className="flex items-center gap-3 min-w-0">
                      <ProviderIcon providerId={route.providerId} />
                      <div className="min-w-0">
                        <div className="flex items-center gap-2">
                          <span className="text-sm font-medium truncate max-w-[120px]">
                            {route.sessionId.slice(0, 8)}
                          </span>
                          <span className="text-xs text-muted-foreground">
                            {route.providerName}
                          </span>
                        </div>
                        <div className="flex items-center gap-3 text-xs text-muted-foreground">
                          <span>
                            {route.requestCount}{" "}
                            {t("sessionRoutes.requests")}
                          </span>
                          {route.failoverCount > 0 && (
                            <span className="text-amber-500">
                              {route.failoverCount}{" "}
                              {t("sessionRoutes.failovers")}
                            </span>
                          )}
                          <span>{formatTime(route.lastUsedAt)}</span>
                        </div>
                      </div>
                    </div>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 shrink-0"
                      onClick={() => handleDeleteRoute(route.sessionId)}
                    >
                      <Trash2 className="w-3 h-3" />
                    </Button>
                  </div>
                ))}
              </div>
            </ScrollArea>
          )}
        </CardContent>
      </Card>
    </div>
  );
}