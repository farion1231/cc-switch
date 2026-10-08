import * as React from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle, Check } from "lucide-react";
import { toast } from "@/lib/toast";
import * as PopoverPrimitive from "@radix-ui/react-popover";
import type { AppId } from "@/lib/api/types";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import { HoverTip } from "@/components/ui/hover-tip";
import { AppGlyph, APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { SearchField } from "@/components/ui/search-field";
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

// ─── 列高亮（悬停 / 键盘焦点落在某一列时，列头跟着亮并显示应用名） ─────────────
/**
 * 只有列头订阅当前列；单元格只拿稳定的 setter，悬停时整张表不会重渲染。
 */
const MatrixColumnSetterContext = React.createContext<
  (app: AppId, active: boolean) => void
>(() => {});
const MatrixActiveColumnContext = React.createContext<AppId | null>(null);

export function MatrixColumnHighlight({
  children,
}: {
  children: React.ReactNode;
}) {
  const [active, setActive] = React.useState<AppId | null>(null);
  const setColumn = React.useCallback((app: AppId, on: boolean) => {
    setActive((prev) => (on ? app : prev === app ? null : prev));
  }, []);
  return (
    <MatrixColumnSetterContext.Provider value={setColumn}>
      <MatrixActiveColumnContext.Provider value={active}>
        {children}
      </MatrixActiveColumnContext.Provider>
    </MatrixColumnSetterContext.Provider>
  );
}

/** 只认键盘焦点：鼠标点开批量弹层再关上时，焦点回到列头不该冒出名字 */
function isKeyboardFocus(element: Element) {
  try {
    return element.matches(":focus-visible");
  } catch {
    return true;
  }
}

function useColumnTracking(app: AppId | undefined) {
  const setColumn = React.useContext(MatrixColumnSetterContext);
  return React.useMemo(() => {
    if (!app) return {};
    return {
      onMouseEnter: () => setColumn(app, true),
      onMouseLeave: () => setColumn(app, false),
      onFocus: (event: React.FocusEvent<HTMLElement>) => {
        if (isKeyboardFocus(event.target)) setColumn(app, true);
      },
      onBlur: () => setColumn(app, false),
    };
  }, [app, setColumn]);
}

// ─── 单元格 ─────────────────────────────────────────────────────────────
export type MatrixCellState = "on" | "off" | "fail";

interface MatrixCellProps {
  state: MatrixCellState;
  /** 所在列：悬停 / 聚焦时高亮这一列的列头 */
  app?: AppId;
  /** 「serena · Codex：已启用」 */
  label: string;
  onClick: () => void;
  disabled?: boolean;
}

/**
 * 勾选框：开、关、写入失败三种状态是同一个 16px 圆角方框、同一个 28px 点击区，
 * 只换填充和里面的符号（关 = 淡描边空框，开 = 操作色实心 + 勾，失败 = 警告描边 + ⚠）。
 */
export function MatrixCell({
  state,
  app,
  label,
  onClick,
  disabled,
}: MatrixCellProps) {
  const tracking = useColumnTracking(app);
  return (
    <span className="flex w-9 shrink-0 justify-center" {...tracking}>
      <HoverTip content={label}>
        <button
          type="button"
          aria-pressed={state === "on"}
          aria-label={label}
          disabled={disabled}
          data-state={state}
          onClick={(event) => {
            event.stopPropagation();
            onClick();
          }}
          className="group/cell flex h-7 w-7 items-center justify-center rounded-control outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-45"
        >
          <span
            aria-hidden="true"
            className={cn(
              // 和 ui-checkbox 同一个外观：1.5px 描边、白底；开 = 主题色底 + 勾（勾从中心放大出来）
              "flex h-4 w-4 items-center justify-center rounded-[4px] border-[1.5px] transition-colors duration-150",
              state === "on" &&
                "border-action bg-action text-action-fg group-hover/cell:border-action-hover group-hover/cell:bg-action-hover",
              state === "off" &&
                "border-border-strong bg-surface group-hover/cell:border-fg-3",
              state === "fail" &&
                "border-warning bg-warning-soft text-warning-text",
            )}
          >
            {state === "fail" ? (
              <AlertTriangle className="h-3 w-3" strokeWidth={2.25} />
            ) : (
              <Check
                className={cn(
                  "h-3 w-3 transition-transform duration-[120ms] ease-out motion-reduce:transition-none",
                  state === "on" ? "scale-100" : "scale-0",
                )}
                strokeWidth={3}
              />
            )}
          </span>
        </button>
      </HoverTip>
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

  const tracking = useColumnTracking(app);
  const activeColumn = React.useContext(MatrixActiveColumnContext);
  // 悬停 / 键盘聚焦在这一列（列头或任一单元格）时亮起；弹层开着时名字已在弹层标题里
  const highlighted = activeColumn === app || open;
  const showName = activeColumn === app && !open;

  const run = (action: () => void) => {
    setOpen(false);
    action();
  };

  return (
    <PopoverPrimitive.Root open={open} onOpenChange={setOpen}>
      <span
        className="relative flex w-9 shrink-0 justify-center"
        data-column={app}
        {...tracking}
      >
        <HoverTip content={title} side="top">
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
              data-highlighted={highlighted ? "" : undefined}
              className={cn(
                "flex h-10 w-[34px] flex-col items-center justify-center gap-0.5 rounded-control text-fg-2 outline-none transition-colors duration-150 focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-60",
                highlighted && "bg-selected text-fg-1",
              )}
            >
              <AppGlyph
                app={app}
                size={16}
                badgeClassName={highlighted ? "bg-selected" : "bg-subtle"}
              />
              <span
                className={cn(
                  "text-badge tabular-nums",
                  highlighted ? "font-semibold" : "font-medium",
                )}
              >
                {enabledCount}
              </span>
            </button>
          </PopoverPrimitive.Trigger>
        </HoverTip>
        {showName && (
          // 名字已在列头的无障碍名称里，这里只给眼睛看
          <span
            aria-hidden="true"
            data-testid="matrix-column-name"
            className="pointer-events-none absolute left-1/2 top-full z-20 mt-0.5 -translate-x-1/2 whitespace-nowrap rounded-control bg-inverse px-2 py-0.5 text-caption font-medium text-inverse-fg shadow-v7-sm"
          >
            {name}
          </span>
        )}
      </span>
      <PopoverPrimitive.Portal>
        <PopoverPrimitive.Content
          align="end"
          sideOffset={4}
          collisionPadding={8}
          className="z-[110] flex w-[260px] flex-col gap-2.5 rounded-panel border border-border bg-surface px-3.5 pb-3.5 pt-3 text-fg-1 shadow-v7-md outline-none data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2"
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
    <>
      <SearchField
        id={inputId}
        value={value}
        onValueChange={onValueChange}
        clearLabel={t("appMatrix.clearSearch")}
        placeholder={placeholder}
        aria-label={ariaLabel}
        autoComplete="off"
        containerClassName={className}
        onKeyDown={(event) => {
          if (event.key === "Enter" && onEnter) {
            event.preventDefault();
            onEnter();
          }
        }}
      />
      <span role="status" className="sr-only">
        {status}
      </span>
    </>
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
