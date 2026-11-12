import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ExternalLink, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useGlobalProxyUrl } from "@/hooks/useGlobalProxy";
import { diagnoseIp, type IpReport } from "@/lib/api/globalProxy";
import { settingsApi } from "@/lib/api";
import { SettingsBlock, SettingsRow } from "./SettingsLayout";

const IP_QUALITY_URL = "https://ip.net.coffee/claude/";
const key = (name: string) => `settings.ipDiagnostics.${name}`;

type Field = [name: string, value: string | null | undefined];

export function IpDiagnostics() {
  const { t } = useTranslation();
  const { data: savedUrl, isLoading, isError } = useGlobalProxyUrl();
  const [route, setRoute] = useState("saved");
  const [customUrl, setCustomUrl] = useState("");
  const url = route === "saved" ? (savedUrl ?? "") : customUrl;

  return (
    <SettingsBlock title={t(key("title"))}>
      <div className="space-y-4 rounded-panel border border-border bg-surface p-5">
        <p className="text-sm text-fg-2">{t(key("scope"))}</p>
        <SettingsRow
          className="p-0"
          label={t(key("route"))}
          htmlFor="ip-diagnostics-route"
          control={
            <Select value={route} onValueChange={setRoute}>
              <SelectTrigger id="ip-diagnostics-route" className="w-[200px]">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="saved">{t(key("saved"))}</SelectItem>
                <SelectItem value="custom">{t(key("custom"))}</SelectItem>
              </SelectContent>
            </Select>
          }
        />
        {route === "custom" && (
          <Input
            type="password"
            autoComplete="off"
            aria-label={t(key("proxyUrl"))}
            placeholder="http://proxy.example:8080 / socks5h://proxy.example:1080"
            value={customUrl}
            onChange={(event) => setCustomUrl(event.target.value)}
          />
        )}
        <p className="text-sm text-fg-2">
          {t(key(url.trim() ? "explicitRoute" : "systemRoute"))}
        </p>
        {isError && route === "saved" && (
          <p role="alert">{t(key("settingsError"))}</p>
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
  const [result, setResult] = useState<{
    report: IpReport;
    checkedAt: string;
  } | null>(null);
  const [error, setError] = useState("");
  const label = (name: string) => t(key(name));
  const unknown = label("unknown");

  const run = async () => {
    setBusy(true);
    setResult(null);
    setError("");
    try {
      const report = await diagnoseIp(url);
      setResult({ report, checkedAt: new Date().toLocaleString() });
    } catch (reason) {
      setError(label(reason === "invalidProxy" ? "invalidProxy" : "failed"));
    } finally {
      setBusy(false);
    }
  };

  const openIpQualityCheck = async () => {
    try {
      await settingsApi.openExternal(IP_QUALITY_URL);
    } catch {
      setError(label("openError"));
    }
  };

  const report = result?.report;
  const location = report?.location;
  const observedIps =
    report?.samples.flatMap((sample) => sample.ip ?? []) ?? [];
  const locationFields: Field[] = location
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
  const fields: Field[] = [
    ...locationFields,
    ["localTimezone", Intl.DateTimeFormat().resolvedOptions().timeZone],
  ];

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
      {result && (
        <div className="space-y-3" aria-live="polite">
          <p className="text-xs text-fg-2">
            {label("checkedAt")}: {result.checkedAt}
          </p>
          <dl className="space-y-2 text-sm">
            {result.report.samples.map((sample) => (
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
          {result.report.locationError && (
            <p role="status" className="text-sm">
              {label("locationError")}:{" "}
              {label(`errors.${result.report.locationError}`)}
            </p>
          )}
          <dl className="space-y-2 text-sm">
            {fields.map(([name, value]) => (
              <div key={name} className="flex flex-wrap justify-between gap-2">
                <dt>{label(name)}</dt>
                <dd className="break-all">{value || unknown}</dd>
              </div>
            ))}
          </dl>
          {location && (
            <p className="text-xs text-fg-2">{label("locationSource")}</p>
          )}
        </div>
      )}
      <div className="space-y-2 border-t border-border pt-4">
        <h3 className="text-sm font-medium">{label("community")}</h3>
        <p className="text-sm text-fg-2">{label("browserRoute")}</p>
        <Button
          variant="outline"
          size="sm"
          onClick={() => void openIpQualityCheck()}
        >
          {label("ipQuality")}
          <ExternalLink className="h-3.5 w-3.5" aria-hidden="true" />
        </Button>
      </div>
    </div>
  );
}
