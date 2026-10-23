import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Bot, ChevronDown, Search } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
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
import { cn } from "@/lib/utils";

/**
 * Hermes 按 agent（profile）独立配置：每个 agent 可以单独进入代理模式——
 * `/hermes/<profile>/v1` 独立命名空间，自己的路由目标、故障转移与负载均衡。
 * 独立进入的 agent 不再跟随统一路由的切换。默认折叠，展开后可搜索、滚动。
 */
export function HermesProfilePanel() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState("");
  const { data: profiles } = useQuery({
    queryKey: ["hermes-profiles"],
    queryFn: hermesProfileApi.list,
  });
  const { data: providersData } = useProvidersQuery("hermes");
  const providers = Object.values(providersData?.providers ?? {});
  // 进入前的路由选择（不落库，进入时带走）
  const [pendingRoute, setPendingRoute] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<string | null>(null);

  const independentCount =
    profiles?.filter((profile) => profile.independent).length ?? 0;
  const filtered = useMemo(() => {
    const keyword = search.trim().toLowerCase();
    if (!profiles) return [];
    return keyword
      ? profiles.filter((profile) =>
          profile.name.toLowerCase().includes(keyword),
        )
      : profiles;
  }, [profiles, search]);

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
    <Collapsible open={open} onOpenChange={setOpen}>
      <CollapsibleTrigger className="group flex w-full items-center gap-3 rounded-xl border border-border bg-surface p-4 text-left transition-colors hover:bg-subtle">
        <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-surface ring-1 ring-border">
          <Bot className="h-4 w-4 text-fg-2" />
        </div>
        <div className="min-w-0 flex-1 space-y-1">
          <p className="text-sm font-medium leading-none">
            {t("hermes.profilePanel.title")}
          </p>
          <p className="text-xs text-fg-2">
            {t("hermes.profilePanel.summary", {
              total: profiles.length,
              independent: independentCount,
            })}
          </p>
        </div>
        <ChevronDown className="h-4 w-4 shrink-0 text-fg-2 transition-transform group-data-[state=open]:rotate-180" />
      </CollapsibleTrigger>
      <CollapsibleContent>
        <div className="space-y-3 pt-3">
          <p className="text-xs text-fg-2">{t("hermes.profilePanel.hint")}</p>
          <div className="relative">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-fg-2" />
            <Input
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              placeholder={t("hermes.profilePanel.search")}
              className="h-8 pl-8 text-sm"
            />
          </div>
          <ScrollArea className="max-h-80 rounded-xl">
            <div className="space-y-2 pr-2">
              {filtered.map((profile) => {
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
                    className="rounded-xl border border-border bg-surface p-3 transition-colors hover:bg-subtle"
                  >
                    <div className="flex items-center gap-2">
                      <span className="min-w-0 flex-1 truncate text-sm font-medium">
                        {profile.name}
                      </span>
                      <Badge
                        variant={profile.independent ? "default" : "secondary"}
                        className="shrink-0"
                      >
                        {profile.independent
                          ? t("hermes.profilePanel.independent")
                          : t("hermes.profilePanel.direct")}
                      </Badge>
                      <Select
                        value={selectedRoute}
                        onValueChange={(value) => {
                          if (profile.independent) {
                            void run(profile.name, async () => {
                              await hermesProfileApi.switchRoute(
                                profile.name,
                                value,
                              );
                            });
                          } else {
                            setPendingRoute((prev) => ({
                              ...prev,
                              [profile.name]: value,
                            }));
                          }
                        }}
                      >
                        <SelectTrigger className="h-8 w-40 shrink-0 text-xs">
                          <SelectValue
                            placeholder={t(
                              "hermes.profilePanel.routePlaceholder",
                            )}
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
                      {profile.independent ? (
                        <Button
                          size="sm"
                          variant="quiet"
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
                          variant="outline"
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
                    {profile.independent && (
                      <div className="mt-2 flex items-center justify-end gap-4 border-t border-border pt-2">
                        <label className="flex items-center gap-1.5 text-xs text-fg-2">
                          {t("hermes.profilePanel.failover")}
                          <Switch
                            className="scale-90"
                            checked={profile.autoFailoverEnabled}
                            onCheckedChange={(checked) =>
                              void run(profile.name, () =>
                                hermesProfileApi.enter({
                                  profile: profile.name,
                                  routeProviderId: selectedRoute,
                                  autoFailover: checked,
                                  loadBalanceMode:
                                    profile.loadBalanceMode === "session"
                                      ? "session"
                                      : "off",
                                }),
                              )
                            }
                          />
                        </label>
                        <label className="flex items-center gap-1.5 text-xs text-fg-2">
                          {t("hermes.profilePanel.sticky")}
                          <Switch
                            className="scale-90"
                            checked={profile.loadBalanceMode === "session"}
                            onCheckedChange={(checked) =>
                              void run(profile.name, () =>
                                hermesProfileApi.enter({
                                  profile: profile.name,
                                  routeProviderId: selectedRoute,
                                  autoFailover: profile.autoFailoverEnabled,
                                  loadBalanceMode: checked ? "session" : "off",
                                }),
                              )
                            }
                          />
                        </label>
                      </div>
                    )}
                  </div>
                );
              })}
              {!filtered.length && (
                <p className={cn("py-6 text-center text-xs text-fg-2")}>
                  {t("hermes.profilePanel.empty")}
                </p>
              )}
            </div>
          </ScrollArea>
        </div>
      </CollapsibleContent>
    </Collapsible>
  );
}
