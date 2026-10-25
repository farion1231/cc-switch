import { useId, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  fetchHuaweiCodeAgent2Models,
  fetchHuaweiModelQuota,
} from "./huaweiCodeAgent2Api";

export function HuaweiModelQuotaUsage({
  accountId,
}: {
  accountId: string | null;
}) {
  const { t } = useTranslation();
  const selectId = useId();
  const [modelId, setModelId] = useState("");
  const models = useQuery({
    queryKey: ["huawei-quota-models", accountId],
    queryFn: () => fetchHuaweiCodeAgent2Models(accountId),
    enabled: false,
    retry: false,
  });
  const usage = useQuery({
    queryKey: ["huawei-model-quota", accountId, modelId],
    queryFn: () => fetchHuaweiModelQuota(accountId, modelId),
    enabled: false,
    retry: false,
  });
  const labels: Record<string, string> = {
    TPD: t("huaweiCodeAgent2.dailyTokens", { defaultValue: "每日 Token" }),
    TPH: t("huaweiCodeAgent2.hourlyTokens", { defaultValue: "每小时 Token" }),
    TPM: t("huaweiCodeAgent2.minuteTokens", { defaultValue: "每分钟 Token" }),
  };
  const number = (value: number | string) => {
    const n = Number(value);
    return Number.isFinite(n)
      ? n.toLocaleString(undefined, { maximumFractionDigits: 2 })
      : "—";
  };
  const validModel =
    !!modelId && models.data?.some((model) => model.id === modelId);
  return (
    <section className="space-y-2 border-t pt-3">
      <p className="font-medium">
        {t("huaweiCodeAgent2.modelQuotaTitle", {
          defaultValue: "模型 Token 用量",
        })}
      </p>
      <Button
        type="button"
        variant="outline"
        size="sm"
        disabled={models.isFetching}
        onClick={() => void models.refetch()}
      >
        {models.isFetching
          ? t("huaweiCodeAgent2.loadingQuotaModels", {
              defaultValue: "正在加载模型…",
            })
          : t("huaweiCodeAgent2.loadQuotaModels", {
              defaultValue: "加载可查询模型",
            })}
      </Button>
      {models.isError && (
        <p role="alert" className="text-destructive">
          {String(models.error)}
        </p>
      )}
      {models.data && (
        <>
          <label htmlFor={selectId} className="block">
            {t("huaweiCodeAgent2.quotaModel", {
              defaultValue: "查询用量的模型",
            })}
          </label>
          <select
            id={selectId}
            className="h-9 w-full rounded-md border bg-background px-2"
            value={modelId}
            onChange={(event) => setModelId(event.target.value)}
          >
            <option value="">
              {t("huaweiCodeAgent2.chooseQuotaModel", {
                defaultValue: "请选择模型",
              })}
            </option>
            {models.data.map((model) => (
              <option key={model.id} value={model.id}>
                {model.id}
              </option>
            ))}
          </select>
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={!validModel || usage.isFetching}
            onClick={() => void usage.refetch()}
          >
            {usage.isFetching
              ? t("huaweiCodeAgent2.queryingQuota", {
                  defaultValue: "正在查询…",
                })
              : t("huaweiCodeAgent2.queryModelQuota", {
                  defaultValue: "查询模型用量",
                })}
          </Button>
        </>
      )}
      {validModel && usage.isError && (
        <p role="alert" className="text-destructive">
          {String(usage.error)}
        </p>
      )}
      {validModel && usage.data && (
        <div className="space-y-2">
          <p className="break-all text-xs text-muted-foreground">{modelId}</p>
          <p className="text-xs text-muted-foreground">
            {t("huaweiCodeAgent2.modelQuotaUnit", {
              defaultValue: "已用 / 总配额，以华为接口返回的统计范围为准",
            })}
          </p>
          {usage.data.quotas.length === 0 && (
            <p>
              {t("huaweiCodeAgent2.emptyModelQuota", {
                defaultValue: "该模型暂无配额数据",
              })}
            </p>
          )}
          {usage.data.quotas.map((quota, index) => (
            <div key={`${quota.type}-${index}`}>
              <div className="flex justify-between gap-3">
                <span>{labels[quota.type] ?? quota.type}</span>
                <span>
                  {number(quota.used_quota)} / {number(quota.total_quota)}
                </span>
              </div>
              {quota.remaining_quota !== undefined && (
                <p>
                  {t("huaweiCodeAgent2.remainingQuota", {
                    defaultValue: "剩余配额",
                  })}
                  : {number(quota.remaining_quota)}
                </p>
              )}
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
          {(usage.data.message_cn ||
            usage.data.message_ch ||
            usage.data.message_en) && (
            <p>
              {usage.data.message_cn ||
                usage.data.message_ch ||
                usage.data.message_en}
            </p>
          )}
          <p className="text-xs text-muted-foreground">
            {t("huaweiCodeAgent2.quotaUpdated", { defaultValue: "查询时间" })}:{" "}
            {new Date(usage.dataUpdatedAt).toLocaleString()}
          </p>
        </div>
      )}
    </section>
  );
}
