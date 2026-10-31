import { useEffect, useRef } from "react";
import { isTauri } from "@tauri-apps/api/core";
import {
  shouldRefreshRates,
  resolveMoneyDisplay,
} from "@/components/usage/format";
import { usageApi } from "@/lib/api";
import type { UsageDisplaySettings } from "@/types";

/**
 * 自动模式的汇率：缓存过期时取一次并写回设置。
 *
 * 只在使用量页挂载时尝试；失败不提示也不重试，继续用上次缓存或手填值，
 * 下次打开用量页再来。浏览器预览里没有 Tauri，跳过。
 */
export function useAutoExchangeRates(
  usageDisplay: UsageDisplaySettings | undefined,
  save: (
    usageDisplay: UsageDisplaySettings,
  ) => Promise<boolean> | boolean | void,
  enabled: boolean = true,
) {
  // 每次挂载只取一次，保存后设置会变，靠它挡住重复请求
  const tried = useRef(false);
  const saveRef = useRef(save);
  saveRef.current = save;
  // 请求期间用户可能改了币种 / 来源：保存时合并进最新设置，别写回发起请求时的旧值
  const displayRef = useRef(usageDisplay);
  displayRef.current = usageDisplay;

  useEffect(() => {
    if (!enabled || tried.current || !isTauri()) return;
    // 按美元显示时不用换算，别为此发请求
    if (resolveMoneyDisplay(usageDisplay).currency === "USD") return;
    if (!shouldRefreshRates(usageDisplay)) return;
    tried.current = true;
    void (async () => {
      try {
        const fetched = await usageApi.fetchExchangeRates();
        await saveRef.current({
          ...displayRef.current,
          autoExchangeRates: fetched.rates,
          rateFetchedAt: fetched.fetchedAt,
        });
      } catch {
        // 取不到就退回缓存或手填值，不打扰使用
      }
    })();
  }, [enabled, usageDisplay]);
}
