import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Command,
  CommandEmpty,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { formatSessionTitle } from "@/components/sessions/utils";
import { useSessionUsageSummary } from "@/lib/query/usage";
import { useSessionsQuery } from "@/lib/query/queries";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";
import type { SessionMeta } from "@/types";
import {
  fmtInt,
  formatHitRatePercent,
  formatTokensCompact,
  getLocaleFromLanguage,
} from "./format";

const getSessionKey = (session: SessionMeta) =>
  JSON.stringify([session.providerId, session.sessionId]);

function SingleSessionSummary({ session }: { session: SessionMeta }) {
  const { t, i18n } = useTranslation();
  const { data, isLoading, isError, refetch } = useSessionUsageSummary(
    session.providerId,
    session.sessionId,
  );
  const { money } = useMoneyFormat();
  const locale = getLocaleFromLanguage(
    i18n.resolvedLanguage || i18n.language || "en",
  );

  return (
    <div className="flex flex-col gap-3 rounded-panel border border-border bg-surface p-4">
      <div className="min-w-0">
        <h3 className="m-0 truncate text-section text-fg-1">
          {formatSessionTitle(session)}
        </h3>
        {session.projectDir && (
          <p
            className="m-0 truncate text-caption text-fg-3"
            title={session.projectDir}
          >
            {session.projectDir}
          </p>
        )}
      </div>
      {isLoading && (
        <p role="status" className="m-0 text-body text-fg-2">
          {t("usage.singleSession.loading")}
        </p>
      )}
      {isError && (
        <div className="flex items-center gap-2 text-body text-danger-text">
          <span>{t("usage.singleSession.error")}</span>
          <Button
            type="button"
            variant="quiet"
            size="compact"
            onClick={() => void refetch()}
          >
            {t("usage.singleSession.retry")}
          </Button>
        </div>
      )}
      {data && data.totalRequests === 0 && (
        <p className="m-0 text-body text-fg-2">
          {t("usage.singleSession.noUsage")}
        </p>
      )}
      {data && data.totalRequests > 0 && (
        <dl className="m-0 grid grid-cols-2 gap-3 md:grid-cols-3">
          <div>
            <dt className="text-caption text-fg-3">
              {t("usage.totalRequests")}
            </dt>
            <dd className="m-0 text-section tabular-nums text-fg-1">
              {fmtInt(data.totalRequests, locale)}
            </dd>
          </div>
          <div>
            <dt className="text-caption text-fg-3">{t("usage.totalCost")}</dt>
            <dd className="m-0 text-section tabular-nums text-fg-1">
              {money(data.totalCost, 2)}
            </dd>
          </div>
          <div>
            <dt className="text-caption text-fg-3">
              {t("usage.singleSession.totalTokens")}
            </dt>
            <dd className="m-0 text-section tabular-nums text-fg-1">
              {formatTokensCompact(data.realTotalTokens, locale)}
            </dd>
          </div>
          <div>
            <dt className="text-caption text-fg-3">
              {t("usage.cacheReadTokens")}
            </dt>
            <dd className="m-0 text-section tabular-nums text-fg-1">
              {formatTokensCompact(data.totalCacheReadTokens, locale)}
            </dd>
          </div>
          <div>
            <dt className="text-caption text-fg-3">
              {t("usage.cacheHitRate")}
            </dt>
            <dd className="m-0 text-section tabular-nums text-fg-1">
              {formatHitRatePercent(data.cacheHitRate)}
            </dd>
          </div>
          <div>
            <dt className="text-caption text-fg-3">
              {t("usage.singleSession.successRate")}
            </dt>
            <dd className="m-0 text-section tabular-nums text-fg-1">
              {`${data.successRate.toFixed(1)}%`}
            </dd>
          </div>
        </dl>
      )}
      <p className="m-0 text-caption text-fg-3">
        {t("usage.singleSession.dataScopeHint")}
      </p>
    </div>
  );
}

export function SingleSessionUsagePanel() {
  const { t } = useTranslation();
  const { data, isLoading, isError } = useSessionsQuery();
  const [selectedKey, setSelectedKey] = useState("");
  const [open, setOpen] = useState(false);
  const sessions = useMemo(() => {
    const unique = new Map<string, SessionMeta>();
    for (const session of data ?? []) {
      const key = getSessionKey(session);
      if (!unique.has(key)) unique.set(key, session);
    }
    return Array.from(unique.values());
  }, [data]);
  const selectedSession = sessions.find(
    (session) => getSessionKey(session) === selectedKey,
  );

  return (
    <div className="flex flex-col gap-4 p-4">
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button
            type="button"
            variant="neutral"
            aria-expanded={open}
            className="min-w-[220px] max-w-full justify-between gap-2"
          >
            <span className="truncate">
              {selectedSession
                ? formatSessionTitle(selectedSession)
                : t("usage.singleSession.select")}
            </span>
            <ChevronDown className="h-3.5 w-3.5 shrink-0" />
          </Button>
        </PopoverTrigger>
        <PopoverContent
          align="start"
          className="w-[min(420px,calc(100vw-2rem))] p-0"
        >
          <Command>
            <CommandInput
              aria-label={t("usage.singleSession.search")}
              placeholder={t("usage.singleSession.search")}
            />
            <CommandList>
              {isLoading ? (
                <CommandEmpty>
                  {t("usage.singleSession.loadingSessions")}
                </CommandEmpty>
              ) : isError ? (
                <CommandEmpty>
                  {t("usage.singleSession.sessionsError")}
                </CommandEmpty>
              ) : sessions.length === 0 ? (
                <CommandEmpty>
                  {t("usage.singleSession.noSessions")}
                </CommandEmpty>
              ) : (
                <>
                  {sessions.map((session) => {
                    const appName =
                      APP_DISPLAY_NAME[
                        session.providerId as keyof typeof APP_DISPLAY_NAME
                      ] ?? session.providerId;
                    return (
                      <CommandItem
                        key={getSessionKey(session)}
                        value={`${formatSessionTitle(session)} ${session.sessionId} ${session.projectDir ?? ""} ${appName}`}
                        onSelect={() => {
                          setSelectedKey(getSessionKey(session));
                          setOpen(false);
                        }}
                      >
                        <span className="flex min-w-0 flex-col">
                          <span className="truncate">
                            {formatSessionTitle(session)}
                          </span>
                          <span className="truncate text-caption text-fg-3">
                            {appName} ·{" "}
                            {session.projectDir || session.sessionId}
                          </span>
                        </span>
                      </CommandItem>
                    );
                  })}
                  <CommandEmpty>
                    {t("usage.singleSession.noMatches")}
                  </CommandEmpty>
                </>
              )}
            </CommandList>
          </Command>
        </PopoverContent>
      </Popover>
      {isError && (
        <p role="alert" className="m-0 text-body text-danger-text">
          {t("usage.singleSession.sessionsError")}
        </p>
      )}
      {selectedSession ? (
        <SingleSessionSummary session={selectedSession} />
      ) : (
        <p className="m-0 text-body text-fg-2">
          {t("usage.singleSession.selectHint")}
        </p>
      )}
    </div>
  );
}
