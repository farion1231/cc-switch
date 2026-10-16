import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Input } from "@/components/ui/input";
import { SegmentedControl } from "@/components/ui/segmented-control";
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
  SIDEBAR_TODAY_MODES,
  type DisplayCurrency,
  type SidebarTodayMode,
} from "./format";

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
 * 用量「设置」页签顶部的「显示」：币种与手填汇率（只改显示，库里照旧存美元）、
 * 侧栏「今日」显示什么。
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

  const savedTodayMode = resolveSidebarToday(value);
  const [todayMode, setTodayMode] = useState<SidebarTodayMode>(savedTodayMode);
  useEffect(() => setTodayMode(savedTodayMode), [savedTodayMode]);

  const save = (next: Partial<UsageDisplaySettings>) =>
    onChange({ ...value, ...next });

  const changeTodayMode = async (next: SidebarTodayMode) => {
    setTodayMode(next);
    if ((await save({ sidebarToday: next })) === false) {
      setTodayMode(savedTodayMode);
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
      <div className="flex flex-col rounded-panel bg-subtle px-4">
        <div className="py-2.5">
          <div className="flex flex-wrap items-center gap-x-6 gap-y-2">
            <div className="flex items-center gap-2">
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
                  className="h-7 w-48 text-body"
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
            </div>
            {currency !== "USD" && (
              <div className="flex items-center gap-2 text-body tabular-nums text-fg-2">
                <label htmlFor={rateId} className="shrink-0">
                  {t("usage.display.rate")}
                </label>
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
            )}
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
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2 border-t border-border py-2.5">
          <span className="text-body text-fg-2">
            {t("usage.display.sidebarToday")}
          </span>
          <SegmentedControl<SidebarTodayMode>
            size="sm"
            aria-label={t("usage.display.sidebarToday")}
            value={todayMode}
            onValueChange={(next) => void changeTodayMode(next)}
            items={SIDEBAR_TODAY_MODES.map((mode) => ({
              value: mode,
              label: t(`usage.display.sidebarTodayModes.${mode}`),
            }))}
          />
        </div>
      </div>
    </section>
  );
}
