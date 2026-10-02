import * as React from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle, Check, Search, X } from "lucide-react";
import { toast } from "sonner";
import * as PopoverPrimitive from "@radix-ui/react-popover";
import type { AppId } from "@/lib/api/types";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import { AppGlyph, APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { cn } from "@/lib/utils";

/**
 * MCP 与 Skills 共用的「应用矩阵」：行 = 服务器 / Skill，列 = 应用。
 * 中性勾选（不用绿色），写入失败显示 ⚠ 可重试；列头点开是批量弹层，批量后给撤销 toast。
 */

// ─── 批量作用范围（只在这里决定） ─────────────────────────────────────────
export type BulkScopeKind = "all" | "search" | "filter";

/**
 * 批量操作实际作用的行，以及弹层里怎么描述这个范围。
 * 列头批量只作用于当前搜索 / 筛选出的行（画板 McpBulk 的写法，有意改变旧版
 * 「搜索时也作用于全集」），弹层写明范围，执行后给撤销。
 */
export function resolveBulkScope<T>(
  allRows: readonly T[],
  visibleRows: readonly T[],
  narrowedBy: "search" | "filter" | null,
): { rows: readonly T[]; kind: BulkScopeKind } {
  if (!narrowedBy) {
    return { rows: allRows, kind: "all" };
  }
  return { rows: visibleRows, kind: narrowedBy };
}

// ─── 单元格 ─────────────────────────────────────────────────────────────
export type MatrixCellState = "on" | "off" | "fail";

interface MatrixCellProps {
  state: MatrixCellState;
  /** 「serena · Codex：已启用」 */
  label: string;
  onClick: () => void;
  disabled?: boolean;
}

export function MatrixCell({
  state,
  label,
  onClick,
  disabled,
}: MatrixCellProps) {
  return (
    <span className="flex w-9 shrink-0 justify-center">
      <button
        type="button"
        aria-pressed={state === "on"}
        aria-label={label}
        title={label}
        disabled={disabled}
        onClick={(event) => {
          event.stopPropagation();
          onClick();
        }}
        className={cn(
          "flex h-7 w-7 items-center justify-center rounded-control text-fg-1 transition-[background-color,box-shadow] duration-150 hover:shadow-[inset_0_0_0_1px_var(--border-strong)] disabled:cursor-not-allowed disabled:opacity-60",
          state === "on" && "bg-selected dark:bg-border-strong",
        )}
      >
        {state === "on" && (
          <Check aria-hidden="true" className="h-3.5 w-3.5" strokeWidth={2} />
        )}
        {state === "off" && (
          <span
            aria-hidden="true"
            className="h-3 w-3 rounded-full border-[1.5px] border-border-strong"
          />
        )}
        {state === "fail" && (
          <AlertTriangle
            aria-hidden="true"
            className="h-3.5 w-3.5 text-warning"
            strokeWidth={2}
          />
        )}
      </button>
    </span>
  );
}

// ─── 列头 + 批量弹层 ─────────────────────────────────────────────────────
interface MatrixColumnHeaderProps {
  app: AppId;
  /** 列头上的数字：全部行里启用了几个 */
  enabledCount: number;
  totalCount: number;
  /** 批量作用范围内的行数 / 已启用数 / 写入失败数 */
  scopeTotal: number;
  scopeEnabled: number;
  scopeFailed: number;
  scopeKind: BulkScopeKind;
  /** 「MCP」「Skill」 */
  noun: string;
  onEnableRest: () => void;
  onDisableAll: () => void;
  /** 弹层里额外的文字按钮（Skills：「只看在 Codex 启用的」） */
  extraAction?: { label: string; onClick: () => void };
  /** 标题旁的「?」（Pi：怎么算启用） */
  help?: { title: string; body: React.ReactNode };
  /** 列头的 title（Pi 说明按目录判断） */
  title?: string;
  disabled?: boolean;
}

export function MatrixColumnHeader({
  app,
  enabledCount,
  totalCount,
  scopeTotal,
  scopeEnabled,
  scopeFailed,
  scopeKind,
  noun,
  onEnableRest,
  onDisableAll,
  extraAction,
  help,
  title,
  disabled,
}: MatrixColumnHeaderProps) {
  const { t } = useTranslation();
  const [open, setOpen] = React.useState(false);
  const countId = React.useId();
  const name = APP_DISPLAY_NAME[app];
  const rest = scopeTotal - scopeEnabled;
  const allOn = scopeTotal > 0 && rest === 0;
  const noneOn = scopeEnabled === 0;

  const run = (action: () => void) => {
    setOpen(false);
    action();
  };

  return (
    <PopoverPrimitive.Root open={open} onOpenChange={setOpen}>
      <span className="flex w-9 shrink-0 justify-center">
        <PopoverPrimitive.Trigger asChild>
          <button
            type="button"
            disabled={disabled}
            aria-label={t("appMatrix.columnAria", {
              app: name,
              on: enabledCount,
              total: totalCount,
              noun,
            })}
            title={title ?? name}
            className="flex h-10 w-[34px] flex-col items-center justify-center gap-0.5 rounded-control transition-colors duration-150 hover:bg-selected disabled:cursor-not-allowed disabled:opacity-60 data-[state=open]:bg-selected"
          >
            <AppGlyph app={app} size={16} badgeClassName="bg-subtle" />
            <span className="text-badge font-medium tabular-nums text-fg-2">
              {enabledCount}
            </span>
          </button>
        </PopoverPrimitive.Trigger>
      </span>
      <PopoverPrimitive.Portal>
        <PopoverPrimitive.Content
          align="end"
          sideOffset={4}
          collisionPadding={8}
          className="z-[110] flex w-[260px] flex-col gap-2.5 rounded-panel border border-border bg-surface px-3.5 pb-3.5 pt-3 text-fg-1 shadow-v7-md outline-none"
        >
          <div className="flex items-center gap-2">
            <AppGlyph app={app} size={16} badgeClassName="bg-surface" />
            <span className="text-body font-semibold">{name}</span>
            {help && <HelpTip title={help.title}>{help.body}</HelpTip>}
          </div>
          <div className="flex flex-col">
            <span id={countId} className="text-caption text-fg-2">
              {t("appMatrix.pop.count", {
                on: scopeEnabled,
                total: scopeTotal,
                noun,
              })}
            </span>
            {scopeFailed > 0 && (
              <span className="text-caption text-warning-text">
                {t("appMatrix.pop.failCount", { count: scopeFailed })}
              </span>
            )}
          </div>
          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              variant="neutral"
              size="compact"
              aria-disabled={allOn || scopeTotal === 0}
              aria-describedby={allOn ? countId : undefined}
              className="aria-disabled:cursor-not-allowed aria-disabled:opacity-45"
              onClick={() => {
                if (allOn || scopeTotal === 0) return;
                run(onEnableRest);
              }}
            >
              {allOn
                ? t("appMatrix.pop.enableAll")
                : t("appMatrix.pop.enableRest", { count: rest })}
            </Button>
            <Button
              type="button"
              variant="neutral"
              size="compact"
              aria-disabled={noneOn}
              aria-describedby={noneOn ? countId : undefined}
              className="aria-disabled:cursor-not-allowed aria-disabled:opacity-45"
              onClick={() => {
                if (noneOn) return;
                run(onDisableAll);
              }}
            >
              {t("appMatrix.pop.disableAll")}
            </Button>
          </div>
          {extraAction && (
            <button
              type="button"
              onClick={() => run(extraAction.onClick)}
              className="self-start text-body font-medium text-fg-1 underline underline-offset-[3px] hover:text-fg-2"
            >
              {extraAction.label}
            </button>
          )}
          <span className="text-caption text-fg-3">
            {scopeKind === "search"
              ? t("appMatrix.pop.scopeSearch", { count: scopeTotal })
              : scopeKind === "filter"
                ? t("appMatrix.pop.scopeFilter", { count: scopeTotal })
                : t("appMatrix.pop.scopeAll", { count: scopeTotal })}
          </span>
        </PopoverPrimitive.Content>
      </PopoverPrimitive.Portal>
    </PopoverPrimitive.Root>
  );
}

// ─── 搜索框 ─────────────────────────────────────────────────────────────
interface MatrixSearchProps {
  value: string;
  onValueChange: (value: string) => void;
  placeholder: string;
  ariaLabel: string;
  /** 有内容时读屏报「找到 N 个」 */
  status?: string;
  className?: string;
  onEnter?: () => void;
  inputId?: string;
}

export function MatrixSearch({
  value,
  onValueChange,
  placeholder,
  ariaLabel,
  status,
  className,
  onEnter,
  inputId,
}: MatrixSearchProps) {
  const { t } = useTranslation();
  return (
    <div role="search" className={cn("relative min-w-0", className)}>
      <Search
        aria-hidden="true"
        strokeWidth={1.5}
        className="pointer-events-none absolute left-2.5 top-2 h-4 w-4 text-fg-3"
      />
      <input
        id={inputId}
        type="text"
        value={value}
        onChange={(event) => onValueChange(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape" && value) {
            event.stopPropagation();
            event.preventDefault();
            onValueChange("");
          } else if (event.key === "Enter" && onEnter) {
            event.preventDefault();
            onEnter();
          }
        }}
        placeholder={placeholder}
        aria-label={ariaLabel}
        autoComplete="off"
        spellCheck={false}
        className="h-8 w-full rounded-[8px] border border-border-strong bg-surface pe-[34px] ps-[34px] text-body text-fg-1 outline-none placeholder:text-fg-3 focus-visible:ring-1 focus-visible:ring-ring"
      />
      {value && (
        <button
          type="button"
          onClick={() => onValueChange("")}
          aria-label={t("appMatrix.clearSearch")}
          title={t("appMatrix.clearSearch")}
          className="absolute right-1 top-1 flex h-6 w-6 items-center justify-center rounded-control text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1"
        >
          <X aria-hidden="true" className="h-3.5 w-3.5" strokeWidth={1.5} />
        </button>
      )}
      <span role="status" className="sr-only">
        {status}
      </span>
    </div>
  );
}

// ─── 小徽标 ─────────────────────────────────────────────────────────────
export function NeutralBadge({
  children,
  mono,
  className,
}: {
  children: React.ReactNode;
  mono?: boolean;
  className?: string;
}) {
  return (
    <span
      className={cn(
        "inline-flex h-[18px] shrink-0 items-center whitespace-nowrap rounded-full border border-border-strong px-1.5 text-badge font-medium text-fg-2",
        mono && "font-mono",
        className,
      )}
    >
      {children}
    </span>
  );
}

// ─── 可撤销的批量 toast ───────────────────────────────────────────────────
export function showUndoToast(
  text: string,
  undoLabel: string,
  onUndo?: () => void,
) {
  if (!onUndo) {
    toast.success(text, { closeButton: true });
    return;
  }
  toast.success(text, {
    closeButton: true,
    duration: 8000,
    action: { label: undoLabel, onClick: onUndo },
  });
}
