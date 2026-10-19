import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, ChevronDown } from "lucide-react";
import { fieldClass, Input } from "@/components/ui/input";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { UsageDisplaySettings } from "@/types";
import { cn } from "@/lib/utils";
import {
  DISPLAY_CURRENCIES,
  getResolvedLang,
  isValidExchangeRate,
  resolveMoneyDisplay,
  resolveSidebarToday,
  resolveTrendChartType,
  type DisplayCurrency,
  SIDEBAR_TODAY_ITEMS,
  SIDEBAR_TODAY_MAX_ITEMS,
  type SidebarTodayItem,
  TREND_CHART_TYPES,
  type TrendChartType,
} from "./format";
import {
  REQUEST_LOG_COLUMN_LABEL_KEYS,
  resolveRequestLogColumns,
  setRequestLogColumn,
  TOGGLEABLE_REQUEST_LOG_COLUMNS,
  type RequestLogColumnId,
} from "./requestLogColumns";

interface UsageDisplaySectionProps {
  value: UsageDisplaySettings | undefined;
  onChange: (next: UsageDisplaySettings) => Promise<boolean> | boolean | void;
}

/** 「美元」「日元」：按界面语言取币种名，取不到就只写代码 */
function currencyName(code: DisplayCurrency, lang: string): string | undefined {
  try {
    const name = new Intl.DisplayNames([lang], { type: "currency" }).of(code);
    return name && name !== code ? name : undefined;
  } catch {
    return undefined;
  }
}

