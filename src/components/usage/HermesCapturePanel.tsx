import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { usageApi } from "@/lib/api/usage";
import { usageKeys } from "@/lib/query/usage";
import { toast } from "sonner";

interface Props {
  startMs: number;
  endMs: number;
  profileName?: string;
  task?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
}

export function HermesCapturePanel({
  startMs,
  endMs,
  profileName,
  task,
  providerName,
  model,
  refreshIntervalMs,
}: Props) {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const [replaying, setReplaying] = useState(false);
  const [enabling, setEnabling] = useState(false);
  const [page, setPage] = useState(0);
  useEffect(
    () => setPage(0),
    [startMs, endMs, profileName, task, providerName, model],
  );
  const { data: events = [], isLoading } = useQuery({
    queryKey: [
      ...usageKeys.all,
      "hermes-capture-events",
      startMs,
      endMs,
      profileName,
      task,
      providerName,
      model,
      page,
    ],
    queryFn: () =>
      usageApi.getHermesRequestEvents({
        startMs,
        endMs,
        profileName,
        task,
        providerName,
        model,
        offset: page * 100,
      }),
    refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
  });
  const { data: estimates = [] } = useQuery({
    queryKey: [...usageKeys.all, "hermes-history-estimates"],
    queryFn: usageApi.getHermesHistoryEstimates,
  });
  const selectedEstimates = profileName
    ? estimates.filter((row) => row.profileName === profileName)
    : estimates;
  const historicalCalls = selectedEstimates.reduce(
    (sum, row) => sum + row.requestCount,
    0,
  );
  const historicalTokens = selectedEstimates.reduce(
    (sum, row) =>
      sum +
      row.inputTokens +
      row.outputTokens +
      row.cacheReadTokens +
      row.cacheWriteTokens,
    0,
  );
  const historicalCost = selectedEstimates
    .reduce((sum, row) => sum + (Number(row.costUsd) || 0), 0)
    .toFixed(4);

  async function replay() {
    setReplaying(true);
    try {
      const result = await usageApi.replayHermesHistory();
      await queryClient.invalidateQueries({
        queryKey: [...usageKeys.all, "hermes-history-estimates"],
      });
      if (
        result.imported === 0 &&
        result.skipped === 0 &&
        result.unavailable === 0 &&
        result.errors.length === 0
      ) {
        toast.warning(t("usage.hermes.historyNotAvailable"));
        return;
      }
      const message = t("usage.hermes.historyReplayResult", {
        imported: result.imported,
        unavailable: result.unavailable,
        errors: result.errors.length,
      });
      if (result.errors.length || result.unavailable) toast.warning(message);
      else toast.success(message);
    } catch (error) {
      toast.error(String(error));
    } finally {
      setReplaying(false);
    }
  }

  async function enable() {
    setEnabling(true);
    try {
      const profile = await usageApi.enableHermesCapturePlugin(profileName);
      toast.success(t("usage.hermes.captureEnabled", { profile }));
    } catch (error) {
      toast.error(String(error));
    } finally {
      setEnabling(false);
    }
  }

  return (
    <div className="space-y-4" data-testid="hermes-capture-panel">
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-violet-500/20 bg-violet-500/5 p-4 text-sm">
        <div>
          <p className="font-medium">{t("usage.hermes.captureTitle")}</p>
          <p className="mt-1 text-muted-foreground">
            {t("usage.hermes.captureCoverage")}
          </p>
          <p className="mt-1 text-muted-foreground">
            {t("usage.hermes.captureSetup")}
          </p>
        </div>
        <Button variant="outline" onClick={enable} disabled={enabling}>
          {t("usage.hermes.enableCapture", {
            profile: profileName || "default",
          })}
        </Button>
      </div>
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border p-4 text-sm">
        <div>
          <p className="font-medium">{t("usage.hermes.historyEstimate")}</p>
          <p className="text-muted-foreground">
            {t("usage.hermes.historyEstimateSummary", {
              calls: historicalCalls.toLocaleString(i18n.language),
              tokens: historicalTokens.toLocaleString(i18n.language),
              cost: historicalCost,
              profiles: selectedEstimates.length,
            })}
          </p>
          <p className="text-xs text-muted-foreground">
            {t("usage.hermes.historyEstimateCaveat")}
          </p>
        </div>
        <Button variant="outline" onClick={replay} disabled={replaying}>
          {t("usage.hermes.replayHistory")}
        </Button>
      </div>
      <div className="overflow-x-auto rounded-lg border">
        <table className="w-full text-sm">
          <thead className="bg-muted/50 text-left">
            <tr>
              <th className="p-3">{t("usage.hermes.eventTime")}</th>
              <th className="p-3">{t("usage.hermes.eventRoute")}</th>
              <th className="p-3">{t("usage.hermes.eventModel")}</th>
              <th className="p-3">{t("usage.hermes.eventTokens")}</th>
              <th className="p-3">{t("usage.hermes.eventStatus")}</th>
              <th className="p-3">{t("usage.hermes.eventLatency")}</th>
            </tr>
          </thead>
          <tbody>
            {events.map((event) => (
              <tr
                key={`${event.profileName}:${event.eventId}`}
                className="border-t"
              >
                <td className="p-3 whitespace-nowrap">
                  {new Date(event.startedAtMs).toLocaleString(i18n.language)}
                </td>
                <td className="p-3">
                  {event.profileName} /{" "}
                  {event.kind === "aux" ? event.auxTask || "aux" : "main"}
                </td>
                <td className="p-3">
                  {event.provider} / {event.model}
                </td>
                <td className="p-3">
                  {event.usageAvailable
                    ? `${event.inputTokens ?? "?"} / ${event.outputTokens ?? "?"}`
                    : t("usage.hermes.unknownUsage")}
                </td>
                <td className="p-3">
                  {event.status}
                  {event.statusCode ? ` (${event.statusCode})` : ""}
                </td>
                <td className="p-3">
                  {event.durationMs == null ? "—" : `${event.durationMs} ms`}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {!isLoading && events.length === 0 && (
          <p className="p-6 text-center text-muted-foreground">
            {t("usage.hermes.noCapturedRequests")}
          </p>
        )}
      </div>
      <div className="flex items-center justify-end gap-2">
        <Button
          variant="outline"
          disabled={page === 0}
          onClick={() => setPage(page - 1)}
        >
          {t("usage.hermes.previousPage")}
        </Button>
        <span className="text-sm text-muted-foreground">{page + 1}</span>
        <Button
          variant="outline"
          disabled={events.length < 100}
          onClick={() => setPage(page + 1)}
        >
          {t("usage.hermes.nextPage")}
        </Button>
      </div>
    </div>
  );
}
