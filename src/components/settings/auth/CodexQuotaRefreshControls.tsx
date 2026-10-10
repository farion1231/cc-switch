import { useId, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { RefreshCw } from "lucide-react";
import { authApi, settingsApi } from "@/lib/api";
import { subscriptionApi } from "@/lib/api/subscription";
import { useSettingsQuery } from "@/lib/query/queries";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { extractErrorMessage } from "@/utils/errorUtils";

const intervals = [0, 1, 5, 15, 30, 60];

/** Batch requests run in the backend, including when this panel is closed. */
export function CodexQuotaRefreshControls() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const selectId = useId();
  const hintId = useId();
  const [message, setMessage] = useState("");
  const [saveError, setSaveError] = useState("");
  const settings = useSettingsQuery();
  const status = useQuery({
    queryKey: ["managed-auth-status", "codex_oauth"],
    queryFn: () => authApi.authGetStatus("codex_oauth"),
    staleTime: 30_000,
  });
  const hasEligibleAccounts =
    status.isSuccess &&
    status.data.accounts.some((account) => !account.reauth_required);
  const refresh = useMutation({
    mutationFn: subscriptionApi.refreshCodexOauthQuotas,
    onMutate: () => setMessage(""),
    onSuccess: (results) => {
      // Account events publish immediately; a completed batch may contain older
      // results than a subsequent individual refresh or account removal.
      const succeeded = results.filter(
        (result) => result.quota?.success && !result.error,
      ).length;
      const failed = results.length - succeeded;
      setMessage(
        results.length === 0
          ? t("codexOauth.quotaRefresh.noEligible")
          : failed > 0
            ? t("codexOauth.quotaRefresh.partial", { succeeded, failed })
            : t("codexOauth.quotaRefresh.complete", { count: succeeded }),
      );
      // Refresh local sign-in status, without invalidating per-account quota queries.
      void queryClient.invalidateQueries({
        queryKey: ["managed-auth-status", "codex_oauth"],
      });
    },
    onError: (error) =>
      setMessage(
        t("codexOauth.quotaRefresh.failed", {
          error: extractErrorMessage(error),
        }),
      ),
  });
  const save = useMutation({
    mutationFn: async (minutes: number) => {
      // Merge with freshly persisted settings so an old panel cannot overwrite other preferences.
      const current = await settingsApi.get();
      const next = { ...current, codexAccountQuotaRefreshMinutes: minutes };
      if (!(await settingsApi.save(next)))
        throw new Error("save_settings returned false");
      return next;
    },
    onMutate: () => setSaveError(""),
    onSuccess: (saved) => queryClient.setQueryData(["settings"], saved),
    onError: (error) =>
      setSaveError(
        t("codexOauth.quotaRefresh.saveFailed", {
          error: extractErrorMessage(error),
        }),
      ),
  });

  return (
    <div className="mt-2 space-y-2 rounded-lg border border-border px-3 py-2">
      <div className="flex flex-wrap items-center gap-3">
        <Button
          variant="outline"
          size="sm"
          disabled={!hasEligibleAccounts || refresh.isPending}
          onClick={() => refresh.mutate()}
        >
          <RefreshCw
            className={
              refresh.isPending
                ? "mr-1.5 h-3.5 w-3.5 animate-spin"
                : "mr-1.5 h-3.5 w-3.5"
            }
          />
          {t(
            refresh.isPending
              ? "codexOauth.quotaRefresh.refreshing"
              : "codexOauth.quotaRefresh.refreshAll",
          )}
        </Button>
        <label className="text-caption text-fg-2" htmlFor={selectId}>
          {t("codexOauth.quotaRefresh.background")}
        </label>
        <Select
          value={String(settings.data?.codexAccountQuotaRefreshMinutes ?? 0)}
          disabled={!settings.isSuccess || save.isPending}
          onValueChange={(value) => save.mutate(Number(value))}
        >
          <SelectTrigger
            id={selectId}
            aria-describedby={hintId}
            className="w-auto min-w-28"
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {intervals.map((minutes) => (
              <SelectItem key={minutes} value={String(minutes)}>
                {t(
                  minutes === 0
                    ? "codexOauth.quotaRefresh.off"
                    : "codexOauth.quotaRefresh.interval",
                  { minutes },
                )}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <p id={hintId} className="text-caption text-fg-3">
        {t("codexOauth.quotaRefresh.hint")}
      </p>
      <p role="status" className="text-caption text-fg-2">
        {message ||
          (status.isPending
            ? t("codexOauth.statusLoading")
            : status.isError
              ? t("codexOauth.statusLoadFailed")
              : !hasEligibleAccounts
                ? t("codexOauth.quotaRefresh.noEligible")
                : "")}
      </p>
      {saveError && (
        <p role="alert" className="text-caption text-danger">
          {saveError}
        </p>
      )}
    </div>
  );
}
