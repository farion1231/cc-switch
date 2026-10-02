import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { Search, SlidersHorizontal, X, Zap } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ClaudeIcon, CodexIcon, GeminiIcon } from "@/components/BrandIcons";
import { ProviderIcon } from "@/components/ProviderIcon";
import type { ProviderPreset } from "@/config/claudeProviderPresets";
import type { CodexProviderPreset } from "@/config/codexProviderPresets";
import type { GeminiProviderPreset } from "@/config/geminiProviderPresets";
import type { ClaudeDesktopProviderPreset } from "@/config/claudeDesktopProviderPresets";
import type { OpenCodeProviderPreset } from "@/config/opencodeProviderPresets";
import type { OpenClawProviderPreset } from "@/config/openclawProviderPresets";
import type { HermesProviderPreset } from "@/config/hermesProviderPresets";
import type { McodeProviderPreset } from "@/config/mcodeProviderPresets";
import type { PiProviderPreset } from "@/config/piProviderPresets";
import type { ProviderCategory } from "@/types";
import type { AppId } from "@/lib/api";
import {
  universalProviderPresets,
  type UniversalProviderPreset,
} from "@/config/universalProviderPresets";
import { cn } from "@/lib/utils";
import { usePresetStep } from "./presetStep";
import {
  PRESET_GROUP_ORDER,
  loginAccountKey,
  presetDisplayName,
  presetDomain,
  presetGroup,
  presetMatches,
  presetNeedsRouting,
  sortPresetsByName,
  type PresetGroup,
} from "./presetGroups";

type PresetTranslator = (key: string) => unknown;

export type AnyPreset =
  | ProviderPreset
  | CodexProviderPreset
  | GeminiProviderPreset
  | ClaudeDesktopProviderPreset
  | OpenCodeProviderPreset
  | OpenClawProviderPreset
  | HermesProviderPreset
  | PiProviderPreset
  | McodeProviderPreset;

export type PresetEntry = {
  id: string;
  preset: AnyPreset;
};

export function getPresetDisplayName(
  preset: AnyPreset,
  t: PresetTranslator,
): string {
  return presetDisplayName(preset, t);
}

/** 搜索：名称、显示名、域名主体、别名（中文名、公司名） */
export function filterPresetEntries(
  entries: PresetEntry[],
  query: string,
  t: PresetTranslator,
): PresetEntry[] {
  if (!query.trim()) return entries;
  return entries.filter((entry) => presetMatches(entry, query, t));
}

/** 一律按名称排（中文名按拼音插进字母序），不再有官方 / 赞助商置顶 */
export function getVisiblePresetEntries(
  entries: PresetEntry[],
  { query, t }: { query: string; t: PresetTranslator },
): PresetEntry[] {
  return sortPresetsByName(filterPresetEntries(entries, query, t), t);
}

type PickerCategory = "all" | PresetGroup | "universal";

interface ProviderPresetSelectorProps {
  selectedPresetId: string | null;
  presetEntries: PresetEntry[];
  /** 旧的分类名表，保留给调用方；v7 的分类由预设字段推算 */
  presetCategoryLabels?: Record<string, string>;
  onPresetChange: (value: string) => void;
  onUniversalPresetSelect?: (preset: UniversalProviderPreset) => void;
  onManageUniversalProviders?: () => void;
  category?: ProviderCategory;
}

/**
 * 添加供应商的预设（v7 两步）：第 1 步在添加页的内容区里选（常驻搜索 + 左侧分类 + 两列列表），
 * 第 2 步在表单最上面只剩一条「预设条」，点「更换」回到第 1 步。
 */
