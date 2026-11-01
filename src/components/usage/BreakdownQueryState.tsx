import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";

/** 切换筛选时不能把 keepPreviousData 挂在新范围的标题下。 */
export function BreakdownQueryState({
  loading,
  retry,
}: {
  loading?: boolean;
  retry: () => void;
}) {
  const { t } = useTranslation();
  return loading ? (
    <p role="status" className="m-0 py-10 text-center text-caption text-fg-3">
      {t("common.loading")}
    </p>
  ) : (
    <div
      role="alert"
      className="flex flex-col items-center gap-2 py-10 text-caption text-fg-3"
    >
      <span>{t("usage.cacheBreakdown.loadError")}</span>
      <Button type="button" variant="quiet" size="sm" onClick={retry}>
        {t("common.retry")}
      </Button>
    </div>
  );
}
