import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { useIsFetching, useQueryClient } from "@tanstack/react-query";
import { RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { HoverTip } from "@/components/ui/hover-tip";
import { forceRefreshCodexResets } from "@/lib/query/subscription";
import { cn } from "@/lib/utils";

/** 供应商页所有额度类 query 的 key 前缀；重置预测（绕过缓存）单独 force */
const QUOTA_QUERY_KEYS = [
  ["subscription"],
  ["codex_oauth"],
  ["xai_oauth"],
  ["agy_oauth"],
  ["kimi_oauth"],
  ["copilot"],
  ["usage", "script"],
] as const;

/** query 的 key 是否以上面某个前缀开头（useIsFetching 只认单个 key，用 predicate 合并） */
function isQuotaQueryKey(queryKey: readonly unknown[]): boolean {
  return QUOTA_QUERY_KEYS.some((prefix) =>
    prefix.every((part, index) => queryKey[index] === part),
  );
}

/**
 * 供应商页头的「刷新所有额度」按钮：把列表里各供应商的额度 / 用量查询全部
 * 标记失效并重查（当前挂载且启用的查询立即发起；未启用的等启用时再查），
 * 并强制绕过后端缓存刷新一次社区重置预测。图标随相关查询进行中而旋转。
 */
export function RefreshAllQuotaButton() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const fetching = useIsFetching({
    predicate: (query) => isQuotaQueryKey(query.queryKey),
  });

  const handleRefresh = useCallback(() => {
    for (const key of QUOTA_QUERY_KEYS) {
      void queryClient.invalidateQueries({ queryKey: [...key] });
    }
    void forceRefreshCodexResets(queryClient);
  }, [queryClient]);

  return (
    <HoverTip content={t("provider.refreshQuotaTip")}>
      <Button
        variant="quiet"
        size="icon-compact"
        className="h-8 w-8"
        aria-label={t("provider.refreshQuotaAriaLabel")}
        onClick={handleRefresh}
      >
        <RefreshCw
          className={cn("h-4 w-4", fetching > 0 && "motion-safe:animate-spin")}
        />
      </Button>
    </HoverTip>
  );
}
