import { useSettingsQuery } from "./queries";

export interface QuotaPolicy {
  /**
   * 额度 hook 能不能自己发请求（挂载、定时、窗口聚焦、网络恢复）。设置还没读到时也先不发：
   * 开了「只手动查询」的人，启动那一下不能先漏出一次请求（读设置是本地调用，很快就有结果）。
   */
  autoAllowed: boolean;
  /** 授权中心「自动查询额度」关着（#8990）：只有点 ↻ /「全部刷新」才查 */
  manualOnly: boolean;
}

/**
 * 官方账号额度的自动查询策略。关掉自动查询后，react-query 的 refetch 不受 enabled 限制，
 * 手动刷新照常可用；缓存里已有的结果照常显示。
 */
export function useQuotaPolicy(): QuotaPolicy {
  const { data, isPending } = useSettingsQuery();
  const manualOnly = data?.officialQuotaManualOnly === true;
  return { autoAllowed: !isPending && !manualOnly, manualOnly };
}

/** 只手动查询、还没查过（也没在查）：界面放一句「点刷新查询额度」，不然这一格什么都没有 */
export function isManualIdle(
  policy: QuotaPolicy,
  query: { data: unknown; isFetching: boolean },
): boolean {
  return policy.manualOnly && query.data === undefined && !query.isFetching;
}
