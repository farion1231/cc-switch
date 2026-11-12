import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ExternalLink, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useGlobalProxyUrl } from "@/hooks/useGlobalProxy";
import { diagnoseIp, type IpReport } from "@/lib/api/ipDiagnostics";
import { settingsApi } from "@/lib/api";
import { SettingsBlock } from "./SettingsLayout";

const COMMUNITY_CHECKS = [
  { key: "ipQuality", url: "https://ip.net.coffee/claude/" },
  { key: "browserCheck", url: "https://fuck-claude.vercel.app/" },
] as const;

export function IpDiagnostics() {
  const { t } = useTranslation();
  const { data: savedUrl, isLoading, isError } = useGlobalProxyUrl();
  const [route, setRoute] = useState("saved");
  const [customUrl, setCustomUrl] = useState("");
  const url = route === "saved" ? (savedUrl ?? "") : customUrl;

  return (
    <SettingsBlock title={t("settings.ipDiagnostics.title")}>
      <div className="space-y-4 rounded-panel border border-border bg-surface p-5">
        <p className="text-sm text-fg-2">{t("settings.ipDiagnostics.scope")}</p>
        <label className="block space-y-2 text-sm">
          <span>{t("settings.ipDiagnostics.route")}</span>
          <select
            className="w-full rounded-control border border-border bg-surface p-2"
            value={route}
            onChange={(event) => setRoute(event.target.value)}
          >
            <option value="saved">{t("settings.ipDiagnostics.saved")}</option>
            <option value="custom">{t("settings.ipDiagnostics.custom")}</option>
          </select>
        </label>
        {route === "custom" && (
          <Input
            type="password"
            autoComplete="off"
            aria-label={t("settings.ipDiagnostics.proxyUrl")}
            placeholder="http://proxy.example:8080 / socks5h://proxy.example:1080"
            value={customUrl}
            onChange={(event) => setCustomUrl(event.target.value)}
          />
        )}
        <p className="text-sm text-fg-2">
          {t(
            url.trim()
              ? "settings.ipDiagnostics.explicitRoute"
              : "settings.ipDiagnostics.systemRoute",
          )}
        </p>
        {isError && route === "saved" && (
          <p role="alert">{t("settings.ipDiagnostics.settingsError")}</p>
        )}
        <DiagnosticRun
          key={`${route}:${url}`}
          url={url}
          disabled={route === "saved" && (isLoading || isError)}
        />
      </div>
    </SettingsBlock>
  );
}

// Changing routes remounts the run so a late response cannot label a new route.
function DiagnosticRun({ url, disabled }: { url: string; disabled: boolean }) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<IpReport | null>(null);
  const [checkedAt, setCheckedAt] = useState("");
  const [error, setError] = useState("");
  const label = (key: string) => t(`settings.ipDiagnostics.${key}`);
  const unknown = label("unknown");

  const run = async () => {
    setBusy(true);
    setReport(null);
    setError("");
    try {
      setReport(await diagnoseIp(url));
      setCheckedAt(new Date().toLocaleString());
    } catch (reason) {
      setError(label(reason === "invalidProxy" ? "invalidProxy" : "failed"));
    } finally {
      setBusy(false);
    }
  };

  const openCommunityCheck = async (link: string) => {
    try {
      await settingsApi.openExternal(link);
    } catch {
      setError(label("openError"));
    }
  };

  const location = report?.location;
  const observedIps =
    report?.samples.flatMap((sample) => sample.ip ?? []) ?? [];
  const fields: [string, string | null | undefined][] = location
    ? [
        ["locationIp", location.ip],
        ["country", location.country],
        ["region", location.region],
        ["city", location.city],
        [
          "asn",
          location.connection?.asn != null
            ? `AS${location.connection.asn}`
            : null,
        ],
        ["isp", location.connection?.isp],
        ["organization", location.connection?.org],
        ["exitTimezone", location.timezone?.id],
        ["utcOffset", location.timezone?.utc],
      ]
    : [];

  return (
    <div className="space-y-4">
      <p className="text-sm text-fg-2">{label("privacy")}</p>
      <Button onClick={() => void run()} disabled={disabled || busy}>
        {busy && (
          <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
        )}
        {label(busy ? "running" : "run")}
      </Button>
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      {report && (
        <div className="space-y-3" aria-live="polite">
          <p className="text-xs text-fg-2">
            {label("checkedAt")}: {checkedAt}
          </p>
          <dl className="space-y-2 text-sm">
            {report.samples.map((sample) => (
              <div
                key={sample.source}
                className="flex flex-wrap justify-between gap-2"
              >
                <dt>{sample.source}</dt>
                <dd className="break-all font-mono">
                  {sample.ip ??
                    label(`errors.${sample.error ?? "unavailable"}`)}
                  {sample.ip && ` (${sample.latencyMs} ms)`}
                </dd>
              </div>
            ))}
          </dl>
          <p className="text-sm text-fg-2">
            {label(
              observedIps.length < 2
                ? "incomplete"
                : new Set(observedIps).size === 1
                  ? "consistent"
                  : "different",
            )}
          </p>
          {report.locationError && (
            <p role="status" className="text-sm">
              {label("locationError")}:{" "}
              {label(`errors.${report.locationError}`)}
            </p>
          )}
          <dl className="space-y-2 text-sm">
            {fields.map(([key, value]) => (
              <div key={key} className="flex flex-wrap justify-between gap-2">
                <dt>{label(key)}</dt>
                <dd className="break-all">{value || unknown}</dd>
              </div>
            ))}
            <div className="flex flex-wrap justify-between gap-2">
              <dt>{label("localTimezone")}</dt>
              <dd>
                {Intl.DateTimeFormat().resolvedOptions().timeZone || unknown}
              </dd>
            </div>
          </dl>
          {location && (
            <p className="text-xs text-fg-2">{label("locationSource")}</p>
          )}
        </div>
      )}
      <div className="space-y-2 border-t border-border pt-4">
        <h3 className="text-sm font-medium">{label("community")}</h3>
        <p className="text-sm text-fg-2">{label("browserRoute")}</p>
        <div className="flex flex-wrap gap-2">
          {COMMUNITY_CHECKS.map((check) => (
            <Button
              key={check.key}
              variant="outline"
              size="sm"
              onClick={() => void openCommunityCheck(check.url)}
            >
              {label(check.key)}
              <ExternalLink className="h-3.5 w-3.5" aria-hidden="true" />
            </Button>
          ))}
        </div>
      </div>
    </div>
  );
}
