import React from "react";
import { useTranslation } from "react-i18next";
import {
  ChevronDown,
  FolderCog,
  Loader2,
  MoreHorizontal,
  Palette,
  Pencil,
  RefreshCw,
  Trash2,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { APP_ICON_MAP } from "@/config/appConfig";
import type {
  InstalledSkill,
  SkillGroup,
  SkillGroupColor,
} from "@/lib/api/skills";
import { SKILL_GROUP_COLORS } from "@/lib/api/skills";
import type { AppId } from "@/lib/api/types";
import { cn } from "@/lib/utils";

export const SKILL_GROUP_COLOR_STYLES: Record<
  SkillGroupColor,
  { dot: string; border: string; wash: string; text: string }
> = {
  blue: {
    dot: "bg-blue-500",
    border: "border-l-blue-500",
    wash: "bg-blue-500/[0.045] dark:bg-blue-400/[0.055]",
    text: "text-blue-600 dark:text-blue-400",
  },
  violet: {
    dot: "bg-violet-500",
    border: "border-l-violet-500",
    wash: "bg-violet-500/[0.045] dark:bg-violet-400/[0.055]",
    text: "text-violet-600 dark:text-violet-400",
  },
  emerald: {
    dot: "bg-emerald-500",
    border: "border-l-emerald-500",
    wash: "bg-emerald-500/[0.045] dark:bg-emerald-400/[0.055]",
    text: "text-emerald-600 dark:text-emerald-400",
  },
  amber: {
    dot: "bg-amber-500",
    border: "border-l-amber-500",
    wash: "bg-amber-500/[0.045] dark:bg-amber-400/[0.055]",
    text: "text-amber-600 dark:text-amber-400",
  },
  rose: {
    dot: "bg-rose-500",
    border: "border-l-rose-500",
    wash: "bg-rose-500/[0.045] dark:bg-rose-400/[0.055]",
    text: "text-rose-600 dark:text-rose-400",
  },
  cyan: {
    dot: "bg-cyan-500",
    border: "border-l-cyan-500",
    wash: "bg-cyan-500/[0.045] dark:bg-cyan-400/[0.055]",
    text: "text-cyan-600 dark:text-cyan-400",
  },
  slate: {
    dot: "bg-slate-400",
    border: "border-l-slate-400",
    wash: "bg-slate-500/[0.035] dark:bg-slate-400/[0.045]",
    text: "text-slate-600 dark:text-slate-400",
  },
};

interface SkillGroupSectionProps {
  group?: SkillGroup;
  skills: InstalledSkill[];
  appIds: AppId[];
  open: boolean;
  forceOpen?: boolean;
  disabled?: boolean;
  pendingApp?: AppId | null;
  updateCount: number;
  updating?: boolean;
  onOpenChange: (open: boolean) => void;
  onToggleApp: (app: AppId, enabled: boolean) => void;
  onUpdateAll: () => void;
  onManageMembers: () => void;
  onRename?: (name: string) => Promise<void>;
  onChangeColor?: (color: SkillGroupColor) => void;
  onDelete?: () => void;
  children: React.ReactNode;
}

export function SkillGroupSection({
  group,
  skills,
  appIds,
  open,
  forceOpen = false,
  disabled = false,
  pendingApp,
  updateCount,
  updating = false,
  onOpenChange,
  onToggleApp,
  onUpdateAll,
  onManageMembers,
  onRename,
  onChangeColor,
  onDelete,
  children,
}: SkillGroupSectionProps) {
  const { t } = useTranslation();
  const [editing, setEditing] = React.useState(false);
  const [draftName, setDraftName] = React.useState(group?.name ?? "");
  const [nameError, setNameError] = React.useState<string>();
  const inputRef = React.useRef<HTMLInputElement>(null);
  const color = group?.color ?? "slate";
  const colorStyle = SKILL_GROUP_COLOR_STYLES[color];
  const effectiveOpen = forceOpen || open;
  const label = group?.name ?? t("skills.groups.ungrouped");

  React.useEffect(() => setDraftName(group?.name ?? ""), [group?.name]);
  React.useEffect(() => {
    if (editing) inputRef.current?.focus();
  }, [editing]);

  const saveName = async () => {
    if (!group || !onRename) return;
    const normalized = draftName.trim();
    const length = Array.from(normalized).length;
    if (length < 1 || length > 50) {
      setNameError(t("skills.groups.nameLengthError"));
      return;
    }
    try {
      await onRename(normalized);
      setNameError(undefined);
      setEditing(false);
    } catch {
      setNameError(t("skills.groups.updateMetadataFailed"));
    }
  };

  return (
    <Collapsible
      open={effectiveOpen}
      onOpenChange={(nextOpen) => !forceOpen && onOpenChange(nextOpen)}
      className={cn(
        "overflow-hidden rounded-xl border border-border-default border-l-[3px] bg-background",
        colorStyle.border,
      )}
    >
      <div
        className={cn(
          "flex min-h-12 items-center gap-2 border-b border-border-default/70 px-3 py-2",
          colorStyle.wash,
          !effectiveOpen && "border-b-0",
        )}
      >
        <CollapsibleTrigger asChild>
          <button
            type="button"
            className="flex min-w-0 items-center gap-2 rounded-md text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            aria-label={
              effectiveOpen
                ? t("skills.groups.collapse", { name: label })
                : t("skills.groups.expand", { name: label })
            }
          >
            <ChevronDown
              size={15}
              className={cn(
                "shrink-0 text-muted-foreground transition-transform duration-200",
                !effectiveOpen && "-rotate-90",
              )}
            />
            <span
              className={cn(
                "h-2.5 w-2.5 shrink-0 rounded-full",
                colorStyle.dot,
              )}
            />
          </button>
        </CollapsibleTrigger>

        <div className="min-w-0 flex-1">
          {editing && group ? (
            <div className="flex max-w-xs items-center gap-2">
              <ImeSafeInput
                ref={inputRef}
                value={draftName}
                onValueChange={(value) => {
                  setDraftName(value);
                  setNameError(undefined);
                }}
                className="h-7 bg-background/80 text-sm font-semibold"
                maxLength={50}
                aria-invalid={Boolean(nameError)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && !event.nativeEvent.isComposing) {
                    event.preventDefault();
                    void saveName();
                  }
                  if (event.key === "Escape") {
                    setDraftName(group.name);
                    setNameError(undefined);
                    setEditing(false);
                  }
                }}
                onBlur={() => void saveName()}
              />
              {nameError && (
                <span
                  className="truncate text-[11px] text-destructive"
                  title={nameError}
                >
                  {nameError}
                </span>
              )}
            </div>
          ) : (
            <CollapsibleTrigger asChild>
              <button
                type="button"
                className="flex min-w-0 items-baseline gap-2 rounded-md text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              >
                <span className="truncate text-sm font-semibold text-foreground">
                  {label}
                </span>
                <span className="shrink-0 text-[11px] tabular-nums text-muted-foreground">
                  {skills.length}
                </span>
              </button>
            </CollapsibleTrigger>
          )}
        </div>

        <div className="no-scrollbar flex max-w-[50%] shrink-0 items-center gap-1 overflow-x-auto">
          {appIds.map((app) => {
            const enabledCount = skills.filter((skill) =>
              Boolean(skill.apps[app]),
            ).length;
            const allEnabled =
              skills.length > 0 && enabledCount === skills.length;
            const partial = enabledCount > 0 && !allEnabled;
            const appPending = pendingApp === app;
            const actionLabel = allEnabled
              ? t("skills.groups.disableForGroup", {
                  app: APP_ICON_MAP[app].label,
                  name: label,
                  count: skills.length,
                })
              : t("skills.groups.enableForGroup", {
                  app: APP_ICON_MAP[app].label,
                  name: label,
                  count: skills.length,
                });
            return (
              <Tooltip key={app}>
                <TooltipTrigger asChild>
                  <button
                    type="button"
                    role="checkbox"
                    aria-checked={partial ? "mixed" : allEnabled}
                    aria-label={actionLabel}
                    title={actionLabel}
                    disabled={disabled || skills.length === 0}
                    onClick={() => onToggleApp(app, !allEnabled)}
                    className={cn(
                      "relative flex h-7 min-w-7 items-center justify-center rounded-lg px-1.5 transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-45",
                      allEnabled
                        ? APP_ICON_MAP[app].activeClass
                        : partial
                          ? "bg-muted text-foreground ring-1 ring-border-active"
                          : "text-muted-foreground/45 hover:bg-muted hover:text-muted-foreground",
                    )}
                  >
                    {appPending ? (
                      <Loader2 size={13} className="animate-spin" />
                    ) : (
                      APP_ICON_MAP[app].icon
                    )}
                    {partial && (
                      <span className="absolute bottom-0.5 right-0.5 h-1.5 w-1.5 rounded-full bg-current" />
                    )}
                  </button>
                </TooltipTrigger>
                <TooltipContent side="bottom">{actionLabel}</TooltipContent>
              </Tooltip>
            );
          })}
        </div>

        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="h-7 w-7 shrink-0"
              disabled={disabled}
              aria-label={t("skills.groups.actions", { name: label })}
            >
              <MoreHorizontal size={15} />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="min-w-52">
            <DropdownMenuItem onSelect={onManageMembers}>
              <FolderCog size={14} />
              {group
                ? t("skills.groups.manageMembers")
                : t("skills.groups.organizeUngrouped")}
            </DropdownMenuItem>
            <DropdownMenuItem
              disabled={updateCount === 0 || updating}
              onSelect={onUpdateAll}
            >
              {updating ? (
                <Loader2 size={14} className="animate-spin" />
              ) : (
                <RefreshCw size={14} />
              )}
              {t("skills.groups.updateGroup", { count: updateCount })}
            </DropdownMenuItem>
            {group && (
              <>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  onSelect={() => {
                    setDraftName(group.name);
                    setNameError(undefined);
                    setEditing(true);
                  }}
                >
                  <Pencil size={14} />
                  {t("skills.groups.rename")}
                </DropdownMenuItem>
                <DropdownMenuSub>
                  <DropdownMenuSubTrigger>
                    <Palette size={14} />
                    {t("skills.groups.changeColor")}
                  </DropdownMenuSubTrigger>
                  <DropdownMenuSubContent className="min-w-40">
                    <DropdownMenuRadioGroup
                      value={group.color}
                      onValueChange={(value) =>
                        onChangeColor?.(value as SkillGroupColor)
                      }
                    >
                      {SKILL_GROUP_COLORS.map((paletteColor) => (
                        <DropdownMenuRadioItem
                          key={paletteColor}
                          value={paletteColor}
                        >
                          <span
                            className={cn(
                              "h-2.5 w-2.5 rounded-full",
                              SKILL_GROUP_COLOR_STYLES[paletteColor].dot,
                            )}
                          />
                          {t(`skills.groups.colors.${paletteColor}`)}
                        </DropdownMenuRadioItem>
                      ))}
                    </DropdownMenuRadioGroup>
                  </DropdownMenuSubContent>
                </DropdownMenuSub>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  className="text-destructive focus:text-destructive"
                  onSelect={onDelete}
                >
                  <Trash2 size={14} />
                  {t("skills.groups.delete")}
                </DropdownMenuItem>
              </>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      <CollapsibleContent>{children}</CollapsibleContent>
    </Collapsible>
  );
}
