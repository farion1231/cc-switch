import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { HoverTip } from "@/components/ui/hover-tip";
import {
  SettingsBlock,
  SettingsCard,
  SettingsRow,
} from "@/components/settings/SettingsLayout";
import { toast } from "@/lib/toast";
import { appTargetsApi, type WslRouteInfo } from "@/lib/api/appTargets";
import { useAppTargetsOverview } from "@/hooks/useAppTargets";

const QUERY_KEY = ["appTargets", "wslRouteInfo"] as const;

/**
 * 设置 → 本地路由 → WSL 客户端地址：接入路由的 WSL 环境写给客户端的代理主机，按发行版
 * 保存。自动检测只给候选地址（mirrored 用 127.0.0.1，NAT 用默认网关），「测试连接」才是
 * 从发行版里真的连一次。只在 Windows 上、检测到发行版时出现。
 */
export function WslRouteHosts() {
  const { t } = useTranslation();
  const overview = useAppTargetsOverview();
  const enabled = Boolean(overview.data?.multiEnv);
  const queryClient = useQueryClient();
  const info = useQuery({
    queryKey: QUERY_KEY,
    queryFn: () => appTargetsApi.wslRouteInfo(false),
    enabled,
    staleTime: 60_000,
  });
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<string | null>(null);
  const [detecting, setDetecting] = useState(false);
  const [results, setResults] = useState<
    Record<string, { ok: boolean; text: string }>
  >({});

  if (!enabled || !info.data?.length) return null;

  const detect = async () => {
    setDetecting(true);
    try {
      queryClient.setQueryData(
        QUERY_KEY,
        await appTargetsApi.wslRouteInfo(true),
      );
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
      await queryClient.invalidateQueries({ queryKey: QUERY_KEY });
    } catch (error) {
      toast.error(String(error));
    } finally {
      setBusy(null);
    }
  };

  const test = async (row: WslRouteInfo) => {
    setBusy(row.distro);
    try {
      const result = await appTargetsApi.testWslRoute(row.distro);
      setResults((r) => ({
        ...r,
        [row.distro]: {
          ok: result.ok,
          text: result.ok
            ? t("appTargets.routeHosts.testOk", { url: result.url })
            : t("appTargets.routeHosts.testFailed", {
                url: result.url,
                detail: result.detail || "—",
              }),
        },
      }));
    } catch (error) {
      setResults((r) => ({
        ...r,
        [row.distro]: { ok: false, text: String(error) },
      }));
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
        {info.data.map((row) => {
          const value = drafts[row.distro] ?? row.savedHost ?? "";
          const dirty = value.trim() !== (row.savedHost ?? "");
          const result = results[row.distro];
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
                  {result && (
                    <span
                      className={
                        result.ok
                          ? "block text-success-text"
                          : "block text-danger-text"
                      }
                    >
                      {result.text}
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
                      onChange={(event) =>
                        setDrafts((d) => ({
                          ...d,
                          [row.distro]: event.target.value,
                        }))
                      }
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
                  <Button
                    variant="neutral"
                    size="compact"
                    disabled={busy !== null}
                    onClick={() => void test(row)}
                  >
                    {busy === row.distro && (
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    )}
                    {t("appTargets.routeHosts.test")}
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
