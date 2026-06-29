/**
 * 首页「路由模式」详情页选择器（Provider 路由 / 模型层级路由）。
 *
 * 只要当前 app 是 Claude 就渲染（无论路由模式是否开启）—— 这是可发现性的关键：
 * 从未开过路由模式的用户也能看到「模型层级路由」这张卡。
 * 点击卡片只切换当前查看/编辑的详情页；真实启用由详情页里的启用按钮处理。
 */
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";

export type RoutingMode = "provider" | "tier";

interface Props {
  value: RoutingMode;
  activeValue: RoutingMode;
  onSelectProvider: () => void;
  onSelectTier: () => void;
  onEnableProvider: () => void;
  onEnableTier: () => void;
}

export function RoutingModeSelector({
  value,
  activeValue,
  onSelectProvider,
  onSelectTier,
  onEnableProvider,
  onEnableTier,
}: Props) {
  const { t } = useTranslation();
  const activeLabel = t("home.routingMode.active", {
    defaultValue: "Enabled",
  });

  return (
    <div className="space-y-2">
      <span className="px-1 text-sm font-medium text-muted-foreground">
        {t("home.routingMode.label")}
      </span>
      <div role="tablist" className="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <RoutingCard
          selected={value === "provider"}
          active={activeValue === "provider"}
          onClick={onSelectProvider}
          title={t("home.routingMode.provider")}
          description={t("home.routingMode.providerDesc")}
          activeLabel={activeLabel}
          enableLabel={t("home.routingMode.enableProvider", {
            defaultValue: "Enable provider routing",
          })}
          onEnable={onEnableProvider}
        />
        <RoutingCard
          selected={value === "tier"}
          active={activeValue === "tier"}
          onClick={onSelectTier}
          title={t("home.routingMode.modelTier")}
          description={t("home.routingMode.modelTierDesc")}
          activeLabel={activeLabel}
          enableLabel={t("home.routingMode.enableTier", {
            defaultValue: "Enable model-tier routing",
          })}
          onEnable={onEnableTier}
        />
      </div>
    </div>
  );
}

interface CardProps {
  selected: boolean;
  active: boolean;
  onClick: () => void;
  title: string;
  description: string;
  activeLabel: string;
  enableLabel: string;
  onEnable: () => void;
}

function RoutingCard({
  selected,
  active,
  onClick,
  title,
  description,
  activeLabel,
  enableLabel,
  onEnable,
}: CardProps) {
  return (
    <div
      className={cn(
        "flex h-full w-full items-center gap-3 rounded-xl glass-card p-4 transition-all",
        selected && "glass-card-active",
      )}
    >
      <button
        type="button"
        role="tab"
        aria-selected={selected}
        onClick={onClick}
        className="min-w-0 flex-1 text-left focus:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <div className="min-w-0 space-y-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-sm font-semibold">{title}</span>
            {active && (
              <span className="rounded bg-primary/10 px-1.5 py-0.5 text-[10px] font-semibold text-primary">
                {activeLabel}
              </span>
            )}
          </div>
          <div className="text-xs text-muted-foreground">{description}</div>
        </div>
      </button>
      <button
        type="button"
        className="inline-flex shrink-0 items-center justify-center rounded-md bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground shadow-sm transition-colors hover:bg-primary/90 focus:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-primary"
        onClick={onEnable}
        disabled={active}
      >
        {enableLabel}
      </button>
    </div>
  );
}
