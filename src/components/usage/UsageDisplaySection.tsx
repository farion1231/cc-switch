import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { UsageDisplaySettings } from "@/types";
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
import { ChecklistMenu } from "./ChecklistMenu";
import { settingsCardClass, settingsGroupTitleClass } from "./settingsStyles";

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

/**
 * 用量设置抽屉里的「显示」：币种与手填汇率（只改显示，库里照旧存美元）、
 * 侧栏「今日」显示什么、趋势图用柱形图还是曲线面积图。请求日志显示哪些列在表格工具栏里改。
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
      <h3 id={`${rateId}-title`} className={settingsGroupTitleClass}>
        {t("usage.display.title")}
      </h3>
      {/* 和数据来源、定价同一种卡片（描边、无底色），不再用灰底块 */}
      <div className={cn(settingsCardClass, "gap-0")}>
        {/* 两列「标签 + 控件」：标签列取各自最宽的标签，控件一律同宽，上下左右都对齐 */}
        <div className="grid grid-cols-[max-content_minmax(0,1fr)] items-center gap-x-3 gap-y-2">
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
          <span className="text-body text-fg-2">
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
              <div className="flex items-center gap-2 text-body tabular-nums text-fg-2">
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
