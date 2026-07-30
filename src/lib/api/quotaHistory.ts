import { invoke } from "@tauri-apps/api/core";

/** 一行历史：某应用某小时某窗口的读数（后端按 hour 升序返回） */
export interface QuotaHistoryRow {
  appId: string;
  /** 纪元小时序号 = floor(measuredAt / 3_600_000) */
  hour: number;
  tier: string;
  utilization: number;
  usedUsd: number | null;
  maxUsd: number | null;
}

/** 送给后端的单个窗口读数 */
export interface QuotaTierSample {
  name: string;
  utilization: number;
  usedUsd: number | null;
  maxUsd: number | null;
}

export const quotaHistoryApi = {
  /** 返回 false 表示这次观察没带来新信息（旧读数或数值相同） */
  record: (
    appId: string,
    measuredAt: number,
    tiers: QuotaTierSample[],
  ): Promise<boolean> =>
    invoke("record_quota_history", { appId, measuredAt, tiers }),

  /** appId 传 null 则返回全部应用 */
  query: (
    appId: string | null,
    startHour: number,
    endHour: number,
  ): Promise<QuotaHistoryRow[]> =>
    invoke("get_quota_history", { appId, startHour, endHour }),
};
