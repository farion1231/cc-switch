import { useTranslation } from "react-i18next";
import {
  ArrowDown,
  ArrowUp,
  ChevronDown,
  Loader2,
  MoreHorizontal,
  Pencil,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { DisabledReason } from "@/components/ui/help-tip";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import type { CardButton, CardPresentation, CardTone } from "./presentation";

const DOT: Record<CardTone | "muted", string> = {
  direct: "bg-direct",
  route: "bg-route",
  stack: "bg-stack",
  neutral: "bg-fg-2",
  muted: "bg-fg-3",
};

interface ProviderCardActionsProps {
  providerName: string;
  presentation: CardPresentation;
  onEdit: () => void;
  onDelete: () => void;
  onDuplicate?: () => void;
  onTest?: () => void;
  isTesting?: boolean;
  onConfigureUsage?: () => void;
  onOpenTerminal?: () => void;
}

/**
 * 卡片右侧（v7）：主操作位（状态文字或按钮）→ 上移 / 下移（故障转移队列）→ 编辑 → ⋯。
 * 次要操作都在 ⋯ 里：复制、检测连通、配置用量查询、打开终端、删除。
 */
export function ProviderCardActions({
  providerName,
  presentation,
  onEdit,
  onDelete,
  onDuplicate,
  onTest,
  isTesting,
  onConfigureUsage,
  onOpenTerminal,
}: ProviderCardActionsProps) {
  const { t } = useTranslation();
  const { status, buttons, move } = presentation;

  return (
    <div className="flex shrink-0 items-center gap-2">
      <div className="flex min-w-[92px] items-center justify-end gap-1.5">
        {status && (
          <span className="inline-flex items-center gap-1.5 whitespace-nowrap px-1 text-body font-medium text-fg-1">
            <span
              aria-hidden="true"
              className={cn("h-1.5 w-1.5 rounded-full", DOT[status.dot])}
            />
            {status.label}
          </span>
        )}
        {buttons.map((button) => (
          <PrimaryButton key={button.key} button={button} />
        ))}
      </div>

      {move && (
        <div className="flex items-center">
          <Button
            variant="quiet"
            size="icon-compact"
            aria-label={t("providerCard.action.moveUp", { name: providerName })}
            title={t("providerCard.action.moveUp", { name: providerName })}
            disabled={!move.onUp}
            onClick={move.onUp}
          >
            <ArrowUp className="h-4 w-4" strokeWidth={1.5} />
          </Button>
          <Button
            variant="quiet"
            size="icon-compact"
            aria-label={t("providerCard.action.moveDown", {
              name: providerName,
            })}
            title={t("providerCard.action.moveDown", { name: providerName })}
            disabled={!move.onDown}
            onClick={move.onDown}
          >
            <ArrowDown className="h-4 w-4" strokeWidth={1.5} />
          </Button>
        </div>
      )}

      <DisabledReason reason={presentation.editDisabledReason} align="end">
        <Button
          variant="quiet"
          size="icon-compact"
          aria-label={t("providerCard.action.edit", { name: providerName })}
          title={t("common.edit")}
          onClick={onEdit}
        >
          <Pencil className="h-4 w-4" strokeWidth={1.5} />
        </Button>
      </DisabledReason>

      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            variant="quiet"
            size="icon-compact"
            aria-label={t("providerCard.action.more", { name: providerName })}
            title={t("common.more")}
          >
            <MoreHorizontal className="h-4 w-4" strokeWidth={1.5} />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="min-w-[180px]">
          {onDuplicate && (
            <DropdownMenuItem onSelect={onDuplicate}>
              {t("provider.duplicate")}
            </DropdownMenuItem>
          )}
          {onTest && (
            <DropdownMenuItem disabled={isTesting} onSelect={onTest}>
              {isTesting && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
              {t("provider.connectivityCheck")}
            </DropdownMenuItem>
          )}
          {onConfigureUsage && (
            <DropdownMenuItem onSelect={onConfigureUsage}>
              {t("provider.configureUsage")}
            </DropdownMenuItem>
          )}
          {onOpenTerminal && (
            <DropdownMenuItem onSelect={onOpenTerminal}>
              {t("provider.openTerminal")}
            </DropdownMenuItem>
          )}
          {(onDuplicate || onTest || onConfigureUsage || onOpenTerminal) && (
            <DropdownMenuSeparator />
          )}
          <DropdownMenuItem
            disabled={Boolean(presentation.deleteDisabledReason)}
            onSelect={onDelete}
            className="flex-col items-start gap-0.5 text-danger-text focus:text-danger-text"
          >
            {t("common.delete")}
            {presentation.deleteDisabledReason && (
              <span className="text-caption text-fg-3">
                {presentation.deleteDisabledReason}
              </span>
            )}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}

function PrimaryButton({ button }: { button: CardButton }) {
  if (button.menu && !button.disabledReason) {
    return (
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="neutral" size="compact">
            {button.label}
            <ChevronDown className="h-3.5 w-3.5 opacity-70" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent
          align="end"
          className="max-h-72 min-w-64 overflow-y-auto"
        >
          <DropdownMenuLabel>{button.menu.title}</DropdownMenuLabel>
          {button.menu.options.map((option) => (
            <DropdownMenuItem
              key={option.key}
              onSelect={option.onSelect}
              className="flex min-w-0 flex-col items-start gap-0.5"
            >
              <span className="max-w-72 truncate">{option.label}</span>
              {option.detail && (
                <span className="max-w-72 truncate font-mono text-caption text-fg-3">
                  {option.detail}
                </span>
              )}
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    );
  }
  return (
    <DisabledReason reason={button.disabledReason} align="end">
      <Button variant="neutral" size="compact" onClick={button.onClick}>
        {button.label}
      </Button>
    </DisabledReason>
  );
}
