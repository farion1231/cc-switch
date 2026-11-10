import { RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  useIsFetching,
  useQueryClient,
  type Query,
} from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { DSH_ACCOUNT_STATUS_KEY } from "@/components/providers/DshAccount";

/** 授权中心各组账号行的额度缓存：`[服务, "quota", 账号]` */
const QUOTA_SERVICES = new Set([
  "claude_official",
  "copilot",
  "codex_oauth",
  "xai_oauth",
  "kimi_oauth",
  "agy_oauth",
]);

/**
 * 当前页面上（有组件在用）的账号额度。只手动查询时这些 query 是 disabled 的，
 * `queryClient.refetchQueries` 会把 disabled 的过滤掉，所以要直接对 query 调 `fetch()`。
 */
function isShownAccountQuotaQuery(query: Query): boolean {
  return query.getObserversCount() > 0 && isAccountQuotaQuery(query);
}

function isAccountQuotaQuery({ queryKey }: Query): boolean {
  const [service, kind] = queryKey;
  if (service === DSH_ACCOUNT_STATUS_KEY[0]) return true;
  return (
    typeof service === "string" &&
    QUOTA_SERVICES.has(service) &&
    kind === "quota"
  );
}

/**
 * 授权中心页头的「全部刷新」：重查当前页面上所有账号行的额度（DeepSeek 是余额），
 * 跟每行自己的 ↻ 走同一份缓存，所以各行也会一起转圈。
 */
export function RefreshAllQuotaButton() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const fetching = useIsFetching({ predicate: isAccountQuotaQuery }) > 0;

  return (
    <Button
      type="button"
      variant="quiet"
      size="regular"
      disabled={fetching}
      onClick={() =>
        void Promise.all(
          queryClient
            .getQueryCache()
            .findAll({ predicate: isShownAccountQuotaQuery })
            .map((query) => query.fetch().catch(() => undefined)),
        )
      }
    >
      <RefreshCw
        className={cn("h-4 w-4", fetching && "motion-safe:animate-spin")}
        strokeWidth={2}
      />
      {t("authCenter.quota.refreshAll", { defaultValue: "全部刷新" })}
    </Button>
  );
}
