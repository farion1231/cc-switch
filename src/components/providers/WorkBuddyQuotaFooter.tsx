import { useTranslation } from "react-i18next";
import type { Provider } from "@/types";
import { QuotaLines } from "@/components/quota/QuotaLines";
import { useQuotaDisplay } from "@/components/quota/useQuotaDisplay";
import { boundWorkBuddyAccountId } from "./workbuddyOfficial";
import { useWorkBuddyAccountQuota, workbuddyCardLines } from "./workbuddyQuota";

/**
 * WorkBuddy 官方登录卡右侧的额度：绑定账号的积分（剩余 / 总量）。和授权中心那一行共用一份查询；
 * WorkBuddy 没在运行或不是当前账号时显示上次结果和更新时间。
 */
export function WorkBuddyQuotaFooter({ provider }: { provider: Provider }) {
  const { t, i18n } = useTranslation();
  const display = useQuotaDisplay();
  const accountId = boundWorkBuddyAccountId(provider);
  const { data, isFetching, refresh } = useWorkBuddyAccountQuota(accountId, {
    autoRefresh: true,
  });
  const lines = workbuddyCardLines(t, data, i18n.language, display);
  if (!lines) return null;
  return (
    <QuotaLines
      lines={lines}
      queriedAt={data?.snapshot?.queriedAt ?? null}
      loading={isFetching}
      onRefresh={refresh}
    />
  );
}
