import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { UsageDisplaySettings } from "@/types";
import { usageApi } from "@/lib/api";
import { cn } from "@/lib/utils";
import {
  DISPLAY_CURRENCIES,
  formatRelativeTime,
  getResolvedLang,
  isValidExchangeRate,
  RATE_SOURCES,
  resolveMoneyDisplay,
  resolveRateProvider,
  resolveRateSource,
  resolveSidebarToday,
  resolveTrendChartType,
  shouldRefreshRates,
  type DisplayCurrency,
  type RateSource,
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
 * 用量设置抽屉里的「显示」：币种、汇率来源（自动获取每日汇率，或手填）与手填汇率
 * （只改显示，库里照旧存美元）、侧栏「今日」显示什么、趋势图用柱形图还是曲线面积图。
 * 请求日志显示哪些列在表格工具栏里改。
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

  // 异步保存（取汇率）可能在用户又改了币种 / 来源之后才回来，
  // 要用最新一版设置与最新的 onChange 做底，别把发起时的整份 value
  // 和当时的保存闭包（autoSaveSettings 会拿它闭包里的整份设置合并）写回去
  const valueRef = useRef(value);
  valueRef.current = value;
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  const save = (next: Partial<UsageDisplaySettings>) =>
    onChangeRef.current({ ...valueRef.current, ...next });

  // 请求在途时离开页面：这次汇率结果作废，不能再保存
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  // 汇率来源：保存要等设置重新读回来才生效，先按用户刚选的显示
  const savedRateSource = resolveRateSource(value);
  const [rateSource, setRateSource] = useState<RateSource>(savedRateSource);
  useEffect(() => setRateSource(savedRateSource), [savedRateSource]);

  const [rateFetching, setRateFetching] = useState(false);
  const [rateFetchFailed, setRateFetchFailed] = useState(false);

  const refreshRates = async () => {
    if (rateFetching) return;
    setRateFetching(true);
    setRateFetchFailed(false);
    try {
      const fetched = await usageApi.fetchExchangeRates();
      if (!mountedRef.current) return;
      const saved = await save({
        autoExchangeRates: fetched.rates,
        rateFetchedAt: fetched.fetchedAt,
        rateProvider: fetched.source,
      });
      if (mountedRef.current) setRateFetchFailed(saved === false);
    } catch {
      if (mountedRef.current) setRateFetchFailed(true);
    } finally {
      if (mountedRef.current) setRateFetching(false);
    }
  };

  // 打开用量页时已由 useAutoExchangeRates 自动取过，这里只在切到自动、或点「立即更新」时取
  const changeRateSource = async (next: RateSource) => {
    setRateSource(next);
    if ((await save({ rateSource: next })) === false) {
      setRateSource(savedRateSource);
      return;
    }
    if (next === "auto" && shouldRefreshRates({ ...value, rateSource: next })) {
      void refreshRates();
    }
  };

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
      return;
    }
    // 刚切到非美元且自动汇率过期：顺手取一次，不等下次打开用量页
    if (next !== "USD" && shouldRefreshRates(value)) {
      void refreshRates();
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

  // 各句自带句号：中日文直接相连，其他语言隔一个空格
  const clauseSeparator = /^(zh|ja)/i.test(lang) ? "" : " ";
  const rateProvider = resolveRateProvider(value?.rateProvider);

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
              <span className="text-body text-fg-2">
                {t("usage.display.rateSource")}
              </span>
              <Select
                value={rateSource}
                onValueChange={(next) => {
                  // Radix Select 偶尔回报空串，忽略
                  const picked = RATE_SOURCES.find((source) => source === next);
                  if (picked && picked !== rateSource) {
                    void changeRateSource(picked);
                  }
                }}
              >
                <SelectTrigger
                  className="h-7 w-full text-body"
                  aria-label={t("usage.display.rateSource")}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {RATE_SOURCES.map((source) => (
                    <SelectItem key={source} value={source}>
                      {t(`usage.display.rateSources.${source}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <label htmlFor={rateId} className="text-body text-fg-2">
                {t("usage.display.rate")}
              </label>
              {/* 汇率跟着来源单占一行：自动时只读，手动时可直接改 */}
              <div className="flex items-center gap-2 text-body tabular-nums text-fg-2">
                <span className="shrink-0">1 USD =</span>
                <Input
                  id={rateId}
                  inputMode="decimal"
                  className="h-7 w-24 tabular-nums"
                  value={rateDraft}
                  aria-invalid={rateInvalid}
                  // 自动模式下这一格只用来显示取到的汇率，改汇率要切到手动
                  readOnly={rateSource === "auto"}
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
                {rateSource === "auto" && (
                  <Button
                    type="button"
                    variant="quiet"
                    size="compact"
                    className="h-7 gap-1 px-2"
                    onClick={() => void refreshRates()}
                    disabled={rateFetching}
                  >
                    <RefreshCw
                      className={cn("size-3.5", rateFetching && "animate-spin")}
                    />
                    {t("usage.display.rateRefresh")}
                  </Button>
                )}
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
              rateInvalid || rateFetchFailed
                ? "m-0 mt-1.5 text-caption text-danger-text"
                : "m-0 mt-1.5 text-caption text-fg-3"
            }
          >
            {rateInvalid ? (
              t("usage.display.rateInvalid")
            ) : rateSource === "auto" ? (
              <>
                {[
                  rateFetchFailed
                    ? t("usage.display.rateAutoFailed")
                    : typeof value?.rateFetchedAt === "number"
                      ? t("usage.display.rateAutoUpdated", {
                          time: formatRelativeTime(value.rateFetchedAt, t),
                        })
                      : t("usage.display.rateAutoNever"),
                  t("usage.display.rateAutoHint"),
                ].join(clauseSeparator)}
                {clauseSeparator}
                {/* open.er-api.com 的免费条款要求在使用汇率的页面署名，链接到实际数据源 */}
                <a
                  href={rateProvider.url}
                  target="_blank"
                  rel="noreferrer"
                  className="underline underline-offset-2 hover:text-fg-2"
                >
                  {t("usage.display.rateAttribution", {
                    provider: rateProvider.name,
                  })}
                </a>
              </>
            ) : (
              [t("usage.display.rateHint"), t("usage.display.approxNote")].join(
                clauseSeparator,
              )
            )}
          </p>
        )}
      </div>
    </section>
  );
}
