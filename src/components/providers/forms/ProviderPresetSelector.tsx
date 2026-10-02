import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
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
import { SegmentedControl } from "@/components/ui/segmented-control";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import {
  PRESET_GROUP_ORDER,
  familyDisplayName,
  groupPresetRows,
  loginAccountKey,
  matchPresetRow,
  presetDisplayName,
  presetDomain,
  presetGroup,
  presetMatches,
  presetNeedsRouting,
  presetPlanLabel,
  presetRegionLabel,
  presetRowGroup,
  presetRowName,
  presetRowNeedsRouting,
  presetVersionLabel,
  presetVersionLayout,
  presetVersionShortLabel,
  presetVersions,
  sectionPresetRows,
  sortPresetRowsByName,
  sortPresetsByName,
  type PresetGroup,
  type PresetRowItem,
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

/** 第 1 步的一行和搜索命中的版本下标（空 = 整家命中） */
export interface VisiblePresetRow {
  row: PresetRowItem;
  hits: number[];
}

/** 同一家的多个版本合成一行，再按名称排 */
export function getVisiblePresetRows(
  entries: PresetEntry[],
  { query, t }: { query: string; t: PresetTranslator },
): VisiblePresetRow[] {
  const found = groupPresetRows(entries).flatMap((row) => {
    const match = matchPresetRow(row, query, t);
    return match ? [{ row, hits: match.versions }] : [];
  });
  return sortPresetRowsByName(found, t);
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

  const entry =
    selectedPresetId && selectedPresetId !== "custom"
      ? presetEntries.find((item) => item.id === selectedPresetId)
      : undefined;

  if (!step) {
    // 没有两步外壳（单独渲染的表单）：就地显示选择列表，选中多版本的那一家时下面跟版本切换
    const versions = entry ? presetVersions(presetEntries, entry) : [];
    return (
      <div className="space-y-3" data-preset-selector="">
        <div className="h-[420px] overflow-hidden rounded-panel border border-border">
          <PresetPicker
            entries={presetEntries}
            onPick={onPresetChange}
            selectedPresetId={selectedPresetId}
            onUniversalPresetSelect={onUniversalPresetSelect}
            onManageUniversalProviders={onManageUniversalProviders}
          />
        </div>
        {entry && versions.length > 1 && (
          <VersionSwitch
            entry={entry}
            versions={versions}
            onVersionChange={onPresetChange}
          />
        )}
      </div>
    );
  }

  return (
    <PresetBar
      appId={step.appId}
      entry={entry}
      versions={entry ? presetVersions(presetEntries, entry) : []}
      onChange={() => step.setStep("pick")}
      // 换版本 = 选了同一家的另一个预设，表单按它重填
      onVersionChange={onPresetChange}
    />
  );
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
  appId,
  entry,
  versions,
  onChange,
  onVersionChange,
}: {
  appId: AppId;
  entry?: PresetEntry;
  /** 同一家的所有版本（含选中的这个）；只有一个时不显示「版本」 */
  versions: PresetEntry[];
  onChange: () => void;
  onVersionChange: (id: string) => void;
}) {
  const { t } = useTranslation();
  const family = versions.length > 1 ? entry?.preset.family : undefined;
  const name = entry
    ? family
      ? familyDisplayName(family, t)
      : presetDisplayName(entry.preset, t)
    : "";
  const domain = entry ? presetDomain(entry.preset) : "";
  return (
    <div
      data-preset-selector=""
      className="flex flex-col gap-3 rounded-panel bg-subtle px-3.5 py-3"
    >
      <div className="flex items-center gap-3">
        <PresetIconBox preset={entry?.preset} />
        <div className="min-w-0 flex-1">
          {entry ? (
            <>
              <div className="flex min-w-0 items-center gap-1.5">
                <span className="truncate text-strong text-fg-1" title={name}>
                  {name}
                </span>
                {presetNeedsRouting(appId, entry) && <NeedsRouteBadge />}
              </div>
              {domain && (
                <div className="truncate text-caption text-fg-2">{domain}</div>
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
      {entry && family && (
        <VersionSwitch
          entry={entry}
          versions={versions}
          onVersionChange={onVersionChange}
        />
      )}
    </div>
  );
}

/**
 * 用户在表单里手动改过东西没有：只认表单里的 input / change 事件。预设填充是程序改值，
 * 不发这些事件；Radix Select / Checkbox / Switch 跟着值同步的隐藏控件（aria-hidden）会发，
 * 要排除；预设选择器自己（搜索框、版本按钮）也不算。`resetKey` 变了（换了预设）就重新算。
 */
function useFormEditedSince(
  anchor: React.RefObject<HTMLElement | null>,
  resetKey: string,
) {
  const edited = useRef(false);
  useEffect(() => {
    edited.current = false;
  }, [resetKey]);
  useEffect(() => {
    const node = anchor.current;
    const root = node?.closest("form") ?? node?.ownerDocument.body;
    if (!root) return;
    const onEdit = (event: Event) => {
      const target = event.target;
      if (!(target instanceof Element)) return;
      if (target.closest("[data-preset-selector]")) return;
      if (target.closest('[aria-hidden="true"]')) return;
      edited.current = true;
    };
    root.addEventListener("input", onEdit);
    root.addEventListener("change", onEdit);
    return () => {
      root.removeEventListener("input", onEdit);
      root.removeEventListener("change", onEdit);
    };
  }, [anchor]);
  return edited;
}

/**
 * 选版本（换版本 = 选同一家的另一个预设，表单按它重填）。版本按「套餐 × 地区」两个维度摆：
 * 只有一维在变 → 一个分段控件；两维都变且成完整网格 → 套餐、地区各一个分段控件
 * （切一维时另一维保持不变）；不成网格 → 下拉。见 presetVersionLayout。
 * 用户已经手动改过表单时先确认（后果写在确认框里），没改过直接切。
 */
function VersionSwitch({
  entry,
  versions,
  onVersionChange,
}: {
  entry: PresetEntry;
  versions: PresetEntry[];
  onVersionChange: (id: string) => void;
}) {
  const { t } = useTranslation();
  const ref = useRef<HTMLDivElement>(null);
  const edited = useFormEditedSince(ref, entry.id);
  const [pending, setPending] = useState<PresetEntry | null>(null);
  const layout = presetVersionLayout(versions);

  const request = (id: string) => {
    if (id === entry.id) return;
    const next = versions.find((version) => version.id === id);
    if (!next) return;
    if (edited.current) {
      setPending(next);
    } else {
      onVersionChange(id);
    }
  };

  // 网格里切一维：另一维保持当前值
  const requestCell = (
    plan = entry.preset.planKey,
    region = entry.preset.regionKey,
  ) => {
    const next = versions.find(
      (version) =>
        version.preset.planKey === plan && version.preset.regionKey === region,
    );
    if (next) request(next.id);
  };

  const segmentClass =
    "h-auto min-w-0 flex-wrap gap-0.5 border border-border-strong bg-transparent p-0.5";

  return (
    <div
      ref={ref}
      className="grid grid-cols-[auto_minmax(0,1fr)] items-start gap-x-3 gap-y-2"
    >
      {layout.kind === "grid" ? (
        <>
          <DimensionLabel>{t("providerPreset.planLabel")}</DimensionLabel>
          <SegmentedControl
            aria-label={t("providerPreset.planLabel")}
            value={entry.preset.planKey ?? ""}
            onValueChange={(plan) =>
              requestCell(plan as typeof entry.preset.planKey)
            }
            items={layout.plans.map((plan) => ({
              value: plan,
              label: presetPlanLabel(plan, t),
              className: "h-[30px]",
            }))}
            className={cn(segmentClass, "justify-self-start")}
          />
          <DimensionLabel>{t("providerPreset.regionLabel")}</DimensionLabel>
          <SegmentedControl
            aria-label={t("providerPreset.regionLabel")}
            value={entry.preset.regionKey ?? ""}
            onValueChange={(region) =>
              requestCell(
                entry.preset.planKey,
                region as typeof entry.preset.regionKey,
              )
            }
            items={layout.regions.map((region) => ({
              value: region,
              label: presetRegionLabel(region, t),
              className: "h-[30px]",
            }))}
            className={cn(segmentClass, "justify-self-start")}
          />
        </>
      ) : layout.kind === "single" ? (
        <>
          <DimensionLabel>{t("providerPreset.versionLabel")}</DimensionLabel>
          <SegmentedControl
            aria-label={t("providerPreset.versionLabel")}
            value={entry.id}
            onValueChange={request}
            items={versions.map((version) => ({
              value: version.id,
              label: presetVersionShortLabel(version, layout.dimension, t),
              className: "h-[30px]",
            }))}
            className={cn(segmentClass, "justify-self-start")}
          />
        </>
      ) : (
        <>
          <DimensionLabel>{t("providerPreset.versionLabel")}</DimensionLabel>
          <Select value={entry.id} onValueChange={request}>
            <SelectTrigger
              aria-label={t("providerPreset.versionLabel")}
              className="h-9 w-auto min-w-[220px] max-w-full justify-self-start rounded-control text-body"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {versions.map((version) => (
                <SelectItem key={version.id} value={version.id}>
                  {presetVersionLabel(version, t)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </>
      )}
      <ConfirmDialog
        isOpen={pending !== null}
        title={t("providerPreset.switchVersionTitle", {
          version: pending ? presetVersionLabel(pending, t) : "",
        })}
        message={t("providerPreset.switchVersionMessage")}
        confirmText={t("providerPreset.switchVersionConfirm")}
        variant="info"
        onConfirm={() => {
          const next = pending;
          setPending(null);
          if (next) onVersionChange(next.id);
        }}
        onCancel={() => setPending(null)}
      />
    </div>
  );
}

/** 版本区左侧的小标签（版本 / 套餐 / 地区），和控件第一行垂直居中 */
function DimensionLabel({ children }: { children: React.ReactNode }) {
  return (
    <span
      aria-hidden="true"
      className="min-w-8 whitespace-nowrap text-center text-caption leading-[36px] text-fg-2"
    >
      {children}
    </span>
  );
}

function NeedsRouteBadge() {
  const { t } = useTranslation();
  return (
    <span className="inline-flex h-[18px] shrink-0 items-center whitespace-nowrap rounded-full border border-border-strong px-1.5 text-badge text-fg-2">
      {t("providerCard.chip.needsRoute")}
    </span>
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

  // 同一家的多个版本合成一行，数量也按行数算
  const matching = useMemo(
    () => getVisiblePresetRows(entries, { query, t }),
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
    // 「自定义配置」固定在第一行，不计入数量
    if (key === "all") return matching.length;
    return matching.filter((item) => presetRowGroup(item.row) === key).length;
  };

  const shown =
    category === "all"
      ? matching
      : category === "universal"
        ? []
        : matching.filter((item) => presetRowGroup(item.row) === category);

  const navItems: PickerCategory[] = [
    "all",
    ...PRESET_GROUP_ORDER.filter((group) => groups.has(group)),
  ];
  const searching = query.trim().length > 0;

  const customRow = (
    <PresetRow
      icon={<PresetIconBox />}
      name={t("providerPreset.custom")}
      detail={t("providerPreset.customDetail")}
      selected={selectedPresetId === "custom"}
      onClick={() => onPick("custom")}
    />
  );
  const renderRow = ({ row, hits }: VisiblePresetRow) => {
    const first = row.versions[0];
    // 搜索命中某个版本时选中那个版本，否则选第一个
    const target = row.versions[hits[0] ?? 0];
    return (
      <PresetRow
        key={row.key}
        icon={<PresetIconBox preset={first.preset} />}
        name={presetRowName(row, t)}
        detail={presetRowDetail(appId, row, hits, t)}
        needsRoute={presetRowNeedsRouting(appId, row)}
        selected={row.versions.some((entry) => entry.id === selectedPresetId)}
        onClick={() => onPick(target.id)}
      />
    );
  };

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
            className="pe-9 ps-9"
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
          ) : category === "all" ? (
            // 「全部」按左侧分类分段，段内按名称；自定义配置固定在最上面
            <div className="flex flex-col gap-4">
              <div className="grid grid-cols-2 gap-2">{customRow}</div>
              {sectionPresetRows(shown).map((section) => (
                <section
                  key={section.group}
                  aria-labelledby={`preset-section-${section.group}`}
                  className="flex flex-col gap-2"
                >
                  <h3
                    id={`preset-section-${section.group}`}
                    className="px-0.5 text-caption font-medium text-fg-2"
                  >
                    {t(`providerPreset.group.${section.group}`)}
                  </h3>
                  <div className="grid grid-cols-2 gap-2">
                    {section.items.map(renderRow)}
                  </div>
                </section>
              ))}
            </div>
          ) : (
            <div className="grid grid-cols-2 gap-2">
              {customRow}
              {shown.map(renderRow)}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

/**
 * 副行：搜索命中某个版本时写「匹配：火山 Coding Plan」；账号登录类写登录说明；
 * 有多个版本的写「kimi.com · 4 个版本」；其余写域名。
 */
function presetRowDetail(
  appId: AppId | undefined,
  row: PresetRowItem,
  hits: number[],
  t: TFunction,
): string {
  const first = row.versions[0];
  if (hits.length > 0) {
    const name = presetDisplayName(row.versions[hits[0]].preset, t);
    return hits.length > 1
      ? t("providerPreset.matchedVersions", {
          name,
          n: hits.length,
          more: hits.length - 1,
        })
      : t("providerPreset.matchedVersion", { name });
  }
  if (presetRowGroup(row) === "login") {
    return t(
      `providerPreset.loginWith.${loginAccountKey(appId, first.preset)}`,
    );
  }
  if (row.family) {
    return t("providerPreset.versionCount", {
      domain: presetDomain(first.preset),
      n: row.versions.length,
    });
  }
  return presetDomain(first.preset);
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
      {needsRoute && <NeedsRouteBadge />}
    </button>
  );
}
