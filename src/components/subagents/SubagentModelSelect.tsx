import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle, Check, ChevronDown, Layers, Route } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { Input } from "@/components/ui/input";
import { Notice } from "@/components/ui/notice";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { useSubagentModelOptionsQuery } from "@/hooks/useSubagents";
import type {
  SubagentModelOption,
  SubagentModelOptions,
} from "@/lib/api/subagents";
import { cn } from "@/lib/utils";

/**
 * 后端读不到选项时（还没就绪、读配置失败）的兜底：Claude Code 自己认识的几个值，
 * 不带「→ 实际模型」，表单照样能选。
 */
const FALLBACK_OPTIONS: SubagentModelOption[] = [
  { value: "inherit", kind: "inherit", label: "inherit" },
  { value: "sonnet", kind: "alias", label: "sonnet" },
  { value: "opus", kind: "alias", label: "opus" },
  { value: "haiku", kind: "alias", label: "haiku" },
  { value: "fable", kind: "alias", label: "fable" },
];

const CUSTOM_VALUE = "__custom__";

interface SubagentModelSelectProps {
  id?: string;
  /** frontmatter 里的 model；undefined = 没写（Claude Code 按继承处理） */
  value: string | undefined;
  onChange: (value: string) => void;
  disabled?: boolean;
}

function groupModels(options: SubagentModelOption[]) {
  const groups = new Map<string, SubagentModelOption[]>();
  for (const option of options) {
    const key = option.providerName ?? "";
    const list = groups.get(key) ?? [];
    list.push(option);
    groups.set(key, list);
  }
  return [...groups.entries()];
}

/**
 * 模型下拉：选项由后端按 Claude Code 当前的供应商和模式算好（直连 / 路由 / 聚合下
 * 别名会被映射成什么、哪些完整模型 id 真能到达），这里只负责分组展示和提示。
 */
