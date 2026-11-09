import { useSettingsQuery } from "@/lib/query/queries";
import type { QuotaDisplay } from "./quotaRules";

/** 设置里「额度显示」选的是剩余还是已用；设置还没读到时按剩余 */
export function useQuotaDisplay(): QuotaDisplay {
  const { data } = useSettingsQuery();
  return data?.quotaDisplay === "used" ? "used" : "left";
}
