import React from "react";
import { useTranslation } from "react-i18next";
import { MoreHorizontal, Pencil } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { HoverTip } from "@/components/ui/hover-tip";
import type { Prompt } from "@/lib/api";
import { cn } from "@/lib/utils";
import { promptMenuContentClass, promptMenuItemClass } from "./PromptPageFrame";

interface PromptListItemProps {
  id: string;
  prompt: Prompt;
  /** 启用中的那条：常驻浅底 +「已启用」徽标，按钮变「停用」 */
  active: boolean;
  /** 第二行：说明（没有就用正文第一行）· 大小 · 更新时间 */
  detail: string;
  first: boolean;
  disabled?: boolean;
  onToggle: (id: string, enabled: boolean) => void;
  onEdit: (id: string) => void;
  onDelete: (id: string) => void;
  onCopyToApps?: (id: string) => void;
  onCopyContent?: (id: string) => void;
  /** 删除不能用时的原因（启用中的不能删） */
  deleteBlockedReason?: string;
}

/** 提示库的一行（高 60）：点行打开编辑抽屉，右边是启用 / 停用、编辑、⋯。 */
const PromptListItem: React.FC<PromptListItemProps> = ({
  id,
  prompt,
  active,
  detail,
  first,
  disabled = false,
  onToggle,
  onEdit,
  onDelete,
  onCopyToApps,
  onCopyContent,
  deleteBlockedReason,
}) => {
  const { t } = useTranslation();

  return (
    <li
      data-testid={`prompt-row-${id}`}
      onClick={(event) => {
        if (disabled) return;
        const target = event.target as HTMLElement;
        if (target.closest("button, a, input, label, [role='menu']")) return;
        onEdit(id);
      }}
      className={cn(
        "flex h-[60px] cursor-pointer items-center gap-3 pe-2 ps-4 transition-colors duration-150 hover:bg-subtle",
        !first && "border-t border-border",
        active && "bg-subtle",
      )}
    >
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="flex min-w-0 items-center gap-1.5">
          <span
            title={prompt.name}
            className="min-w-0 truncate text-body font-medium text-fg-1"
          >
            {prompt.name}
          </span>
          {active ? (
            <span className="inline-flex h-[18px] shrink-0 items-center whitespace-nowrap rounded-full bg-success-soft px-[7px] text-badge text-success-text">
              {t("prompts.enabled")}
            </span>
          ) : null}
        </div>
        <span
          title={detail}
          className="min-w-0 truncate text-caption text-fg-2"
        >
          {detail}
        </span>
      </div>

      <Button
        type="button"
        variant="neutral"
        size="compact"
        disabled={disabled}
        aria-label={t(active ? "prompts.disableAria" : "prompts.enableAria", {
          name: prompt.name,
        })}
        onClick={() => onToggle(id, !active)}
        className="min-w-14 shrink-0"
      >
        {t(active ? "prompts.disable" : "prompts.enable")}
      </Button>

      <div className="flex shrink-0 gap-1">
        <HoverTip content={t("common.edit")}>
          <Button
            type="button"
            variant="quiet"
            size="icon-compact"
            disabled={disabled}
            aria-label={t("prompts.editAria", { name: prompt.name })}
            onClick={() => onEdit(id)}
          >
            <Pencil className="h-[15px] w-[15px]" strokeWidth={1.5} />
          </Button>
        </HoverTip>
        <DropdownMenu modal={false}>
          <HoverTip content={t("common.more")}>
            <DropdownMenuTrigger asChild>
              <Button
                type="button"
                variant="quiet"
                size="icon-compact"
                disabled={disabled}
                aria-label={t("prompts.rowMoreActions", { name: prompt.name })}
              >
                <MoreHorizontal
                  className="h-[15px] w-[15px]"
                  strokeWidth={1.5}
                />
              </Button>
            </DropdownMenuTrigger>
          </HoverTip>
          <DropdownMenuContent
            align="end"
            className={cn(promptMenuContentClass, "w-[240px]")}
          >
            {onCopyToApps ? (
              <DropdownMenuItem
                className={promptMenuItemClass}
                onSelect={() => onCopyToApps(id)}
              >
                {t("prompts.copyToApps")}
              </DropdownMenuItem>
            ) : null}
            {onCopyContent ? (
              <DropdownMenuItem
                className={promptMenuItemClass}
                onSelect={() => onCopyContent(id)}
              >
                {t("prompts.copyContent")}
              </DropdownMenuItem>
            ) : null}
            {onCopyToApps || onCopyContent ? (
              <DropdownMenuSeparator className="mx-1.5 my-1 bg-border" />
            ) : null}
            <DropdownMenuItem
              disabled={Boolean(deleteBlockedReason)}
              onSelect={() => onDelete(id)}
              className={cn(
                promptMenuItemClass,
                "flex-col items-start gap-0",
                deleteBlockedReason
                  ? "text-fg-3"
                  : "text-danger-text focus:text-danger-text",
              )}
            >
              <span>{t("common.delete")}</span>
              {deleteBlockedReason ? (
                <span className="text-caption text-fg-2">
                  {deleteBlockedReason}
                </span>
              ) : null}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </li>
  );
};

export default PromptListItem;
