import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, Loader2 } from "lucide-react";
import { HoverTip } from "@/components/ui/hover-tip";
import { toast } from "@/lib/toast";
import { cn } from "@/lib/utils";
import {
  appTargetsApi,
  distroOf,
  enabledTargetKeys,
  sameTarget,
  type AppTargetAppId,
  type AppTargetsView,
  type TargetKey,
} from "@/lib/api/appTargets";
import {
  reasonLabel,
  targetLabel,
  useInvalidateAppTargets,
} from "@/hooks/useAppTargets";
import type { Provider } from "@/types";
import type { CardButton } from "./presentation";

interface ProviderEnvSwitchesProps {
  app: AppTargetAppId;
  provider: Provider;
  view: AppTargetsView;
  /** 主环境（Windows）现在用的就是这张卡 */
  mainActive: boolean;
  /** 主环境原来的主操作（切换 / 回到直连并使用），没有时表示已在用或不可用 */
  mainAction?: CardButton;
}

/**
 * 供应商卡上每个写入环境一个启用开关：各环境独立选择直连供应商。主环境沿用原来的切换
 * （含路由确认等规则），其他环境只写那一个环境。
 */
export function ProviderEnvSwitches({
  app,
  provider,
  view,
  mainActive,
  mainAction,
}: ProviderEnvSwitchesProps) {
  const { t } = useTranslation();
  const invalidate = useInvalidateAppTargets();
  const [busy, setBusy] = useState<TargetKey | null>(null);

  const activate = async (key: TargetKey) => {
    setBusy(key);
    try {
      const report = await appTargetsApi.activate(app, key, provider.id);
      if (report.status === "ok") {
        toast.success(
          t("appTargets.switch.done", {
            provider: provider.name,
            target: targetLabel(key),
          }),
        );
      } else {
        toast.warning(
          t("appTargets.switch.failed", {
            provider: provider.name,
            target: targetLabel(key),
          }),
          {
            description: [reasonLabel(t, report.reason), report.detail]
              .filter(Boolean)
              .join(" · "),
            duration: 8000,
          },
        );
      }
    } catch (error) {
      toast.error(
        t("appTargets.switch.failed", {
          provider: provider.name,
          target: targetLabel(key),
        }),
        { description: String(error), duration: 8000 },
      );
    } finally {
      setBusy(null);
      await invalidate();
    }
  };

  return (
    <div
      className="flex shrink-0 items-center gap-1"
      role="group"
      aria-label={t("appTargets.switch.group")}
    >
      {enabledTargetKeys(view.config).map((key) => {
        const isMain = sameTarget(key, view.config.primary);
        const active = isMain
          ? mainActive
          : view.records[key]?.providerId === provider.id;
        const disabledReason = isMain ? mainAction?.disabledReason : undefined;
        const label = distroOf(key) ?? "Windows";
        const tip = active
          ? t("appTargets.switch.inUse", { target: targetLabel(key) })
          : disabledReason ||
            t("appTargets.switch.enable", { target: targetLabel(key) });
        const loading = busy !== null && sameTarget(busy, key);
        return (
          <HoverTip key={key} content={tip}>
            <button
              type="button"
              aria-pressed={active}
              aria-label={tip}
              disabled={busy !== null || (!active && Boolean(disabledReason))}
              onClick={() => {
                if (active) return;
                if (isMain) mainAction?.onClick();
                else void activate(key);
              }}
              className={cn(
                "inline-flex h-6 max-w-[104px] items-center gap-1 rounded-full border px-2 text-badge transition-colors",
                active
                  ? "cursor-default border-direct-border bg-direct-soft text-direct-text"
                  : "border-border-strong text-fg-2 hover:bg-subtle hover:text-fg-1",
                "disabled:opacity-60",
              )}
            >
              {loading ? (
                <Loader2 className="h-3 w-3 shrink-0 animate-spin" />
              ) : active ? (
                <Check className="h-3 w-3 shrink-0" />
              ) : (
                <span
                  aria-hidden
                  className="h-1.5 w-1.5 shrink-0 rounded-full border border-fg-3"
                />
              )}
              <span className="min-w-0 truncate">{label}</span>
            </button>
          </HoverTip>
        );
      })}
    </div>
  );
}