export function ProviderPresetSelector({
  selectedPresetId,
  presetEntries,
  onPresetChange,
  onUniversalPresetSelect,
  onManageUniversalProviders,
}: Readonly<ProviderPresetSelectorProps>) {
  const step = usePresetStep();
  const registerSelector = step?.registerSelector;
  useEffect(() => registerSelector?.(), [registerSelector]);

  const pick = (id: string) => {
    onPresetChange(id);
    step?.setStep("form");
  };

  if (step?.step === "pick" && step.host) {
    return createPortal(
      <PresetPicker
        appId={step.appId}
        entries={presetEntries}
        onPick={pick}
        onUniversalPresetSelect={onUniversalPresetSelect}
        onManageUniversalProviders={onManageUniversalProviders}
      />,
      step.host,
    );
  }

  if (!step) {
    // 没有两步外壳（单独渲染的表单）：就地显示选择列表
    return (
      <div className="h-[420px] overflow-hidden rounded-panel border border-border">
        <PresetPicker
          entries={presetEntries}
          onPick={onPresetChange}
          selectedPresetId={selectedPresetId}
          onUniversalPresetSelect={onUniversalPresetSelect}
          onManageUniversalProviders={onManageUniversalProviders}
        />
      </div>
    );
  }

  const entry =
    selectedPresetId && selectedPresetId !== "custom"
      ? presetEntries.find((item) => item.id === selectedPresetId)
      : undefined;
  return <PresetBar entry={entry} onChange={() => step.setStep("pick")} />;
}

// ─── 图标块 ─────────────────────────────────────────────────────────────────

function PresetIconBox({ preset }: { preset?: AnyPreset }) {
  const { t } = useTranslation();
  let inner: React.ReactNode;
  if (!preset) {
    inner = (
      <SlidersHorizontal className="h-4 w-4 text-fg-2" strokeWidth={1.5} />
    );
  } else if (preset.icon) {
    inner = (
      <ProviderIcon
        icon={preset.icon}
        name={preset.name}
        color={preset.iconColor}
        size={18}
        className="shrink-0 text-fg-1"
      />
    );
  } else if (preset.theme?.icon === "claude") {
    inner = <ClaudeIcon size={16} />;
  } else if (preset.theme?.icon === "codex") {
    inner = <CodexIcon size={16} />;
  } else if (preset.theme?.icon === "gemini") {
    inner = <GeminiIcon size={16} />;
  } else if (preset.theme?.icon === "generic") {
    inner = <Zap className="h-4 w-4 text-fg-2" strokeWidth={1.5} />;
  } else {
    inner = (
      <span className="text-caption font-semibold text-fg-2">
        {presetDisplayName(preset, t).slice(0, 1).toUpperCase()}
      </span>
    );
  }
  return (
    <span
      aria-hidden="true"
      className="flex h-8 w-8 shrink-0 items-center justify-center rounded-[8px] border border-border bg-surface"
    >
      {inner}
    </span>
  );
}

// ─── 第 2 步：预设条 ────────────────────────────────────────────────────────

function PresetBar({
  entry,
  onChange,
}: {
  entry?: PresetEntry;
  onChange: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex items-center gap-3 rounded-panel bg-subtle px-3.5 py-3">
      <PresetIconBox preset={entry?.preset} />
      <div className="min-w-0 flex-1">
        {entry ? (
          <>
            <div className="truncate text-strong text-fg-1">
              {presetDisplayName(entry.preset, t)}
            </div>
            {presetDomain(entry.preset) && (
              <div className="truncate text-caption text-fg-2">
                {presetDomain(entry.preset)}
              </div>
            )}
          </>
        ) : (
          <div className="truncate text-strong text-fg-1">
            {t("providerPreset.customBar")}
          </div>
        )}
      </div>
      <Button type="button" variant="quiet" size="compact" onClick={onChange}>
        {t("providerPreset.change")}
      </Button>
    </div>
  );
}

// ─── 第 1 步：选预设 ────────────────────────────────────────────────────────

interface PresetPickerProps {
  appId?: AppId;
  entries: PresetEntry[];
  selectedPresetId?: string | null;
  onPick: (id: string) => void;
  onUniversalPresetSelect?: (preset: UniversalProviderPreset) => void;
  onManageUniversalProviders?: () => void;
}

