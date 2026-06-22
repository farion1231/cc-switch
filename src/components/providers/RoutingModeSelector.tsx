/**
 * 首页「路由模式」二选一选择器（Provider 路由 / 模型层级路由）。
 *
 * 只要当前 app 是 Claude 就渲染（无论路由模式是否开启）—— 这是可发现性的关键：
 * 从未开过路由模式的用户也能看到「模型层级路由」这张卡。
 *
 * - 路由模式开启：两张卡都正常可点，选谁谁生效。
 * - 路由模式关闭：模型层级卡是「锁定态」（灰底 + 锁图标 + 提示），但保持可点击 ——
 *   点击直接触发开路由流程（由 App 的 onSelectTier 桥接到确认弹窗）。
 */
import { CheckCircle2, Circle, Lock } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";

export type RoutingMode = "provider" | "tier";

interface Props {
  value: RoutingMode;
  routingEnabled: boolean;
  onSelectProvider: () => void;
  onSelectTier: () => void;
}

export function RoutingModeSelector({
  value,
  routingEnabled,
  onSelectProvider,
  onSelectTier,
}: Props) {
  const { t } = useTranslation();

  return (
    <div className="space-y-2">
      <span className="px-1 text-sm font-medium text-muted-foreground">
        {t("home.routingMode.label")}
      </span>
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <RoutingCard
          selected={value === "provider"}
          onClick={onSelectProvider}
          title={t("home.routingMode.provider")}
          description={t("home.routingMode.providerDesc")}
        />
        <RoutingCard
          selected={value === "tier"}
          onClick={onSelectTier}
          title={t("home.routingMode.modelTier")}
          description={t("home.routingMode.modelTierDesc")}
          locked={!routingEnabled}
          lockedHint={t("home.routingMode.tierLockedHint")}
        />
      </div>
    </div>
  );
}

interface CardProps {
  selected: boolean;
  onClick: () => void;
  title: string;
  description: string;
  locked?: boolean;
  lockedHint?: string;
}

function RoutingCard({
  selected,
  onClick,
  title,
  description,
  locked,
  lockedHint,
}: CardProps) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      onClick={onClick}
      className={cn(
        "relative rounded-xl glass-card p-4 text-left transition-all",
        "focus:outline-none focus-visible:ring-2 focus-visible:ring-ring",
        selected && "glass-card-active",
        locked && "opacity-60",
      )}
    >
      {locked && (
        <Lock className="absolute right-3 top-3 h-4 w-4 text-muted-foreground" />
      )}
      <div className="flex items-start gap-3">
        {selected ? (
          <CheckCircle2 className="mt-0.5 h-5 w-5 shrink-0 text-primary" />
        ) : (
          <Circle className="mt-0.5 h-5 w-5 shrink-0 text-muted-foreground" />
        )}
        <div className="min-w-0 space-y-1">
          <div className="text-sm font-semibold">{title}</div>
          <div className="text-xs text-muted-foreground">{description}</div>
          {locked && lockedHint && (
            <div className="pt-1 text-xs text-muted-foreground/80">
              {lockedHint}
            </div>
          )}
        </div>
      </div>
    </button>
  );
}
