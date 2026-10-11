import { useEffect, useState } from "react";

import { readProviderIcon } from "@/lib/api/providerIcons";

/**
 * 解析本地图标资产键为可渲染的 data: URL（issue #8117）。
 *
 * 解析结果按键缓存（模块级 Map），同一资产在列表、卡片、表单间只读盘一次；
 * 资产缺失或读取失败返回 null，调用方沿用内建图标 → 首字母的回退链。
 */
export function useProviderIconSrc(key?: string): string | null {
  const [src, setSrc] = useState<string | null>(() =>
    key ? (cache.get(key) ?? null) : null,
  );

  useEffect(() => {
    if (!key) {
      setSrc(null);
      return;
    }
    const cached = cache.get(key);
    if (cached) {
      setSrc(cached);
      return;
    }

    let cancelled = false;
    let promise = inflight.get(key);
    if (!promise) {
      promise = readProviderIcon(key)
        .then((url) => {
          if (url) cache.set(key, url);
          return url;
        })
        .catch(() => null)
        .finally(() => {
          inflight.delete(key);
        });
      inflight.set(key, promise);
    }
    void promise.then((url) => {
      if (!cancelled) setSrc(url);
    });
    return () => {
      cancelled = true;
    };
  }, [key]);

  return src;
}

export function clearProviderIconSrcCache(key?: string): void {
  if (key) {
    cache.delete(key);
  } else {
    cache.clear();
  }
}

const cache = new Map<string, string>();
const inflight = new Map<string, Promise<string | null>>();
