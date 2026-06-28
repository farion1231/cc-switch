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
import { CheckCircle2, Circle, Lock, Pencil } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";

export type RoutingMode = "provider" | "tier";

interface Props {
  value: RoutingMode;
  routingEnabled: boolean;
  onSelectProvider: () => void;
  onSelectTier: () => void;
  onEditTier?: () => void;
}

export function RoutingModeSelector({
  value,
  routingEnabled,
  onSelectProvider,
  onSelectTier,
  onEditTier,
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
          onEdit={onEditTier}
          editLabel={t("home.routingMode.editTier", {
            defaultValue: "Edit tier mappings",
          })}
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
  onEdit?: () => void;
  editLabel?: string;
}

function RoutingCard({
  selected,
  onClick,
  title,
  description,
  locked,
  lockedHint,
  onEdit,
  editLabel,
}: CardProps) {
  // 外层是 <button role="radio">，不能再内嵌 <button>，所以 edit 按钮做成兄弟节点：
  // 用 relative 包裹层承载卡片 button + 绝对定位的 edit 按钮，点击靠 stopPropagation 隔离。
  const buttonClass = cn(
    "relative h-full w-full rounded-xl glass-card p-4 text-left transition-all",
    "focus:outline-none focus-visible:ring-2 focus-visible:ring-ring",
    selected && "glass-card-active",
    locked && "opacity-60",
    onEdit && "pr-14",
  );

  const inner = (
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
  );

  if (!onEdit) {
    return (
      <button
        type="button"
        role="radio"
        aria-checked={selected}
        onClick={onClick}
        className={buttonClass}
      >
        {inner}
      </button>
    );
  }

  return (
    <div className="relative h-full">
      <button
        type="button"
        role="radio"
        aria-checked={selected}
        onClick={onClick}
        className={buttonClass}
      >
        {inner}
      </button>
      <div className="absolute right-2 top-2 z-10 flex items-center gap-1">
        {locked && <Lock className="h-4 w-4 text-muted-foreground" />}
        <button
          type="button"
          title={editLabel}
          aria-label={editLabel}
          className="inline-flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground hover:bg-foreground/10 hover:text-foreground focus:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          onClick={(e) => {
            e.stopPropagation();
            onEdit();
          }}
        >
          <Pencil className="h-3.5 w-3.5" />
        </button>
      </div>
    </div>
  );
}
