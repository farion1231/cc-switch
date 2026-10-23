import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { badgeVariants } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { hermesProfileApi } from "@/lib/api/hermesProfile";
import { useProvidersQuery } from "@/lib/query";
import { toast } from "@/lib/toast";

/**
 * Hermes 按 agent（profile）独立配置：每个 agent 可以单独进入代理模式——
 * `/hermes/<profile>/v1` 独立命名空间，自己的路由目标、故障转移与负载均衡。
 * 独立进入的 agent 不再跟随统一路由的切换。
 */
export function HermesProfilePanel() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: profiles } = useQuery({
    queryKey: ["hermes-profiles"],
    queryFn: hermesProfileApi.list,
  });
  const { data: providersData } = useProvidersQuery("hermes");
  const providers = Object.values(providersData?.providers ?? {});
  // 进入前的选择（不落库，进入时带走）
  const [pendingRoute, setPendingRoute] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<string | null>(null);

  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: ["hermes-profiles"] });

  const run = async (key: string, action: () => Promise<void>) => {
    setBusy(key);
    try {
      await action();
      await invalidate();
    } catch (error) {
      toast.error(String(error));
    } finally {
      setBusy(null);
    }
  };

  if (!profiles?.length) {
    return null;
  }

  return (
    <div className="space-y-2">
      <p className="text-muted-foreground text-xs">
        {t("hermes.profilePanel.hint")}
      </p>
      <div className="divide-y rounded-md border">
        {profiles.map((profile) => {
          const routeOptions = providers.filter(
            (provider) => provider.category !== "official",
          );
          const selectedRoute =
            profile.routeProviderId ??
            pendingRoute[profile.name] ??
            routeOptions[0]?.id ??
            "";
          return (
            <div
              key={profile.name}
              className="flex flex-wrap items-center gap-2 px-3 py-2"
            >
              <span className="min-w-32 font-mono text-sm">{profile.name}</span>
              <span
                className={
                  profile.independent
                    ? `${badgeVariants({ variant: "default" })} text-xs`
                    : `${badgeVariants({ variant: "secondary" })} text-xs`
                }
              >
                {profile.independent
                  ? t("hermes.profilePanel.independent")
                  : t("hermes.profilePanel.direct")}
              </span>
              <Select
                value={selectedRoute}
                onValueChange={(value) => {
                  if (profile.independent) {
                    void run(profile.name, async () => {
                      await hermesProfileApi.switchRoute(profile.name, value);
                    });
                  } else {
                    setPendingRoute((prev) => ({
                      ...prev,
                      [profile.name]: value,
                    }));
                  }
                }}
              >
                <SelectTrigger className="h-8 w-40">
                  <SelectValue
                    placeholder={t("hermes.profilePanel.routePlaceholder")}
                  />
                </SelectTrigger>
                <SelectContent>
                  {routeOptions.map((provider) => (
                    <SelectItem key={provider.id} value={provider.id}>
                      {provider.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {profile.independent && (
                <>
                  <label className="flex items-center gap-1 text-xs">
                    {t("hermes.profilePanel.failover")}
                    <Switch
                      checked={profile.autoFailoverEnabled}
                      onCheckedChange={(checked) =>
                        void run(profile.name, async () => {
                          // 重进同一路由，只改开关
                          await hermesProfileApi.enter({
                            profile: profile.name,
                            routeProviderId: selectedRoute,
                            autoFailover: checked,
                            loadBalanceMode:
                              profile.loadBalanceMode === "session"
                                ? "session"
                                : "off",
                          });
                        })
                      }
                    />
                  </label>
                  <label className="flex items-center gap-1 text-xs">
                    {t("hermes.profilePanel.sticky")}
                    <Switch
                      checked={profile.loadBalanceMode === "session"}
                      onCheckedChange={(checked) =>
                        void run(profile.name, async () => {
                          await hermesProfileApi.enter({
                            profile: profile.name,
                            routeProviderId: selectedRoute,
                            autoFailover: profile.autoFailoverEnabled,
                            loadBalanceMode: checked ? "session" : "off",
                          });
                        })
                      }
                    />
                  </label>
                </>
              )}
              {profile.independent ? (
                <Button
                  size="sm"
                  variant="outline"
                  disabled={busy === profile.name}
                  onClick={() =>
                    void run(profile.name, () =>
                      hermesProfileApi.exit(profile.name),
                    )
                  }
                >
                  {t("hermes.profilePanel.exit")}
                </Button>
              ) : (
                <Button
                  size="sm"
                  disabled={!selectedRoute || busy === profile.name}
                  onClick={() =>
                    void run(profile.name, () =>
                      hermesProfileApi.enter({
                        profile: profile.name,
                        routeProviderId: selectedRoute,
                      }),
                    )
                  }
                >
                  {t("hermes.profilePanel.enter")}
                </Button>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
