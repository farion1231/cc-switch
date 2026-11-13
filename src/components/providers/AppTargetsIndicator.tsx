import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { Layers, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { Dot, toneOf } from "@/components/settings/AppTargetsEditor";
import {
  appTargetsApi,
  enabledTargetKeys,
  isAppTargetApp,
  isWindowsOnly,
  routeFlag,
  sameTarget,
  type TargetKey,
} from "@/lib/api/appTargets";
import {
  reasonLabel,
  statusLabel,
  targetLabel,
  useAppTargetView,
  useInvalidateAppTargets,
} from "@/hooks/useAppTargets";
import type { Provider } from "@/types";

const NEEDS_SYNC = ["error", "deferred", "external"];

/**
 * 供应商页头的「写入目标」：目标不是默认「仅 Windows」时出现。点开看每个环境的目录、
 * 写到了哪家、最近结果，并能只重试未同步的目标（不只靠悬停）。
 */
export function AppTargetsIndicator({
  app,
  providers,
  currentProviderId,
}: {
  app: string;
  providers?: Record<string, Provider>;
  /** 主环境（Windows）正在用的那家 */
  currentProviderId?: string;
}) {
  const { t } = useTranslation();
  const view = useAppTargetView(app);
  const invalidate = useInvalidateAppTargets();
  const [busy, setBusy] = useState(false);

  if (!isAppTargetApp(app) || !view || view.source !== "saved") return null;
  if (isWindowsOnly(view.config)) return null;

  const enabled = enabledTargetKeys(view.config);
  const followers = enabled.filter(
    (key) => !sameTarget(key, view.config.primary),
  );
  const pending = followers.filter((key) =>
    NEEDS_SYNC.includes(view.records[key]?.status ?? "deferred"),
  );
  const worst = followers.some((k) => view.records[k]?.status === "error")
    ? "bad"
    : pending.length
      ? "warn"
      : "ok";

  const retry = async (keys?: TargetKey[]) => {
    setBusy(true);
    try {
      const result = await appTargetsApi.sync(app, keys);
      const failed = result.targets.filter((r) => r.status === "error");
      if (failed.length) {
        toast.warning(
          t("appTargets.toast.partial", {
            targets: failed.map((r) => targetLabel(r.target)).join("、"),
          }),
          { description: failed[0].detail },
        );
      } else {
        toast.success(t("appTargets.toast.synced"));
      }
    } catch (error) {
      toast.error(t("appTargets.toast.syncFailed"), {
        description: String(error),
      });
    } finally {
      setBusy(false);
      await invalidate();
    }
  };

  // 路由 / 聚合按应用共用一套；Claude Code、Codex、Gemini 的其他环境可以选择接入。
  const routeCapable = app !== "opencode";

  const toggleRoute = async (key: TargetKey, join: boolean) => {
    setBusy(true);
    try {
      const report = await appTargetsApi.setRoute(app, key, join);
      if (report.status === "error") {
        toast.warning(
          t("appTargets.route.failed", { target: targetLabel(key) }),
          {
            description: [reasonLabel(t, report.reason), report.detail]
              .filter(Boolean)
              .join(" · "),
            duration: 8000,
          },
        );
      } else {
        toast.success(
          report.routed
            ? t("appTargets.route.joined", { target: targetLabel(key) })
            : join
              ? t("appTargets.route.joinLater", { target: targetLabel(key) })
              : t("appTargets.route.left", { target: targetLabel(key) }),
        );
      }
    } catch (error) {
      toast.error(String(error));
    } finally {
      setBusy(false);
      await invalidate();
    }
  };

  const providerName = (id?: string) =>
    id ? (providers?.[id]?.name ?? id) : undefined;

  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button
          variant="quiet"
          size="compact"
          aria-label={t("appTargets.indicator.title")}
        >
          <Layers className="h-4 w-4" />
          <span className="tabular-nums">{enabled.length}</span>
          <Dot tone={worst} />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-[380px] p-0">
        <div className="border-b border-border px-3 py-2">
          <div className="text-body font-medium text-fg-1">
            {t("appTargets.indicator.title")}
          </div>
          <div className="text-caption text-fg-2">
            {t("appTargets.indicator.subtitle")}
          </div>
        </div>
        <ul className="max-h-[320px] divide-y divide-border overflow-auto">
          {enabled.map((key) => {
            const primary = sameTarget(key, view.config.primary);
            const record = view.records[key];
            const status = primary ? "ok" : (record?.status ?? "deferred");
            return (
              <li key={key} className="space-y-0.5 px-3 py-2">
                <div className="flex items-center gap-2">
                  <span className="inline-flex h-[18px] shrink-0 items-center rounded-full border border-border-strong px-1.5 text-badge text-fg-2">
                    {targetLabel(key)}
                  </span>
                  <span className="ms-auto inline-flex items-center gap-1.5 text-caption text-fg-1">
                    <Dot tone={toneOf(status)} />
                    {!primary && record?.routed
                      ? t("appTargets.route.routed")
                      : statusLabel(t, status)}
                  </span>
                </div>
                <div className="break-all text-caption text-fg-2">
                  {view.resolvedDirs[key] ?? record?.dir ?? "—"}
                </div>
                {primary ? (
                  currentProviderId && (
                    <div className="text-caption text-fg-2">
                      {t("appTargets.indicator.provider", {
                        name: providerName(currentProviderId),
                      })}
                    </div>
                  )
                ) : (
                  <div className="text-caption text-fg-2">
                    {record?.providerId
                      ? t("appTargets.indicator.provider", {
                          name: providerName(record.providerId),
                        })
                      : t("appTargets.indicator.noProvider")}
                    {record?.reason && ` · ${reasonLabel(t, record.reason)}`}
                    {record?.at
                      ? ` · ${new Date(record.at * 1000).toLocaleString()}`
                      : ""}
                  </div>
                )}
                {!primary && record?.detail && (
                  <div className="break-all text-caption text-danger-text">
                    {record.detail}
                  </div>
                )}
                {!primary && record?.warnings?.includes("listen_loopback") && (
                  <div className="text-caption text-warning-text">
                    {t("appTargets.route.listenLoopback")}
                  </div>
                )}
                {!primary && routeCapable && (
                  <label className="flex items-center gap-2 pt-1 text-caption text-fg-2">
                    <Switch
                      checked={routeFlag(view.config, key)}
                      disabled={busy}
                      aria-label={t("appTargets.route.joinFor", {
                        target: targetLabel(key),
                      })}
                      onCheckedChange={(value) => void toggleRoute(key, value)}
                    />
                    {t("appTargets.route.join")}
                  </label>
                )}
              </li>
            );
          })}
        </ul>
        <div className="flex justify-end gap-2 border-t border-border px-3 py-2">
          <Button
            variant="neutral"
            size="compact"
            disabled={busy || followers.length === 0}
            onClick={() => void retry()}
          >
            {t("appTargets.indicator.syncAll")}
          </Button>
          <Button
            variant="solid"
            size="compact"
            disabled={busy || pending.length === 0}
            onClick={() => void retry(pending)}
          >
            {busy && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
            {t("appTargets.indicator.retry", { count: pending.length })}
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  );
}
