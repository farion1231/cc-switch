import { useMemo } from "react";
import {
  formatMoney,
  formatMoneyCompact,
  formatMoneyTick,
  USD_DISPLAY,
} from "@/components/usage/format";

/**
 * 用量金额的统一出口：用量面板、侧栏「今日」、会话阅读页的费用都经这里显示。
 * 定价（按美元录入）和额度 / 余额（供应商自己的币种）不走这里。
 */
export function useMoneyFormat() {
  const display = USD_DISPLAY;

  return useMemo(
    () => ({
      money: (usd: unknown, digits: number, fallback?: string) =>
        formatMoney(usd, digits, display, fallback),
      moneyCompact: (usd: number) => formatMoneyCompact(usd, display),
      moneyTick: (usd: unknown) => formatMoneyTick(usd, display),
    }),
    [display],
  );
}
