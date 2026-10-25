import { HuaweiModelQuotaUsage } from "./HuaweiModelQuotaUsage";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { fetchHuaweiQuota } from "./huaweiCodeAgent2Api";

export function HuaweiQuotaUsage({ accountId }: { accountId: string | null }) {
  const { t } = useTranslation();
  const query = useQuery({
    queryKey: ["huawei-quota", accountId],
    queryFn: () => fetchHuaweiQuota(accountId),
    enabled: false,
    retry: false,
  });
  const labels: Record<string, string> = {
    CPM: t("huaweiCodeAgent2.monthlyQuota", { defaultValue: "月度额度" }),
    CPD_Peak: t("huaweiCodeAgent2.peakQuota", { defaultValue: "每日高峰额度" }),
    CPD_Valley: t("huaweiCodeAgent2.valleyQuota", {
      defaultValue: "每日低谷额度",
    }),
  };
  const number = (value: number | string) => {
    const n = Number(value);
    return Number.isFinite(n)
      ? n.toLocaleString(undefined, { maximumFractionDigits: 2 })
      : "—";
  };
  const groups = query.data
    ? ([
        [
          t("huaweiCodeAgent2.totalQuota", { defaultValue: "总额度" }),
          query.data.quotas,
        ],
        [
          t("huaweiCodeAgent2.externalQuota", { defaultValue: "外部模型额度" }),
          query.data.external_quotas,
        ],
        [
          t("huaweiCodeAgent2.internalQuota", { defaultValue: "内部模型额度" }),
          query.data.internal_quotas,
        ],
      ] as const)
    : [];
  return (
    <div className="space-y-2 rounded-md border p-3 text-sm">
      <Button
        type="button"
        variant="outline"
        size="sm"
        disabled={query.isFetching}
        onClick={() => void query.refetch()}
      >
        {query.isFetching
          ? t("huaweiCodeAgent2.queryingQuota", { defaultValue: "正在查询…" })
          : t("huaweiCodeAgent2.queryQuota", {
              defaultValue: "查询华为额度用量",
            })}
      </Button>
      {query.isError && (
        <p role="alert" className="text-destructive">
          {String(query.error)}
        </p>
      )}
      {query.data && (
        <>
          <p className="text-xs text-muted-foreground">
            {t("huaweiCodeAgent2.quotaUnit", {
              defaultValue: "华为额度：已用 / 总量（不是 Token 数或费用）",
            })}
          </p>
          {groups.map(
            ([label, quotas]) =>
              quotas &&
              quotas.length > 0 && (
                <div key={label} className="space-y-1">
                  <p className="font-medium">{label}</p>
                  {quotas.map((quota, index) => (
                    <div key={`${quota.type}-${index}`}>
                      <div className="flex justify-between gap-3">
                        <span>{labels[quota.type] ?? quota.type}</span>
                        <span>
                          {number(quota.used_quota)} /{" "}
                          {number(quota.total_quota)}
                        </span>
                      </div>
                      {quota.reset_at && (
                        <p className="text-xs text-muted-foreground">
                          {t("huaweiCodeAgent2.quotaReset", {
                            defaultValue: "重置时间",
                          })}
                          : {quota.reset_at}
                        </p>
                      )}
                    </div>
                  ))}
                </div>
              ),
          )}
          {query.data.message_cn && <p>{query.data.message_cn}</p>}
          <p className="text-xs text-muted-foreground">
            {t("huaweiCodeAgent2.quotaUpdated", { defaultValue: "查询时间" })}:{" "}
            {new Date(query.dataUpdatedAt).toLocaleString()}
          </p>
        </>
      )}
      <HuaweiModelQuotaUsage
        key={accountId ?? "default"}
        accountId={accountId}
      />
    </div>
  );
}
