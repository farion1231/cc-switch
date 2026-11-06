import { useCallback, useMemo, useRef, useSyncExternalStore } from "react";
import { useQueryClient, type QueryKey } from "@tanstack/react-query";

interface PlanCarrier {
  success?: boolean;
  planName?: string | null;
}

type PlanPicker = (data: unknown) => string | null | undefined;

const defaultPick: PlanPicker = (data) => (data as PlanCarrier).planName;

/**
 * 额度查询顺带带回的套餐：只读缓存、不发请求，额度查到才有。一次刷新失败时沿用上次
 * 查到的，免得额度还显示着上次的值、套餐却没了。`entries` 是「名字 → 查询键」。
 */
function useQuotaPlansByKey(
  entries: [string, QueryKey][],
  pick: PlanPicker,
): Record<string, string> {
  const queryClient = useQueryClient();
  const subscribe = useCallback(
    (onChange: () => void) => queryClient.getQueryCache().subscribe(onChange),
    [queryClient],
  );
  // 快照是字符串，缓存里别的查询变了也不会白白重渲染；null＝这次没查成功，沿用上次的
  const snapshot = useSyncExternalStore(subscribe, () =>
    JSON.stringify(
      entries.map(([name, key]): [string, string | null] => {
        const data = queryClient.getQueryData(key);
        if (!data || (data as PlanCarrier).success === false) {
          return [name, null];
        }
        return [name, pick(data)?.trim() || ""];
      }),
    ),
  );
  const lastRef = useRef<Record<string, string>>({});

  return useMemo(() => {
    const seen = JSON.parse(snapshot) as [string, string | null][];
    const plans: Record<string, string> = {};
    for (const [name, plan] of seen) {
      const shown = plan ?? lastRef.current[name];
      if (shown) plans[name] = shown;
    }
    lastRef.current = plans;
    return plans;
    // 名字已经在快照里，entries 换了数组引用不用重算
  }, [snapshot]);
}

/**
 * 授权中心账号行第二行的套餐：各服务的账号额度查询（`[service, "quota", accountId]`）
 * 顺带带回套餐。
 */
export function useAccountQuotaPlans(
  service: string,
  accountIds: string[],
  pick: PlanPicker = defaultPick,
): Record<string, string> {
  return useQuotaPlansByKey(
    accountIds.map((id) => [id, [service, "quota", id]]),
    pick,
  );
}

/** 供应商卡片副标题的套餐：读卡片额度那个查询的缓存；`queryKey` 为 null 时不显示 */
export function useQuotaPlan(
  queryKey: QueryKey | null,
  pick: PlanPicker = defaultPick,
): string | null {
  // 名字带上查询键：换了账号时不沿用上一个号的套餐
  const name = JSON.stringify(queryKey);
  const plans = useQuotaPlansByKey(queryKey ? [[name, queryKey]] : [], pick);
  return plans[name] ?? null;
}