function PresetPicker({
  appId,
  entries,
  selectedPresetId,
  onPick,
  onUniversalPresetSelect,
  onManageUniversalProviders,
}: PresetPickerProps) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState<PickerCategory>("all");
  const searchRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const frame = requestAnimationFrame(() => searchRef.current?.focus());
    return () => cancelAnimationFrame(frame);
  }, []);

  // ⌘F / Ctrl+F 回到搜索框（捕获阶段，别让后面供应商列表的同名快捷键接到）
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f") {
        event.preventDefault();
        event.stopPropagation();
        searchRef.current?.focus();
      }
    };
    globalThis.addEventListener("keydown", onKeyDown, true);
    return () => globalThis.removeEventListener("keydown", onKeyDown, true);
  }, []);

  const groups = useMemo(() => {
    const byGroup = new Map<PresetGroup, PresetEntry[]>();
    for (const entry of entries) {
      const group = presetGroup(entry.preset);
      byGroup.set(group, [...(byGroup.get(group) ?? []), entry]);
    }
    return byGroup;
  }, [entries]);

  const matching = useMemo(
    () => getVisiblePresetEntries(entries, { query, t }),
    [entries, query, t],
  );
  const matchingUniversal = useMemo(
    () =>
      onUniversalPresetSelect
        ? universalProviderPresets.filter((preset) =>
            preset.name.toLowerCase().includes(query.trim().toLowerCase()),
          )
        : [],
    [onUniversalPresetSelect, query],
  );

  const countFor = (key: PickerCategory) => {
    if (key === "universal") return matchingUniversal.length;
    if (key === "all") return matching.length + 1;
    return matching.filter((entry) => presetGroup(entry.preset) === key).length;
  };

  const shown =
    category === "all"
      ? matching
      : category === "universal"
        ? []
        : matching.filter((entry) => presetGroup(entry.preset) === category);

  const navItems: PickerCategory[] = [
    "all",
    ...PRESET_GROUP_ORDER.filter((group) => groups.has(group)),
  ];
  const searching = query.trim().length > 0;
  const nothing =
    searching &&
    matching.length === 0 &&
    (category !== "universal" || matchingUniversal.length === 0);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="shrink-0 px-6 pb-3 pt-4">
        <div className="relative">
          <Search
            className="pointer-events-none absolute start-3 top-1/2 h-4 w-4 -translate-y-1/2 text-fg-3"
            strokeWidth={1.5}
          />
          <Input
            ref={searchRef}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape" && query) {
                event.preventDefault();
                setQuery("");
              }
            }}
            placeholder={t("providerPreset.searchPlaceholder")}
            aria-label={t("providerPreset.searchAriaLabel")}
            className="h-9 pe-9 ps-9"
          />
          {query && (
            <button
              type="button"
              onClick={() => {
                setQuery("");
                searchRef.current?.focus();
              }}
              aria-label={t("providerPreset.clearSearch")}
              className="absolute end-2 top-1/2 flex h-6 w-6 -translate-y-1/2 items-center justify-center rounded-control text-fg-3 hover:bg-subtle hover:text-fg-1"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          )}
        </div>
      </div>

      <div className="flex min-h-0 flex-1 gap-4 px-6 pb-4">
        <div
          role="group"
          aria-label={t("providerPreset.categoriesLabel")}
          className="flex w-[168px] shrink-0 flex-col gap-0.5 overflow-y-auto"
        >
          {navItems.map((key) => (
            <CategoryButton
              key={key}
              label={t(`providerPreset.group.${key}`)}
              count={countFor(key)}
              active={category === key}
              dim={searching && countFor(key) === 0}
              onClick={() => setCategory(key)}
            />
          ))}
          {onUniversalPresetSelect && (
            <>
              <div className="my-2 border-t border-border" />
              <div className="px-2 pb-1 text-badge text-fg-3">
                {t("providerPreset.crossApp")}
              </div>
              <CategoryButton
                label={t("providerPreset.group.universal")}
                count={countFor("universal")}
                active={category === "universal"}
                dim={searching && countFor("universal") === 0}
                onClick={() => setCategory("universal")}
              />
            </>
          )}
        </div>

        <div className="min-w-0 flex-1 overflow-y-auto">
          {category !== "all" && (
            <p className="mb-2 text-caption text-fg-2">
              {t(`providerPreset.groupHint.${category}`)}
            </p>
          )}

          {nothing ? (
            <div className="flex flex-col items-center gap-2 py-12 text-center">
              <p className="text-body text-fg-1">
                {t("providerPreset.noResults", { query: query.trim() })}
              </p>
              <p className="text-caption text-fg-2">
                {t("providerPreset.noResultsHint")}
              </p>
              <Button
                type="button"
                variant="neutral"
                size="compact"
                className="mt-1"
                onClick={() => onPick("custom")}
              >
                {t("providerPreset.useCustom")}
              </Button>
            </div>
          ) : category === "universal" ? (
            <div className="grid grid-cols-2 gap-2">
              {matchingUniversal.map((preset) => (
                <PresetRow
                  key={`universal-${preset.providerType}`}
                  icon={
                    <span
                      aria-hidden="true"
                      className="flex h-8 w-8 shrink-0 items-center justify-center rounded-[8px] border border-border bg-surface"
                    >
                      <ProviderIcon
                        icon={preset.icon}
                        name={preset.name}
                        size={18}
                        className="shrink-0 text-fg-1"
                      />
                    </span>
                  }
                  name={preset.name}
                  detail={t("providerPreset.universalDetail")}
                  onClick={() => onUniversalPresetSelect?.(preset)}
                />
              ))}
              {onManageUniversalProviders && (
                <button
                  type="button"
                  onClick={onManageUniversalProviders}
                  className="col-span-2 justify-self-start text-caption text-fg-1 underline underline-offset-2 hover:text-fg-2"
                >
                  {t("providerPreset.manageUniversal")}
                </button>
              )}
            </div>
          ) : (
            <div className="grid grid-cols-2 gap-2">
              <PresetRow
                icon={<PresetIconBox />}
                name={t("providerPreset.custom")}
                detail={t("providerPreset.customDetail")}
                selected={selectedPresetId === "custom"}
                onClick={() => onPick("custom")}
              />
              {shown.map((entry) => {
                const group = presetGroup(entry.preset);
                return (
                  <PresetRow
                    key={entry.id}
                    icon={<PresetIconBox preset={entry.preset} />}
                    name={presetDisplayName(entry.preset, t)}
                    detail={
                      group === "login"
                        ? t(
                            `providerPreset.loginWith.${loginAccountKey(appId, entry.preset)}`,
                          )
                        : presetDomain(entry.preset)
                    }
                    needsRoute={presetNeedsRouting(appId, entry)}
                    selected={selectedPresetId === entry.id}
                    onClick={() => onPick(entry.id)}
                  />
                );
              })}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function CategoryButton({
  label,
  count,
  active,
  dim,
  onClick,
}: {
  label: string;
  count: number;
  active: boolean;
  dim: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      className={cn(
        "flex h-8 items-center justify-between gap-2 rounded-control px-2 text-start text-body transition-colors",
        active ? "bg-selected font-medium text-fg-1" : "hover:bg-subtle",
        !active && (dim ? "text-fg-3" : "text-fg-1"),
      )}
    >
      <span className="truncate">{label}</span>
      <span className="shrink-0 text-caption tabular-nums text-fg-3">
        {count}
      </span>
    </button>
  );
}

function PresetRow({
  icon,
  name,
  detail,
  needsRoute = false,
  selected = false,
  onClick,
}: {
  icon: React.ReactNode;
  name: string;
  detail?: string;
  needsRoute?: boolean;
  selected?: boolean;
  onClick: () => void;
}) {
  const { t } = useTranslation();
  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={selected || undefined}
      aria-label={name}
      aria-description={detail}
      className={cn(
        "flex h-[52px] min-w-0 items-center gap-3 rounded-panel border bg-surface px-3 text-start transition-colors hover:bg-subtle",
        selected ? "border-border-strong" : "border-border",
      )}
    >
      {icon}
      <span className="min-w-0 flex-1">
        <span
          className="block truncate text-body font-medium text-fg-1"
          title={name}
        >
          {name}
        </span>
        {detail && (
          <span
            className="block truncate text-caption text-fg-2"
            title={detail}
          >
            {detail}
          </span>
        )}
      </span>
      {needsRoute && (
        <span className="inline-flex h-[18px] shrink-0 items-center whitespace-nowrap rounded-full border border-border-strong px-1.5 text-badge text-fg-2">
          {t("providerCard.chip.needsRoute")}
        </span>
      )}
    </button>
  );
}
