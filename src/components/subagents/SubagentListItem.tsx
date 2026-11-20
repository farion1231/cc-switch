import { useTranslation } from "react-i18next";
import { AlertTriangle, FileX, MoreHorizontal } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { HoverTip } from "@/components/ui/hover-tip";
import { Switch } from "@/components/ui/switch";
import {
  promptMenuContentClass,
  promptMenuItemClass,
} from "@/components/prompts/PromptPageFrame";
import type { Subagent } from "@/lib/api/subagents";
import { cn } from "@/lib/utils";
import type { SubagentFields } from "./frontmatter";
import { colorHexOf } from "./subagentUtils";

export const subagentBadgeClass =
  "inline-flex h-[18px] shrink-0 items-center gap-1 whitespace-nowrap rounded-full px-[7px] text-badge";

interface SubagentListItemProps {
  subagent: Subagent;
  fields: SubagentFields;
  first: boolean;
  disabled?: boolean;
  onEdit: () => void;
  onToggle: (enabled: boolean) => void;
  onResolveDrift: () => void;
  onRewrite: () => void;
  onDuplicate: () => void;
  onExport: () => void;
  onCopyPath: () => void;
  onDelete: () => void;
}

/** 模型徽标：没写 model 或写了 inherit 都是「继承」 */
export function ModelBadge({ model }: { model: string | undefined }) {
  const { t } = useTranslation();
  const inherit = !model || model === "inherit";
  return (
    <span
      title={model}
      className={cn(
        subagentBadgeClass,
        "max-w-[180px] bg-subtle text-fg-2",
        !inherit && "font-mono",
      )}
    >
      <span className="truncate">
        {inherit ? t("subagents.model.inheritShort") : model}
      </span>
    </span>
  );
}

/** 工具摘要：没写 tools 是「全部工具」，否则前 3 个 + 剩余个数 */
export function toolsSummary(
  t: (key: string, options?: Record<string, unknown>) => string,
  tools: string[] | undefined,
): string {
  if (tools === undefined) return t("subagents.tools.all");
  if (tools.length === 0) return t("subagents.tools.none");
  const head = tools.slice(0, 3).join(", ");
  return tools.length > 3
    ? t("subagents.tools.summaryMore", { head, count: tools.length - 3 })
    : head;
}

/** 已管理的一行：点行打开编辑器；右侧是状态处理按钮、启用开关、⋯ */
export function SubagentListItem({
  subagent,
  fields,
  first,
  disabled = false,
  onEdit,
  onToggle,
  onResolveDrift,
  onRewrite,
  onDuplicate,
  onExport,
  onCopyPath,
  onDelete,
}: SubagentListItemProps) {
  const { t } = useTranslation();
  const name = fields.name || subagent.name;
  const description = fields.description ?? subagent.description;
  const color = colorHexOf(fields.color);
  const tools = toolsSummary(t, fields.tools);

  return (
    <li
      data-testid={`subagent-row-${subagent.id}`}
      onClick={(event) => {
        if (disabled) return;
        const target = event.target as HTMLElement;
        if (target.closest("button, a, input, label, [role='menu']")) return;
        onEdit();
      }}
      className={cn(
        "flex cursor-pointer items-center gap-3 py-3 pe-2 ps-4 transition-colors duration-150 hover:bg-subtle",
        !first && "border-t border-border",
      )}
    >
      <span
        aria-hidden="true"
        className={cn(
          "h-2.5 w-2.5 shrink-0 self-start rounded-full",
          !color && "border border-border-strong",
        )}
        style={{ backgroundColor: color, marginTop: 5 }}
      />
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <div className="flex min-w-0 items-center gap-1.5">
          <span
            title={name}
            className="min-w-0 truncate font-mono text-body font-medium text-fg-1"
          >
            {name}
          </span>
          {subagent.status === "drifted" ? (
            <span
              className={cn(
                subagentBadgeClass,
                "bg-warning-soft text-warning-text",
              )}
            >
              <AlertTriangle
                aria-hidden="true"
                className="h-3 w-3"
                strokeWidth={1.5}
              />
              {t("subagents.status.drifted")}
            </span>
          ) : subagent.status === "missing" ? (
            <span
              className={cn(
                subagentBadgeClass,
                "bg-danger-soft text-danger-text",
              )}
            >
              <FileX aria-hidden="true" className="h-3 w-3" strokeWidth={1.5} />
              {t("subagents.status.missing")}
            </span>
          ) : null}
        </div>
        {description ? (
          <p
            title={description}
            className="m-0 line-clamp-2 whitespace-pre-line text-caption text-fg-2"
          >
            {description}
          </p>
        ) : null}
        <div className="mt-0.5 flex min-w-0 items-center gap-2">
          <ModelBadge model={fields.model} />
          <span
            title={fields.tools?.join(", ")}
            className="min-w-0 truncate font-mono text-caption text-fg-3"
          >
            {tools}
          </span>
        </div>
      </div>

      {subagent.status === "drifted" ? (
        <Button
          type="button"
          variant="neutral"
          size="compact"
          disabled={disabled}
          onClick={onResolveDrift}
          className="shrink-0"
        >
          {t("subagents.actions.resolve")}
        </Button>
      ) : subagent.status === "missing" ? (
        <Button
          type="button"
          variant="neutral"
          size="compact"
          disabled={disabled}
          onClick={onRewrite}
          className="shrink-0"
        >
          {t("subagents.actions.rewrite")}
        </Button>
      ) : null}

      <HoverTip
        content={
          subagent.enabled
            ? t("subagents.actions.disableTip")
            : t("subagents.actions.enableTip")
        }
      >
        <span className="flex shrink-0 items-center">
          <Switch
            checked={subagent.enabled}
            disabled={disabled}
            onCheckedChange={onToggle}
            aria-label={t(
              subagent.enabled
                ? "subagents.actions.disableAria"
                : "subagents.actions.enableAria",
              { name },
            )}
          />
        </span>
      </HoverTip>

      <DropdownMenu modal={false}>
        <HoverTip content={t("common.more")}>
          <DropdownMenuTrigger asChild>
            <Button
              type="button"
              variant="quiet"
              size="icon-compact"
              disabled={disabled}
              aria-label={t("subagents.actions.rowMore", { name })}
            >
              <MoreHorizontal className="h-[15px] w-[15px]" strokeWidth={1.5} />
            </Button>
          </DropdownMenuTrigger>
        </HoverTip>
        <DropdownMenuContent
          align="end"
          className={cn(promptMenuContentClass, "w-[240px]")}
        >
          <DropdownMenuItem className={promptMenuItemClass} onSelect={onEdit}>
            {t("common.edit")}
          </DropdownMenuItem>
          <DropdownMenuItem
            className={promptMenuItemClass}
            onSelect={onDuplicate}
          >
            {t("subagents.actions.duplicate")}
          </DropdownMenuItem>
          <DropdownMenuItem className={promptMenuItemClass} onSelect={onExport}>
            {t("subagents.actions.export")}
          </DropdownMenuItem>
          <DropdownMenuItem
            className={promptMenuItemClass}
            onSelect={onCopyPath}
          >
            {t("subagents.actions.copyPath")}
          </DropdownMenuItem>
          <DropdownMenuSeparator className="mx-1.5 my-1 bg-border" />
          <DropdownMenuItem
            onSelect={onDelete}
            className={cn(
              promptMenuItemClass,
              "text-danger-text focus:text-danger-text",
            )}
          >
            {t("common.delete")}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </li>
  );
}
