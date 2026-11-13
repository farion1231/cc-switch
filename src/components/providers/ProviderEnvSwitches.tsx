import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, Loader2 } from "lucide-react";
import { HoverTip } from "@/components/ui/hover-tip";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { resolveManagedAccountId } from "@/lib/authBinding";
import { toast } from "@/lib/toast";
import { cn } from "@/lib/utils";
import {
  appTargetsApi,
  distroOf,
  enabledTargetKeys,
  sameTarget,
  wslTargetKey,
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
  /** 装了这个客户端、还没加入写入环境的 WSL 发行版：点了先确认，再加入并启用 */
  candidates?: string[];
}

/** 绑了 CC Switch 托管账号的官方卡：只能在 Windows 用（与后端 is_managed_login 一致）。 */
export function isManagedLogin(app: string, provider: Provider): boolean {
  if (app === "claude") {
    return (
      provider.category === "official" &&
      Boolean(resolveManagedAccountId(provider.meta, "claude_official")?.trim())
    );
  }
  if (app === "codex") {
    return Boolean(
      resolveManagedAccountId(provider.meta, "codex_oauth")?.trim(),
    );
  }
  return false;
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
  candidates = [],
}: ProviderEnvSwitchesProps) {
  const { t } = useTranslation();
  const invalidate = useInvalidateAppTargets();
  const [busy, setBusy] = useState<TargetKey | null>(null);
  const [confirming, setConfirming] = useState<TargetKey | null>(null);
  const managed = isManagedLogin(app, provider);
  const candidateKeys = candidates.map(wslTargetKey);

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
      {[...enabledTargetKeys(view.config), ...candidateKeys].map((key) => {
        const isMain = sameTarget(key, view.config.primary);
        const isCandidate = candidateKeys.some((c) => sameTarget(c, key));
        const active = isMain
          ? mainActive
          : !isCandidate && view.records[key]?.providerId === provider.id;
        const disabledReason = isMain
          ? mainAction?.disabledReason
          : managed
            ? t("appTargets.switch.managedLogin")
            : undefined;
        const label = distroOf(key) ?? "Windows";
        const tip = active
          ? t("appTargets.switch.inUse", { target: targetLabel(key) })
          : disabledReason ||
            (isCandidate
              ? t("appTargets.switch.addAndEnable", {
                  target: targetLabel(key),
                })
              : t("appTargets.switch.enable", { target: targetLabel(key) }));
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
                else if (isCandidate) setConfirming(key);
                else void activate(key);
              }}
              className={cn(
                "inline-flex h-6 max-w-[104px] items-center gap-1 rounded-full border px-2 text-badge transition-colors",
                isCandidate && "border-dashed",
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
      <ConfirmDialog
        isOpen={confirming !== null}
        variant="info"
        zIndex="top"
        title={t("appTargets.switch.addTitle", {
          target: confirming ? targetLabel(confirming) : "",
        })}
        message={t("appTargets.switch.addMessage", {
          target: confirming ? targetLabel(confirming) : "",
          provider: provider.name,
        })}
        confirmText={t("appTargets.switch.addConfirm")}
        onConfirm={() => {
          const key = confirming;
          setConfirming(null);
          if (key) void activate(key);
        }}
        onCancel={() => setConfirming(null)}
      />
    </div>
  );
}
