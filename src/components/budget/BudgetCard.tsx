import { useTranslation } from "react-i18next";
import { motion } from "framer-motion";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Pencil, Trash2 } from "lucide-react";
import { cn } from "@/lib/utils";
import type { BudgetStatus } from "@/types/budget";
import {
  SCOPE_LABEL_KEYS,
  PERIOD_LABEL_KEYS,
} from "@/types/budget";

interface BudgetCardProps {
  status: BudgetStatus;
  onEdit: () => void;
  onDelete: () => void;
  onToggleEnabled: (enabled: boolean) => void;
}

/** 进度条颜色：<70% 绿，70-95% 黄，>95% 红 */
function progressColor(pct: number | undefined): string {
  if (pct === undefined) return "bg-muted-foreground/30";
  if (pct > 0.95) return "bg-red-500";
  if (pct > 0.7) return "bg-amber-500";
  return "bg-emerald-500";
}

/** 格式化 token 数量 */
function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toString();
}

/** 格式化 USD 字符串 */
function formatUsd(s: string): string {
  const n = parseFloat(s);
  if (isNaN(n)) return s;
  return `$${n.toFixed(2)}`;
}

export function BudgetCard({
  status,
  onEdit,
  onDelete,
  onToggleEnabled,
}: BudgetCardProps) {
  const { t } = useTranslation();
  const { budget, consumedTokens, consumedUsd, pctTokens, pctUsd, remainingTokens, remainingUsd } = status;
  const isOverTokens = (pctTokens ?? 0) > 1;
  const isOverUsd = (pctUsd ?? 0) > 1;

  return (
    <motion.div
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3 }}
      className={cn(
        "rounded-xl border border-border/50 bg-card/40 backdrop-blur-sm p-4 space-y-3",
        !budget.enabled && "opacity-50",
      )}
    >
      {/* Header: name + scope badge + period + actions */}
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2 min-w-0">
          <h3 className="font-semibold truncate">{budget.name}</h3>
          <Badge variant="secondary" className="text-xs shrink-0">
            {t(SCOPE_LABEL_KEYS[budget.scope])}
          </Badge>
          <Badge variant="outline" className="text-xs shrink-0">
            {t(PERIOD_LABEL_KEYS[budget.period])}
          </Badge>
        </div>
        <div className="flex items-center gap-1.5 shrink-0">
          <Switch
            checked={budget.enabled}
            onCheckedChange={onToggleEnabled}
            className="scale-75"
          />
          <Button variant="ghost" size="icon" className="h-7 w-7" onClick={onEdit}>
            <Pencil className="h-3.5 w-3.5" />
          </Button>
          <Button variant="ghost" size="icon" className="h-7 w-7 text-destructive" onClick={onDelete}>
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>

      {/* Scope value (if not global) */}
      {budget.scopeValue && (
        <p className="text-xs text-muted-foreground truncate">
          {budget.scopeValue}
        </p>
      )}

      {/* Token progress */}
      {budget.limitTokens !== undefined && (
        <div className="space-y-1.5">
          <div className="flex justify-between text-xs">
            <span className="text-muted-foreground">
              Tokens: {t("budget.consumed")} {formatTokens(consumedTokens)}
            </span>
            <span className={cn(isOverTokens && "text-red-500 font-medium")}>
              {formatTokens(consumedTokens)} / {formatTokens(budget.limitTokens)}
            </span>
          </div>
          <div className="h-2 rounded-full bg-muted overflow-hidden">
            <div
              className={cn("h-full rounded-full transition-all duration-500", progressColor(pctTokens))}
              style={{ width: `${Math.min((pctTokens ?? 0) * 100, 100)}%` }}
            />
          </div>
          {remainingTokens !== undefined && (
            <p className={cn("text-xs", isOverTokens ? "text-red-500 font-medium" : "text-muted-foreground")}>
              {isOverTokens
                ? t("budget.overBudget")
                : `${t("budget.remaining")} ${formatTokens(remainingTokens)}`}
            </p>
          )}
        </div>
      )}

      {/* USD progress */}
      {budget.limitUsd !== undefined && (
        <div className="space-y-1.5">
          <div className="flex justify-between text-xs">
            <span className="text-muted-foreground">
              USD: {t("budget.consumed")} {formatUsd(consumedUsd)}
            </span>
            <span className={cn(isOverUsd && "text-red-500 font-medium")}>
              {formatUsd(consumedUsd)} / {formatUsd(budget.limitUsd)}
            </span>
          </div>
          <div className="h-2 rounded-full bg-muted overflow-hidden">
            <div
              className={cn("h-full rounded-full transition-all duration-500", progressColor(pctUsd))}
              style={{ width: `${Math.min((pctUsd ?? 0) * 100, 100)}%` }}
            />
          </div>
          {remainingUsd !== undefined && (
            <p className={cn("text-xs", isOverUsd ? "text-red-500 font-medium" : "text-muted-foreground")}>
              {isOverUsd
                ? t("budget.overBudget")
                : `${t("budget.remaining")} ${formatUsd(remainingUsd)}`}
            </p>
          )}
        </div>
      )}

      {/* If neither limit is set (shouldn't happen with validation, but safe fallback) */}
      {budget.limitTokens === undefined && budget.limitUsd === undefined && (
        <p className="text-xs text-muted-foreground">{t("budget.noLimit")}</p>
      )}
    </motion.div>
  );
}
