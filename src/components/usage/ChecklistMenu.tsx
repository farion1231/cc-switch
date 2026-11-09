import { useTranslation } from "react-i18next";
import type { ReactNode } from "react";
import { Check, ChevronDown } from "lucide-react";
import { fieldClass } from "@/components/ui/input";
import { HoverTip } from "@/components/ui/hover-tip";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";

export interface ChecklistItem {
  id: string;
  label: string;
  checked: boolean;
  /** 勾满上限时没勾的项灰掉 */
  disabled?: boolean;
}

/**
 * 下拉勾选清单：按钮上写当前选了什么，点开逐项勾；勾一项菜单不关，可以接着勾。
 * 有 onReset 时底部给「恢复默认」，有 note 时底部写一行说明。
 */
export function ChecklistMenu({
  label,
  summary,
  items,
  onToggle,
  onReset,
  note,
  className,
  quiet = false,
  icon,
}: {
  label: string;
  summary: string;
  items: ChecklistItem[];
  onToggle: (id: string) => void;
  onReset?: () => void;
  note?: string;
  /** 按钮宽度 */
  className?: string;
  /** 透明按钮样式（表格工具栏里），默认是表单字段样式 */
  quiet?: boolean;
  /** 给了就是纯图标按钮（配合 quiet）：名字放 aria-label 和悬停提示 */
  icon?: ReactNode;
}) {
  const { t } = useTranslation();
  return (
    <DropdownMenu>
      {icon ? (
        <HoverTip content={label}>
          <DropdownMenuTrigger
            aria-label={label}
            className={cn(
              "inline-flex h-7 w-7 items-center justify-center rounded-control text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring data-[state=open]:bg-subtle data-[state=open]:text-fg-1",
              className,
            )}
          >
            {icon}
          </DropdownMenuTrigger>
        </HoverTip>
      ) : (
        <DropdownMenuTrigger
          aria-label={summary === label ? label : `${label}: ${summary}`}
          className={cn(
            quiet
              ? "inline-flex h-7 items-center gap-1 rounded-control pe-1.5 ps-2 text-caption text-fg-2 hover:bg-subtle hover:text-fg-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring data-[state=open]:bg-subtle"
              : cn(
                  fieldClass,
                  "flex h-7 items-center justify-between gap-2 whitespace-nowrap text-start text-body data-[state=open]:border-ring data-[state=open]:ring-[3px] data-[state=open]:ring-ring/20",
                ),
            className,
          )}
        >
          <span className="truncate">{summary}</span>
          <ChevronDown
            aria-hidden="true"
            className="h-4 w-4 shrink-0 opacity-50"
          />
        </DropdownMenuTrigger>
      )}
      <DropdownMenuContent
        align={icon ? "end" : "start"}
        aria-label={label}
        className="min-w-[192px] max-w-[320px] rounded-panel border-border bg-surface p-1 shadow-v7-md"
      >
        {items.map((item) => (
          <DropdownMenuItem
            key={item.id}
            role="menuitemcheckbox"
            aria-checked={item.checked}
            disabled={item.disabled}
            className="h-[30px] gap-2 rounded-control px-2 text-body"
            onSelect={(event) => {
              event.preventDefault();
              onToggle(item.id);
            }}
          >
            <Check
              aria-hidden="true"
              strokeWidth={1.75}
              className={cn(
                "h-3.5 w-3.5 shrink-0",
                !item.checked && "invisible",
              )}
            />
            {item.label}
          </DropdownMenuItem>
        ))}
        {note && (
          <>
            <DropdownMenuSeparator />
            <p className="m-0 px-2 py-1 text-caption text-fg-3">{note}</p>
          </>
        )}
        {onReset && (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuItem
              className="h-[30px] gap-2 rounded-control px-2 text-body"
              onSelect={onReset}
            >
              <span aria-hidden="true" className="w-3.5 shrink-0" />
              {t("usage.logColumns.reset")}
            </DropdownMenuItem>
          </>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
