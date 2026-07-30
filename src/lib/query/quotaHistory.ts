import { useQuery } from "@tanstack/react-query";
import { quotaHistoryApi } from "@/lib/api/quotaHistory";

export const quotaHistoryKeys = {
  all: ["quotaHistory"] as const,
  range: (startHour: number, endHour: number) =>
    [...quotaHistoryKeys.all, startHour, endHour] as const,
};

/**
 * 读取 `[startHour, endHour]` 内所有应用的额度历史。
 *
 * key 用**小时序号**而不是秒级时间戳：面板的范围终点是「现在」，秒级的话每次
 * 渲染都会换 key、无限重查。按小时量化后一小时内 key 稳定，正好也是探针的写入
 * 节奏。数据由探针写入后主动 invalidate，所以不需要轮询。
 */
export function useQuotaHistory(startHour: number, endHour: number) {
  return useQuery({
    queryKey: quotaHistoryKeys.range(startHour, endHour),
    queryFn: () => quotaHistoryApi.query(null, startHour, endHour),
    staleTime: 60 * 60 * 1000,
  });
}