export function SubagentModelSelect({
  id,
  value,
  onChange,
  disabled = false,
}: SubagentModelSelectProps) {
  const { t } = useTranslation();
  const query = useSubagentModelOptionsQuery();
  const data: SubagentModelOptions | undefined = query.data;
  const options = data?.options.length ? data.options : FALLBACK_OPTIONS;
  const [open, setOpen] = useState(false);
  const [customOpen, setCustomOpen] = useState(false);

  const inheritOptions = options.filter((option) => option.kind === "inherit");
  const aliasOptions = options.filter((option) => option.kind === "alias");
  const modelGroups = useMemo(
    () => groupModels(options.filter((option) => option.kind === "model")),
    [options],
  );

  const effectiveValue = value ?? inheritOptions[0]?.value ?? "inherit";
  const selected = options.find((option) => option.value === effectiveValue);
  // 当前值不在这个模式的可选列表里（手写的、或切换供应商 / 模式之前选的）
  const outOfList = value !== undefined && !selected && !query.isLoading;

  const triggerLabel = selected
    ? selected.kind === "inherit"
      ? t("subagents.model.inherit")
      : selected.label
    : effectiveValue;

  const pick = (next: string) => {
    setOpen(false);
    if (next === CUSTOM_VALUE) {
      setCustomOpen(true);
      return;
    }
    setCustomOpen(false);
    onChange(next);
  };

  const contextLine = (() => {
    if (!data) return null;
    if (data.mode === "stack") {
      return (
        <span className="inline-flex items-center gap-1 text-stack-text">
          <Layers
            aria-hidden="true"
            className="h-3.5 w-3.5"
            strokeWidth={1.5}
          />
          {t("subagents.model.contextStack")}
        </span>
      );
    }
    if (data.mode === "route") {
      return (
        <span className="inline-flex items-center gap-1 text-route-text">
          <Route aria-hidden="true" className="h-3.5 w-3.5" strokeWidth={1.5} />
          {t("subagents.model.contextRoute", {
            provider: data.providerName ?? t("subagents.model.unknownProvider"),
          })}
        </span>
      );
    }
    return (
      <span>
        {data.providerName
          ? t("subagents.model.contextDirect", { provider: data.providerName })
          : t("subagents.model.contextDirectNone")}
      </span>
    );
  })();

  const renderOption = (option: SubagentModelOption) => {
    const isSelected = option.value === effectiveValue;
    const unmapped = option.warning === "alias_unmapped";
    return (
      <CommandItem
        key={`${option.kind}:${option.providerName ?? ""}:${option.value}`}
        value={`${option.kind}:${option.providerName ?? ""}:${option.value}`}
        keywords={[
          option.label,
          option.value,
          option.resolvesTo ?? "",
          option.providerName ?? "",
        ]}
        onSelect={() => pick(option.value)}
        className="items-start gap-2 rounded-control px-2 py-1.5 text-body"
      >
        <Check
          aria-hidden="true"
          strokeWidth={1.5}
          className={cn(
            "mt-0.5 h-3.5 w-3.5 shrink-0",
            isSelected ? "visible" : "invisible",
          )}
        />
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="flex min-w-0 items-center gap-1.5">
            <span
              className={cn(
                "truncate",
                option.kind === "inherit" ? "" : "font-mono text-caption",
                unmapped && "text-warning-text",
              )}
            >
              {option.kind === "inherit"
                ? t("subagents.model.inherit")
                : option.label}
            </span>
            {option.resolvesTo ? (
              <span className="min-w-0 truncate font-mono text-caption text-fg-2">
                → {option.resolvesTo}
              </span>
            ) : null}
            {unmapped ? (
              <AlertTriangle
                aria-label={t("subagents.model.aliasUnmappedShort")}
                strokeWidth={1.5}
                className="h-3.5 w-3.5 shrink-0 text-warning-text"
              />
            ) : null}
          </span>
          {option.kind === "inherit" ? (
            <span className="text-caption text-fg-2">
              {t("subagents.model.inheritHint")}
            </span>
          ) : option.description ? (
            <span className="text-caption text-fg-2">{option.description}</span>
          ) : null}
        </span>
      </CommandItem>
    );
  };

  const notices: Array<{
    key: string;
    text: string;
    tone: "neutral" | "warning";
  }> = [];
  for (const notice of data?.notices ?? []) {
    if (notice === "subagent_model_override") {
      notices.push({
        key: notice,
        tone: "warning",
        text: t("subagents.model.notice.override", {
          value: data?.subagentModelOverride ?? "",
        }),
      });
    } else if (notice === "route_aliases_only") {
      notices.push({
        key: notice,
        tone: "neutral",
        text: t("subagents.model.notice.routeAliasesOnly"),
      });
    } else if (notice === "stack_aliases_share_default") {
      notices.push({
        key: notice,
        tone: "neutral",
        text: t("subagents.model.notice.stackAliasesShareDefault"),
      });
    } else if (notice === "no_provider") {
      notices.push({
        key: notice,
        tone: "neutral",
        text: t("subagents.model.notice.noProvider"),
      });
    }
  }

  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex min-h-[18px] items-center gap-1 text-caption text-fg-2">
        {query.isLoading ? (
          <span>{t("subagents.model.loading")}</span>
        ) : query.isError ? (
          <span>{t("subagents.model.loadFailed")}</span>
        ) : (
          contextLine
        )}
      </div>
      <Popover modal open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button
            id={id}
            type="button"
            variant="neutral"
            size="regular"
            disabled={disabled}
            aria-haspopup="listbox"
            className={cn(
              "w-full justify-between gap-2 px-2.5 font-normal",
              (outOfList || selected?.warning) && "border-warning",
            )}
          >
            <span className="flex min-w-0 items-center gap-1.5">
              <span
                className={cn(
                  "truncate",
                  selected?.kind !== "inherit" && "font-mono text-caption",
                )}
              >
                {triggerLabel}
              </span>
              {selected?.resolvesTo ? (
                <span className="min-w-0 truncate font-mono text-caption text-fg-2">
                  → {selected.resolvesTo}
                </span>
              ) : null}
            </span>
            <ChevronDown
              aria-hidden="true"
              className="h-3.5 w-3.5 shrink-0 text-fg-2"
            />
          </Button>
        </PopoverTrigger>
        <PopoverContent
          align="start"
          sideOffset={4}
          collisionPadding={8}
          className="z-[200] w-[var(--radix-popover-trigger-width)] min-w-[320px] rounded-panel border-border bg-surface p-0 shadow-v7-md"
        >
          <Command label={t("subagents.model.searchPlaceholder")}>
            <CommandInput
              data-unsaved-ignore=""
              placeholder={t("subagents.model.searchPlaceholder")}
            />
            <CommandList className="max-h-72">
              <CommandEmpty>{t("subagents.model.searchEmpty")}</CommandEmpty>
              {outOfList && value !== undefined ? (
                <CommandGroup heading={t("subagents.model.groupCurrent")}>
                  <CommandItem
                    value={`current:${value}`}
                    onSelect={() => pick(value)}
                    className="items-start gap-2 rounded-control px-2 py-1.5 text-body"
                  >
                    <Check
                      aria-hidden="true"
                      strokeWidth={1.5}
                      className="mt-0.5 h-3.5 w-3.5 shrink-0"
                    />
                    <span className="flex min-w-0 flex-col">
                      <span className="truncate font-mono text-caption text-warning-text">
                        {value}
                      </span>
                      <span className="text-caption text-fg-2">
                        {t("subagents.model.notInList")}
                      </span>
                    </span>
                  </CommandItem>
                </CommandGroup>
              ) : null}
              {inheritOptions.length > 0 ? (
                <CommandGroup>{inheritOptions.map(renderOption)}</CommandGroup>
              ) : null}
              {aliasOptions.length > 0 ? (
                <CommandGroup heading={t("subagents.model.groupAlias")}>
                  {aliasOptions.map(renderOption)}
                </CommandGroup>
              ) : null}
              {modelGroups.map(([providerName, items]) => (
                <CommandGroup
                  key={`models:${providerName}`}
                  heading={
                    providerName
                      ? t("subagents.model.groupModelsOf", {
                          provider: providerName,
                        })
                      : t("subagents.model.groupModels")
                  }
                >
                  {items.map(renderOption)}
                </CommandGroup>
              ))}
              <CommandGroup>
                <CommandItem
                  value={CUSTOM_VALUE}
                  keywords={["custom"]}
                  onSelect={() => pick(CUSTOM_VALUE)}
                  className="gap-2 rounded-control px-2 py-1.5 text-body"
                >
                  <span className="h-3.5 w-3.5 shrink-0" />
                  {t("subagents.model.custom")}
                </CommandItem>
              </CommandGroup>
            </CommandList>
          </Command>
        </PopoverContent>
      </Popover>

      {customOpen ? (
        <Input
          autoFocus
          value={value ?? ""}
          disabled={disabled}
          onChange={(event) => onChange(event.target.value.trim())}
          placeholder={t("subagents.model.customPlaceholder")}
          aria-label={t("subagents.model.custom")}
          className="font-mono text-caption"
        />
      ) : null}

      {selected?.warning === "alias_unmapped" ? (
        <p className="m-0 flex items-start gap-1 text-caption text-warning-text">
          <AlertTriangle
            aria-hidden="true"
            strokeWidth={1.5}
            className="mt-0.5 h-3.5 w-3.5 shrink-0"
          />
          <span>{t("subagents.model.aliasUnmapped")}</span>
        </p>
      ) : null}
      {outOfList ? (
        <p className="m-0 flex items-start gap-1 text-caption text-warning-text">
          <AlertTriangle
            aria-hidden="true"
            strokeWidth={1.5}
            className="mt-0.5 h-3.5 w-3.5 shrink-0"
          />
          <span>{t("subagents.model.notInListHint", { value })}</span>
        </p>
      ) : null}
      {notices.map((notice) => (
        <Notice
          key={notice.key}
          tone={notice.tone}
          title={notice.text}
          className="py-2 text-caption"
        />
      ))}
    </div>
  );
}
