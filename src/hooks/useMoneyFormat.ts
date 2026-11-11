import { useMemo } from "react";
import {
  formatMoney,
  formatMoneyCompact,
  formatMoneyShort,
  formatMoneyTick,
  resolveMoneyDisplay,
  USD_DISPLAY,
} from "@/components/usage/format";
import { useSettingsQuery } from "@/lib/query";

/**
 * 用量金额的统一出口：用量面板、侧栏「今日」、会话阅读页的费用都经这里显示。
 * 定价（按美元录入）和额度 / 余额（供应商自己的币种）不走这里。
 */
export function useMoneyFormat() {
  const { data: settings } = useSettingsQuery();
  const { currency, rate } = resolveMoneyDisplay(settings?.usageDisplay);

  return useMemo(() => {
    const display = { currency, rate };
    const converted = currency !== "USD";
    return {
      currency,
      /** 1 美元折多少显示币种；画图要先把数据换算好，刻度才整齐 */
      rate,
      money: (usd: unknown, digits: number, fallback?: string) =>
        formatMoney(usd, digits, display, fallback),
      moneyCompact: (usd: number) => formatMoneyCompact(usd, display),
      moneyShort: (usd: unknown) => formatMoneyShort(usd, display),
      moneyTick: (value: unknown) => formatMoneyTick(value, display),
      /** 换算显示时悬停给出美元原值；按美元显示时没有，调用方别挂 title */
      usdHint: (usd: unknown, digits: number = 6) =>
        converted ? formatMoney(usd, digits, USD_DISPLAY) : undefined,
      /** 本来就有悬停提示的地方：换算时在后面补上美元原值 */
      moneyTitle: (usd: unknown, digits: number) =>
        converted
          ? `${formatMoney(usd, digits, display)} (${formatMoney(usd, digits, USD_DISPLAY)})`
          : formatMoney(usd, digits, display),
    };
  }, [currency, rate]);
}
