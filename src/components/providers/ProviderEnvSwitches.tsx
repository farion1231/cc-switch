import { useTranslation } from "react-i18next";
import { Check } from "lucide-react";
import { HoverTip } from "@/components/ui/hover-tip";
import { resolveManagedAccountId } from "@/lib/authBinding";
import { cn } from "@/lib/utils";
import {
  distroOf,
  visibleTargetKeys,
  type AppTargetAppId,
  type AppTargetsView,
  type WslInventory,
} from "@/lib/api/appTargets";
import { targetLabel } from "@/hooks/useAppTargets";
import type { Provider } from "@/types";
import type { CardButton } from "./presentation";

interface ProviderEnvSwitchesProps {
  app: AppTargetAppId;
  provider: Provider;
  view: AppTargetsView;
  inventory: WslInventory | undefined;
  mainActive: boolean;
  mainAction?: CardButton;
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
 * 当前只允许 Windows 选择供应商，WSL 显示跟随状态。
 * 后续 PR 可在这里增加独立目标选择；必须新增显式配置，不把同步结果当成选择。
 */
export function ProviderEnvSwitches({
  app,
  provider,
  view,
  inventory,
  mainActive,
  mainAction,
}: ProviderEnvSwitchesProps) {
  const { t } = useTranslation();
  const managed = isManagedLogin(app, provider);
  return (
    <div
      className="flex shrink-0 items-center gap-1"
      role="group"
      aria-label={t("appTargets.switch.group")}
    >
      {visibleTargetKeys(view.config, inventory).map((key) => {
        const windows = key === "windows";
        const active = mainActive && (windows || !managed);
        const tip = !windows
          ? t(
              managed
                ? "appTargets.switch.managedLogin"
                : "appTargets.switch.followsWindows",
            )
          : mainAction?.disabledReason ||
            t(
              active
                ? "appTargets.switch.inUseMain"
                : "appTargets.switch.enable",
              { target: targetLabel(key) },
            );
        return (
          <HoverTip key={key} content={tip}>
            <span className="inline-flex">
              <button
                type="button"
                aria-pressed={active}
                aria-label={tip}
                disabled={!windows || Boolean(mainAction?.disabledReason)}
                onClick={() => {
                  if (windows && !active) mainAction?.onClick();
                }}
                className={cn(
                  "inline-flex h-6 max-w-[104px] items-center gap-1 rounded-full border px-2 text-badge",
                  active
                    ? "border-border-strong bg-selected text-fg-1"
                    : "border-border text-fg-2",
                )}
              >
                {active && <Check className="h-3 w-3 shrink-0" />}
                <span className="min-w-0 truncate">
                  {distroOf(key) ?? "Windows"}
                </span>
              </button>
            </span>
          </HoverTip>
        );
      })}
    </div>
  );
}
