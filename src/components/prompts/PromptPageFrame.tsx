import { useState, type ReactNode } from "react";
import { useQueries } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  BookOpen,
  Check,
  ChevronDown,
  MoreHorizontal,
  Plus,
} from "lucide-react";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { AppGlyph, APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { HoverTip } from "@/components/ui/hover-tip";
import { promptsApi, type AppId } from "@/lib/api";
import { promptKeys } from "@/lib/query/prompts";
import { SearchField } from "@/components/ui/search-field";
import { cn } from "@/lib/utils";
import { renderSegs } from "./promptUtils";

export const promptMenuContentClass =
  "min-w-[200px] rounded-panel border border-border bg-surface p-1 text-fg-1 shadow-v7-md";
export const promptMenuItemClass =
  "min-h-8 rounded-control px-2.5 py-1.5 text-body text-fg-1 focus:bg-subtle focus:text-fg-1 data-[disabled]:opacity-100";

export interface PromptMoreItem {
  key: string;
  label: ReactNode;
  onSelect: () => void;
  /** 不能点时写原因（菜单里的原因写成看得见的第二行） */
  reason?: string;
}

interface PromptPageFrameProps {
  app: AppId;
  apps: AppId[];
  onAppChange: (app: AppId) => void;
  /** 有未保存的抽屉 / 正在写入时不能换应用 */
  appSwitchDisabled?: boolean;
  /** 当前应用的条数（列表里现成的，不另外查） */
  count: number;
  help: { title: string; text: string };
  primary?: { label: string; onClick: () => void; disabled?: boolean } | null;
  moreItems: PromptMoreItem[];
  /** 应用下拉右边的附加控件（Pi 的三段分段） */
  segments?: ReactNode;
  target?: { label: string; path: string; meta?: string } | null;
  search?: {
    value: string;
    onChange: (value: string) => void;
    placeholder: string;
    ariaLabel: string;
    /** 搜索时读给屏幕阅读器的结果数 */
    resultCount?: number;
    width?: number;
  } | null;
  notices?: ReactNode;
  children: ReactNode;
}

/** 提示词全局页的页头 + 工具行（Prompts.dc.html 第 48–137 行）。 */
export function PromptPageFrame({
  app,
  apps,
  onAppChange,
  appSwitchDisabled = false,
  count,
  help,
  primary,
  moreItems,
  segments,
  target,
  search,
  notices,
  children,
}: PromptPageFrameProps) {
  const { t } = useTranslation();

  return (
    <>
      <AppPageHeader
        icon={<BookOpen className="h-[18px] w-[18px]" strokeWidth={2} />}
        title={t("nav.prompts")}
        titleExtra={
          <HelpTip title={help.title}>{renderSegs(help.text)}</HelpTip>
        }
        actions={
          <>
            {primary ? (
              <Button
                variant="solid"
                size="regular"
                disabled={primary.disabled}
                onClick={primary.onClick}
              >
                <Plus className="h-4 w-4" />
                {primary.label}
              </Button>
            ) : null}
            <DropdownMenu>
              <HoverTip content={t("common.more")}>
                <DropdownMenuTrigger asChild>
                  <Button
                    variant="quiet"
                    size="icon-compact"
                    className="h-8 w-8"
                    aria-label={t("prompts.moreActions")}
                  >
                    <MoreHorizontal className="h-4 w-4" />
                  </Button>
                </DropdownMenuTrigger>
              </HoverTip>
              <DropdownMenuContent
                align="end"
                className={cn(promptMenuContentClass, "w-[300px]")}
              >
                {moreItems.map((item) => (
                  <DropdownMenuItem
                    key={item.key}
                    disabled={Boolean(item.reason)}
                    onSelect={item.onSelect}
                    className={cn(
                      promptMenuItemClass,
                      "flex-col items-start gap-0",
                      item.reason && "text-fg-3",
                    )}
                  >
                    <span>{item.label}</span>
                    {item.reason ? (
                      <span className="text-caption text-fg-2">
                        {renderSegs(item.reason)}
                      </span>
                    ) : null}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          </>
        }
      />

      <div className="mt-0.5 flex h-14 shrink-0 items-center gap-3 px-6">
        <PromptAppPicker
          app={app}
          apps={apps}
          count={count}
          disabled={appSwitchDisabled}
          onAppChange={onAppChange}
        />
        {segments}
        {target ? (
          <span
            title={`${target.label} ${target.path}${target.meta ?? ""}`}
            className="min-w-0 shrink truncate text-caption text-fg-2"
          >
            {target.label}{" "}
            <code className="font-mono text-caption text-fg-1">
              {target.path}
            </code>
            {target.meta}
          </span>
        ) : null}
        <div className="flex-1" />
        {search ? <PromptSearch {...search} /> : null}
      </div>

      {notices ? (
        <div role="status" className="flex shrink-0 flex-col gap-2 px-6 pb-3">
          {notices}
        </div>
      ) : (
        <div role="status" className="sr-only" />
      )}

      <div className="flex min-h-0 flex-1 flex-col px-6 pb-5">{children}</div>
    </>
  );
}

function PromptSearch({
  value,
  onChange,
  placeholder,
  ariaLabel,
  resultCount,
  width = 220,
}: NonNullable<PromptPageFrameProps["search"]>) {
  const { t } = useTranslation();
  const hasQuery = value.trim().length > 0;
  return (
    <div className="relative shrink-0" style={{ width }}>
      <SearchField
        value={value}
        onValueChange={onChange}
        clearLabel={t("prompts.clearSearch")}
        aria-label={ariaLabel}
        placeholder={placeholder}
        autoComplete="off"
      />
      <span role="status" className="sr-only">
        {hasQuery && resultCount !== undefined
          ? t("prompts.searchFound", { count: resultCount })
          : ""}
      </span>
    </div>
  );
}

function PromptAppPicker({
  app,
  apps,
  count,
  disabled,
  onAppChange,
}: {
  app: AppId;
  apps: AppId[];
  count: number;
  disabled: boolean;
  onAppChange: (app: AppId) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  // 弹层里每个应用的条数：打开时才读，读的是和应用页页头入口同一份缓存
  const counts = useQueries({
    queries: apps.map((id) => ({
      queryKey: promptKeys.list(id),
      queryFn: () => promptsApi.getPrompts(id),
      enabled: open,
    })),
  });

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          variant="neutral"
          size="regular"
          disabled={disabled}
          className="shrink-0 gap-2 pe-2 ps-2.5"
        >
          <span className="sr-only">{t("prompts.appLabel")}</span>
          <AppGlyph app={app} size={16} badgeClassName="bg-surface" />
          <span>{APP_DISPLAY_NAME[app]}</span>
          <span className="text-caption font-normal tabular-nums text-fg-2">
            {count}
          </span>
          <ChevronDown
            aria-hidden="true"
            className="h-3.5 w-3.5 shrink-0 text-fg-2"
          />
        </Button>
      </PopoverTrigger>
      <PopoverContent
        align="start"
        aria-label={t("prompts.appSelect")}
        className="flex w-[300px] flex-col rounded-panel border-border bg-surface p-1 shadow-v7-md"
      >
        {apps.map((id, index) => {
          const selected = id === app;
          const data = counts[index]?.data;
          const n = selected
            ? count
            : data
              ? Object.keys(data).length
              : undefined;
          return (
            <button
              key={id}
              type="button"
              aria-pressed={selected}
              onClick={() => {
                setOpen(false);
                if (!selected) onAppChange(id);
              }}
              className={cn(
                "flex min-h-8 w-full items-center gap-2 rounded-control pe-2 ps-2.5 text-left text-body text-fg-1 transition-colors hover:bg-subtle",
                selected ? "font-medium" : "font-normal",
              )}
            >
              <AppGlyph app={id} size={16} badgeClassName="bg-surface" />
              <span className="min-w-0 flex-1 truncate">
                {APP_DISPLAY_NAME[id]}
              </span>
              <span
                className={cn(
                  "text-caption font-normal tabular-nums",
                  n ? "text-fg-2" : "text-fg-3",
                )}
              >
                {n ?? ""}
              </span>
              <Check
                aria-hidden="true"
                strokeWidth={1.5}
                className={cn(
                  "h-3.5 w-3.5 shrink-0",
                  selected ? "visible" : "invisible",
                )}
              />
            </button>
          );
        })}
        <div aria-hidden="true" className="mx-1.5 my-1 h-px bg-border" />
        <p className="m-0 px-2.5 pb-1.5 pt-1 text-caption text-fg-2">
          <span className="block">{t("prompts.appNoteDesktop")}</span>
          <span className="block">{t("prompts.appNoteOpenclaw")}</span>
        </p>
      </PopoverContent>
    </Popover>
  );
}