interface ChecklistItem {
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
function ChecklistMenu({
  label,
  summary,
  items,
  onToggle,
  onReset,
  note,
  className,
}: {
  label: string;
  summary: string;
  items: ChecklistItem[];
  onToggle: (id: string) => void;
  onReset?: () => void;
  note?: string;
  /** 按钮宽度 */
  className: string;
}) {
  const { t } = useTranslation();
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        aria-label={`${label}: ${summary}`}
        className={cn(
          fieldClass,
          "flex h-7 items-center justify-between gap-2 whitespace-nowrap text-start text-body data-[state=open]:border-ring data-[state=open]:ring-[3px] data-[state=open]:ring-ring/20",
          className,
        )}
      >
        <span className="truncate">{summary}</span>
        <ChevronDown
          aria-hidden="true"
          className="h-4 w-4 shrink-0 opacity-50"
        />
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="start"
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

/**
 * 用量「设置」页签顶部的「显示」：币种与手填汇率（只改显示，库里照旧存美元）、
 * 侧栏「今日」显示什么、请求日志显示哪些列、趋势图用柱形图还是曲线面积图。
 */
export function UsageDisplaySection({
  value,
  onChange,
}: UsageDisplaySectionProps) {
  const { t, i18n } = useTranslation();
  const lang = getResolvedLang(i18n);
  const rateId = useId();
  const savedCurrency = resolveMoneyDisplay(value).currency;
  // 保存要等设置重新读回来才生效，先按用户刚选的显示，免得下拉闪回旧值
  const [currency, setCurrency] = useState<DisplayCurrency>(savedCurrency);
  useEffect(() => setCurrency(savedCurrency), [savedCurrency]);

  // 选中币种的汇率（填过的，或预置值）；切币种时输入框跟着换
  const rate = resolveMoneyDisplay({ ...value, currency }).rate;
  const [rateDraft, setRateDraft] = useState(String(rate));
  const [rateInvalid, setRateInvalid] = useState(false);
  useEffect(() => {
    setRateDraft(String(rate));
    setRateInvalid(false);
  }, [currency, rate]);

  // 每次读出来都是新数组，按内容判断设置有没有变
  const savedTodayItems = resolveSidebarToday(value);
  const savedTodayKey = savedTodayItems.join(",");
  const [todayItems, setTodayItems] =
    useState<SidebarTodayItem[]>(savedTodayItems);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => setTodayItems(savedTodayItems), [savedTodayKey]);

  const save = (next: Partial<UsageDisplaySettings>) =>
    onChange({ ...value, ...next });

  const toggleTodayItem = async (item: SidebarTodayItem) => {
    // 按菜单顺序存，侧栏上也按这个顺序排
    const next = SIDEBAR_TODAY_ITEMS.filter((i) =>
      i === item ? !todayItems.includes(i) : todayItems.includes(i),
    );
    if (next.length > SIDEBAR_TODAY_MAX_ITEMS) return;
    setTodayItems(next);
    if ((await save({ sidebarTodayItems: next })) === false) {
      setTodayItems(savedTodayItems);
    }
  };
  const todayLabel = (item: SidebarTodayItem) =>
    t(`usage.display.sidebarTodayItems.${item}`);
  const todayFull = todayItems.length >= SIDEBAR_TODAY_MAX_ITEMS;

  // 请求日志的列：只存改过的列；保存要等设置读回来，先按刚勾的显示
  const savedColumns = value?.requestLogColumns;
  const [columnOverrides, setColumnOverrides] = useState(savedColumns ?? {});
  useEffect(() => setColumnOverrides(savedColumns ?? {}), [savedColumns]);
  const logColumns = resolveRequestLogColumns(columnOverrides);
  const shownToggleable = TOGGLEABLE_REQUEST_LOG_COLUMNS.filter(({ id }) =>
    logColumns.includes(id),
  ).length;

  const saveColumnOverrides = async (next: Record<string, boolean>) => {
    setColumnOverrides(next);
    if ((await save({ requestLogColumns: next })) === false) {
      setColumnOverrides(savedColumns ?? {});
    }
  };

  const savedChartType = resolveTrendChartType(value);
  const [chartType, setChartType] = useState<TrendChartType>(savedChartType);
  useEffect(() => setChartType(savedChartType), [savedChartType]);

  const changeChartType = async (next: TrendChartType) => {
    setChartType(next);
    if ((await save({ trendChartType: next })) === false) {
      setChartType(savedChartType);
    }
  };

  const changeCurrency = async (next: DisplayCurrency) => {
    setCurrency(next);
    // 保存失败（useSettings 已提示）就退回原来的币种
    if ((await save({ currency: next })) === false) {
      setCurrency(savedCurrency);
    }
  };

  const commitRate = () => {
    if (currency === "USD") return;
    const parsed = Number(rateDraft.trim());
    if (!isValidExchangeRate(parsed)) {
      setRateInvalid(true);
      return;
    }
    setRateInvalid(false);
    // 没改就不写：没动过的预置汇率不落进设置文件，以后调整预置值还能跟上
    if (parsed === rate) return;
    void save({
      exchangeRates: { ...value?.exchangeRates, [currency]: parsed },
    });
  };

  const optionLabel = (code: DisplayCurrency) => {
    const name = currencyName(code, lang);
    return name ? `${code} · ${name}` : code;
  };

  return (
    <section
      aria-labelledby={`${rateId}-title`}
      className="flex flex-col gap-2"
    >
      <h3
        id={`${rateId}-title`}
        className="m-0 text-strong font-semibold text-fg-1"
      >
        {t("usage.display.title")}
      </h3>
      <div className="rounded-panel bg-subtle px-4 py-2.5">
        {/* 两列「标签 + 控件」：标签列取各自最宽的标签，控件一律同宽，上下左右都对齐 */}
        <div className="grid grid-cols-[max-content_11rem] items-center gap-x-3 gap-y-2 md:grid-cols-[max-content_11rem_max-content_11rem]">
          <span className="text-body text-fg-2">
            {t("usage.display.currency")}
          </span>
          <Select
            value={currency}
            onValueChange={(next) => {
              // Radix Select 偶尔回报空串，忽略
              const picked = DISPLAY_CURRENCIES.find((c) => c === next);
              if (picked && picked !== currency) {
                void changeCurrency(picked);
              }
            }}
          >
            <SelectTrigger
              className="h-7 w-full text-body"
              aria-label={t("usage.display.currency")}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {DISPLAY_CURRENCIES.map((code) => (
                <SelectItem key={code} value={code}>
                  {optionLabel(code)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <span className="text-body text-fg-2 md:ps-5">
            {t("usage.display.trendChartType")}
          </span>
          <Select
            value={chartType}
            onValueChange={(next) => {
              // Radix Select 偶尔回报空串，忽略
              const picked = TREND_CHART_TYPES.find((c) => c === next);
              if (picked && picked !== chartType) {
                void changeChartType(picked);
              }
            }}
          >
            <SelectTrigger
              className="h-7 w-full text-body"
              aria-label={t("usage.display.trendChartType")}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {TREND_CHART_TYPES.map((type) => (
                <SelectItem key={type} value={type}>
                  {t(`usage.display.trendChartTypes.${type}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          {currency !== "USD" && (
            <>
              <label htmlFor={rateId} className="text-body text-fg-2">
                {t("usage.display.rate")}
              </label>
              {/* 汇率紧跟币种单占一行，跨过后面所有列 */}
              <div className="col-span-1 flex items-center gap-2 text-body tabular-nums text-fg-2 md:col-span-3">
                <span className="shrink-0">1 USD =</span>
                <Input
                  id={rateId}
                  inputMode="decimal"
                  className="h-7 w-24 tabular-nums"
                  value={rateDraft}
                  aria-invalid={rateInvalid}
                  onChange={(event) => {
                    setRateDraft(event.target.value);
                    setRateInvalid(false);
                  }}
                  onBlur={commitRate}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") commitRate();
                  }}
                />
                <span className="shrink-0">{currency}</span>
              </div>
            </>
          )}
          <span className="text-body text-fg-2">
            {t("usage.display.sidebarToday")}
          </span>
          <ChecklistMenu
            label={t("usage.display.sidebarToday")}
            className="w-full"
            summary={
              todayItems.length > 0
                ? todayItems.map(todayLabel).join(" + ")
                : t("usage.display.sidebarTodayItems.none")
            }
            items={SIDEBAR_TODAY_ITEMS.map((item) => {
              const checked = todayItems.includes(item);
              return {
                id: item,
                label: todayLabel(item),
                checked,
                disabled: todayFull && !checked,
              };
            })}
            note={t("usage.display.sidebarTodayLimit", {
              count: SIDEBAR_TODAY_MAX_ITEMS,
            })}
            onToggle={(id) => void toggleTodayItem(id as SidebarTodayItem)}
          />
          <span className="text-body text-fg-2 md:ps-5">
            {t("usage.display.logColumns")}
          </span>
          <ChecklistMenu
            label={t("usage.display.logColumns")}
            className="w-full"
            summary={t("usage.display.logColumnsSummary", {
              shown: shownToggleable,
              total: TOGGLEABLE_REQUEST_LOG_COLUMNS.length,
            })}
            items={TOGGLEABLE_REQUEST_LOG_COLUMNS.map(({ id }) => ({
              id,
              label: t(REQUEST_LOG_COLUMN_LABEL_KEYS[id]),
              checked: logColumns.includes(id),
            }))}
            onToggle={(id) => {
              const column = id as RequestLogColumnId;
              void saveColumnOverrides(
                setRequestLogColumn(
                  columnOverrides,
                  column,
                  !logColumns.includes(column),
                ),
              );
            }}
            onReset={
              Object.keys(columnOverrides).length > 0
                ? () => void saveColumnOverrides({})
                : undefined
            }
          />
        </div>
        {currency !== "USD" && (
          <p
            className={
              rateInvalid
                ? "m-0 mt-1.5 text-caption text-danger-text"
                : "m-0 mt-1.5 text-caption text-fg-3"
            }
          >
            {rateInvalid
              ? t("usage.display.rateInvalid")
              : // 两句各自带句号：中日文直接相连，其他语言隔一个空格
                [
                  t("usage.display.rateHint"),
                  t("usage.display.approxNote"),
                ].join(/^(zh|ja)/i.test(lang) ? "" : " ")}
          </p>
        )}
      </div>
    </section>
  );
}
