import type * as React from "react";
import { useTranslation } from "react-i18next";
import {
  ArrowDown,
  ArrowUp,
  ChevronRight,
  CircleDot,
  ListMinus,
  ListPlus,
  Loader2,
  Minus,
  MoreHorizontal,
  Pencil,
  Play,
  Plug,
  Plus,
  Power,
  PowerOff,
  Route,
  SquareTerminal,
  type LucideIcon,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { DisabledReason } from "@/components/ui/help-tip";
import { HoverTip } from "@/components/ui/hover-tip";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuPortal,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import type {
  CardButton,
  CardMenuItem,
  CardPresentation,
  CardTone,
} from "./presentation";

const DOT: Record<CardTone | "muted", string> = {
  direct: "bg-direct",
  route: "bg-route",
  stack: "bg-stack",
  muted: "bg-fg-3",
};

/** 主操作位只放图标，名字由 HoverTip 悬停即显（key 见 presentation.ts）。 */
const BUTTON_ICON: Record<string, LucideIcon> = {
  switch: Play,
  exitAndUse: Plug,
  routeHere: Route,
  queueAdd: ListPlus,
  queueRemove: ListMinus,
  add: Plus,
  remove: Minus,
  enable: Power,
  disable: PowerOff,
  openTerminal: SquareTerminal,
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
  /** 这张卡打开的终端是否与 CC Switch 的切换隔离（见 terminalIsIsolated），决定提示里带不带「环境隔离」 */
  openTerminalIsolated?: boolean;
}

/**
 * 卡片右侧（v7）：主操作位（状态文字或按钮）→ 上移 / 下移（故障转移队列）→ 打开终端（Claude Code）
 * → 编辑 → ⋯。次要操作都在 ⋯ 里：跟当前模式有关的（聚合页、OpenClaw / OpenCode 的「设为默认」）在
 * 最前，然后是复制、检测连通、配置用量查询、删除。
 * 按钮一律纯图标 + HoverTip；有禁用原因时改由 DisabledReason 的说明卡报原因。
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
  openTerminalIsolated,
}: ProviderCardActionsProps) {
  const { t } = useTranslation();
  const { status, buttons, move, menuItems = [] } = presentation;

  return (
    <div className="flex shrink-0 items-center gap-2">
      {/* 主操作位定宽：图标按钮和「● 使用中」这类状态文字宽度不同，不定宽的话
          切换后左边的用量会被推开、跟其他卡片对不齐。按最长的常见状态留宽，靠右对齐 */}
      <div className="flex min-w-20 items-center justify-end gap-1">
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
          <HoverTip
            content={t("providerCard.action.moveUp", { name: providerName })}
          >
            <Button
              variant="quiet"
              size="icon-compact"
              aria-label={t("providerCard.action.moveUp", {
                name: providerName,
              })}
              disabled={!move.onUp}
              onClick={move.onUp}
            >
              <ArrowUp className="h-4 w-4" strokeWidth={1.5} />
            </Button>
          </HoverTip>
          <HoverTip
            content={t("providerCard.action.moveDown", { name: providerName })}
          >
            <Button
              variant="quiet"
              size="icon-compact"
              aria-label={t("providerCard.action.moveDown", {
                name: providerName,
              })}
              disabled={!move.onDown}
              onClick={move.onDown}
            >
              <ArrowDown className="h-4 w-4" strokeWidth={1.5} />
            </Button>
          </HoverTip>
        </div>
      )}

      {onOpenTerminal && (
        <HoverTip
          content={t(
            openTerminalIsolated
              ? "provider.openTerminalIsolated"
              : "provider.openTerminal",
          )}
        >
          <Button
            variant="quiet"
            size="icon-compact"
            aria-label={t("providerCard.action.openTerminal", {
              name: providerName,
            })}
            onClick={onOpenTerminal}
          >
            <SquareTerminal className="h-4 w-4" strokeWidth={1.5} />
          </Button>
        </HoverTip>
      )}

      <IconAction
        tip={t("common.edit")}
        disabledReason={presentation.editDisabledReason}
      >
        <Button
          variant="quiet"
          size="icon-compact"
          aria-label={t("providerCard.action.edit", { name: providerName })}
          onClick={onEdit}
        >
          <Pencil className="h-4 w-4" strokeWidth={1.5} />
        </Button>
      </IconAction>

      <DropdownMenu>
        <HoverTip content={t("common.more")}>
          <DropdownMenuTrigger asChild>
            <Button
              variant="quiet"
              size="icon-compact"
              aria-label={t("providerCard.action.more", {
                name: providerName,
              })}
            >
              <MoreHorizontal className="h-4 w-4" strokeWidth={1.5} />
            </Button>
          </DropdownMenuTrigger>
        </HoverTip>
        <DropdownMenuContent align="end" className="min-w-[180px]">
          {menuItems.map((item) => (
            <ModeMenuItem key={item.key} item={item} />
          ))}
          {menuItems.length > 0 && <DropdownMenuSeparator />}
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
          {(onDuplicate || onTest || onConfigureUsage) && (
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

/** 有禁用原因时只挂说明卡（报原因），否则挂 HoverTip（报名字），两者不叠。 */
function IconAction({
  tip,
  disabledReason,
  children,
}: {
  tip: string;
  disabledReason?: string;
  children: React.ReactElement;
}) {
  if (disabledReason) {
    return (
      <DisabledReason reason={disabledReason} align="end">
        {children}
      </DisabledReason>
    );
  }
  return <HoverTip content={tip}>{children}</HoverTip>;
}

/** 「更多」菜单里跟模式有关的一项；带子菜单时（选默认模型）展开选，否则直接点。 */
function ModeMenuItem({ item }: { item: CardMenuItem }) {
  if (item.submenu && !item.disabledReason) {
    return (
      <DropdownMenuSub>
        <DropdownMenuSubTrigger>
          {item.label}
          <ChevronRight
            aria-hidden="true"
            className="ml-auto h-3.5 w-3.5 text-fg-3"
          />
        </DropdownMenuSubTrigger>
        <DropdownMenuPortal>
          <DropdownMenuSubContent className="max-h-72 min-w-64 overflow-y-auto">
            <DropdownMenuLabel>{item.submenu.title}</DropdownMenuLabel>
            {item.submenu.options.map((option) => (
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
          </DropdownMenuSubContent>
        </DropdownMenuPortal>
      </DropdownMenuSub>
    );
  }
  return (
    <DropdownMenuItem
      disabled={Boolean(item.disabledReason)}
      onSelect={item.onSelect}
      className="flex-col items-start gap-0.5"
    >
      {item.label}
      {item.disabledReason && (
        <span className="max-w-64 text-caption text-fg-3">
          {item.disabledReason}
        </span>
      )}
    </DropdownMenuItem>
  );
}

function PrimaryButton({ button }: { button: CardButton }) {
  const Icon = BUTTON_ICON[button.key] ?? CircleDot;
  const icon = <Icon className="h-4 w-4" strokeWidth={1.5} />;
  return (
    <IconAction tip={button.label} disabledReason={button.disabledReason}>
      <Button
        variant="quiet"
        size="icon-compact"
        aria-label={button.label}
        onClick={button.onClick}
      >
        {icon}
      </Button>
    </IconAction>
  );
}
