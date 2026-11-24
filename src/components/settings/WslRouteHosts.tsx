import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { HoverTip } from "@/components/ui/hover-tip";
import {
  SettingsBlock,
  SettingsCard,
  SettingsRow,
} from "@/components/settings/SettingsLayout";
import { toast } from "@/lib/toast";
import {
  appTargetsApi,
  runningWslDistros,
  type WslRouteInfo,
} from "@/lib/api/appTargets";
import {
  APP_TARGETS_QUERY_KEY,
  useAppTargetsOverview,
  WSL_ROUTE_INFO_QUERY_KEY,
} from "@/hooks/useAppTargets";

/**
 * 设置 → 本地路由 → WSL 客户端地址：接入路由的 WSL 环境写给客户端的代理主机，按发行版
 * 保存。只读缓存和 UNC 文件，不运行 WSL shell；也可手动填写 Windows 主机地址。
 */
export function WslRouteHosts() {
  const { t } = useTranslation();
  const overview = useAppTargetsOverview();
  const distros = runningWslDistros(overview.data?.inventory)
    .map((env) => env.distro.toLowerCase())
    .sort();
  const enabled = Boolean(overview.data?.multiEnv) && distros.length > 0;
  const infoKey = [...WSL_ROUTE_INFO_QUERY_KEY, distros] as const;
  const queryClient = useQueryClient();
  const info = useQuery({
    queryKey: infoKey,
    queryFn: () => appTargetsApi.wslRouteInfo(false),
    enabled,
    staleTime: 60_000,
  });
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<string | null>(null);
  const [detecting, setDetecting] = useState(false);
  const rows =
    info.data?.filter((row) => distros.includes(row.distro.toLowerCase())) ??
    [];
  if (!enabled || !rows.length) return null;

  const detect = async () => {
    setDetecting(true);
    try {
      queryClient.setQueryData(infoKey, await appTargetsApi.wslRouteInfo(true));
    } catch (error) {
      toast.error(String(error));
    } finally {
      setDetecting(false);
    }
  };

  const save = async (row: WslRouteInfo) => {
    setBusy(row.distro);
    try {
      await appTargetsApi.setWslClientHost(
        row.distro,
        drafts[row.distro] ?? row.savedHost ?? "",
      );
      toast.success(t("appTargets.routeHosts.saved", { distro: row.distro }));
      setDrafts((d) => {
        const next = { ...d };
        delete next[row.distro];
        return next;
      });
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: WSL_ROUTE_INFO_QUERY_KEY }),
        queryClient.invalidateQueries({ queryKey: APP_TARGETS_QUERY_KEY }),
      ]);
    } catch (error) {
      toast.error(String(error));
    } finally {
      setBusy(null);
    }
  };

  return (
    <SettingsBlock
      title={t("appTargets.routeHosts.title")}
      help={{
        title: t("appTargets.routeHosts.title"),
        body: t("appTargets.routeHosts.help"),
      }}
      actions={
        <Button
          variant="quiet"
          size="compact"
          disabled={detecting}
          onClick={() => void detect()}
        >
          <RefreshCw
            className={detecting ? "h-3.5 w-3.5 animate-spin" : "h-3.5 w-3.5"}
          />
          {t("appTargets.routeHosts.detect")}
        </Button>
      }
    >
      <SettingsCard>
        {rows.map((row) => {
          const value = drafts[row.distro] ?? row.savedHost ?? "";
          const dirty = value.trim() !== (row.savedHost ?? "");
          return (
            <SettingsRow
              key={row.distro}
              label={`WSL · ${row.distro}`}
              description={
                <>
                  {row.networkingMode
                    ? t("appTargets.routeHosts.mode", {
                        mode: row.networkingMode,
                      })
                    : t("appTargets.routeHosts.modeUnknown")}
                  {row.listenUnreachable && (
                    <span className="block text-warning-text">
                      {t("appTargets.routeHosts.listenLoopback", {
                        listen: row.listenAddress,
                      })}
                    </span>
                  )}
                </>
              }
              control={
                <div className="flex items-center gap-2">
                  <HoverTip
                    content={value || row.detectedHost}
                    disableHoverableContent
                  >
                    <Input
                      value={value}
                      placeholder={
                        row.detectedHost ?? t("appTargets.routeHosts.noHost")
                      }
                      aria-label={t("appTargets.routeHosts.hostFor", {
                        distro: row.distro,
                      })}
                      className="w-[200px]"
                      onChange={(event) => {
                        const value = event.target.value;
                        setDrafts((d) => ({
                          ...d,
                          [row.distro]: value,
                        }));
                      }}
                    />
                  </HoverTip>
                  <Button
                    variant="neutral"
                    size="compact"
                    disabled={!dirty || busy !== null}
                    onClick={() => void save(row)}
                  >
                    {t("common.save")}
                  </Button>
                </div>
              }
            />
          );
        })}
      </SettingsCard>
    </SettingsBlock>
  );
}
