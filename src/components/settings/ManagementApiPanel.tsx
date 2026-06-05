import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Activity,
  Check,
  Code2,
  Copy,
  ExternalLink,
  FileText,
  KeyRound,
  Loader2,
  Play,
  RefreshCw,
  ShieldCheck,
  Square,
  Trash2,
  X,
} from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";
import { copyText } from "@/lib/clipboard";
import {
  settingsApi,
  type ApiAuditLogRecord,
  type ApiPairingSessionRecord,
  type ApiTokenRecord,
  type ManagementApiStatus,
} from "@/lib/api/settings";
import type { ManagementApiSettings } from "@/types";

const DEFAULT_CONFIG: ManagementApiSettings = {
  enabled: false,
  listenAddress: "127.0.0.1",
  port: 15722,
  lanEnabled: false,
  allowedCidrs: [],
  corsOrigins: [],
  tlsEnabled: false,
  certificateFingerprint: null,
  pairingEnabled: true,
};

const DEFAULT_SCOPES = [
  "api:read",
  "providers:read",
  "providers:switch",
  "proxy:read",
  "settings:read",
];

const SCOPE_HINT =
  "providers:read, providers:switch, proxy:read, proxy:control, settings:read, auth:admin, api:read";

interface ManagementApiPanelProps {
  config?: ManagementApiSettings;
  onChange: (config: ManagementApiSettings) => Promise<void> | void;
}

function splitList(value: string): string[] {
  return value
    .split(/[,\n]/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function joinList(value?: string[]): string {
  return (value ?? []).join("\n");
}

function formatDate(
  value: number | null | undefined,
  emptyLabel: string,
): string {
  if (!value) return emptyLabel;
  return new Date(value).toLocaleString();
}

export function ManagementApiPanel({
  config,
  onChange,
}: ManagementApiPanelProps) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<ManagementApiStatus | null>(null);
  const [tokens, setTokens] = useState<ApiTokenRecord[]>([]);
  const [pairings, setPairings] = useState<ApiPairingSessionRecord[]>([]);
  const [auditLogs, setAuditLogs] = useState<ApiAuditLogRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [tokenName, setTokenName] = useState(() =>
    t("settings.advanced.managementApi.defaultTokenName"),
  );
  const [scopeText, setScopeText] = useState(DEFAULT_SCOPES.join(", "));
  const [rawToken, setRawToken] = useState<string | null>(null);

  const value = useMemo(
    () => ({ ...DEFAULT_CONFIG, ...(config ?? {}) }),
    [config],
  );

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [nextStatus, nextTokens, nextPairings, nextAuditLogs] =
        await Promise.all([
          settingsApi.getManagementApiStatus(),
          settingsApi.listManagementApiTokens(),
          settingsApi.listManagementApiPairingSessions(false),
          settingsApi.listManagementApiAuditLogs(100),
        ]);
      setStatus(nextStatus);
      setTokens(nextTokens);
      setPairings(nextPairings);
      setAuditLogs(nextAuditLogs);
    } catch (error) {
      console.error("[ManagementApiPanel] refresh failed", error);
      toast.error(String(error));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const updateConfig = async (updates: Partial<ManagementApiSettings>) => {
    const next = { ...value, ...updates };
    await onChange(next);
  };

  const handleStart = async () => {
    setBusy(true);
    try {
      await settingsApi.startManagementApi();
      await onChange({ ...value, enabled: true });
      await refresh();
    } catch (error) {
      toast.error(String(error));
    } finally {
      setBusy(false);
    }
  };

  const handleStop = async () => {
    setBusy(true);
    try {
      await settingsApi.stopManagementApi();
      await onChange({ ...value, enabled: false });
      await refresh();
    } catch (error) {
      toast.error(String(error));
    } finally {
      setBusy(false);
    }
  };

  const handleRestart = async () => {
    setBusy(true);
    try {
      await settingsApi.restartManagementApi();
      await refresh();
    } catch (error) {
      toast.error(String(error));
    } finally {
      setBusy(false);
    }
  };

  const handleCreateToken = async () => {
    const scopes = splitList(scopeText);
    if (!tokenName.trim() || scopes.length === 0) {
      toast.error(t("settings.advanced.managementApi.tokenNameScopesRequired"));
      return;
    }
    setBusy(true);
    try {
      const created = await settingsApi.createManagementApiToken({
        name: tokenName.trim(),
        scopes,
      });
      setRawToken(created.token);
      await refresh();
    } catch (error) {
      toast.error(String(error));
    } finally {
      setBusy(false);
    }
  };

  const handleRevoke = async (id: string) => {
    setBusy(true);
    try {
      await settingsApi.revokeManagementApiToken(id);
      await refresh();
    } catch (error) {
      toast.error(String(error));
    } finally {
      setBusy(false);
    }
  };

  const handleApprovePairing = async (pairing: ApiPairingSessionRecord) => {
    setBusy(true);
    try {
      await settingsApi.approveManagementApiPairing({
        pairingId: pairing.id,
        name: pairing.clientName,
        scopes: pairing.requestedScopes,
      });
      await refresh();
    } catch (error) {
      toast.error(String(error));
    } finally {
      setBusy(false);
    }
  };

  const handleRejectPairing = async (pairingId: string) => {
    setBusy(true);
    try {
      await settingsApi.rejectManagementApiPairing(pairingId);
      await refresh();
    } catch (error) {
      toast.error(String(error));
    } finally {
      setBusy(false);
    }
  };

  const handleClearLogs = async () => {
    setBusy(true);
    try {
      await settingsApi.clearManagementApiAuditLogs();
      await refresh();
    } catch (error) {
      toast.error(String(error));
    } finally {
      setBusy(false);
    }
  };

  const baseUrl =
    status?.baseUrl ?? `http://${value.listenAddress}:${value.port}/v1`;
  const docsUrl = baseUrl.replace(/\/v1$/, "");
  const curlExample = `curl -H "Authorization: Bearer <token>" ${baseUrl}/apps/codex/providers`;
  const providerRequestDemo = useMemo(
    () =>
      [
        `curl -s -H "Authorization: Bearer <token>" \\`,
        `  ${baseUrl}/apps/codex/providers`,
      ].join("\n"),
    [baseUrl],
  );
  const tokenRequestDemo = useMemo(() => {
    const body = JSON.stringify({
      name: t("settings.advanced.managementApi.defaultTokenName"),
      scopes: ["api:read", "providers:read"],
    });
    return [
      `curl -s -X POST ${baseUrl}/auth/tokens \\`,
      `  -H "Authorization: Bearer <admin-token>" \\`,
      `  -H "Content-Type: application/json" \\`,
      `  -d '${body}'`,
    ].join("\n");
  }, [baseUrl, t]);
  const pairingResponseDemo = useMemo(
    () =>
      JSON.stringify(
        {
          data: {
            pairingId: "pairing-id",
            userCode: "A1B2C3D4",
            pollToken: "poll_example_secret",
            expiresAt: 1780661400000,
          },
          meta: { requestId: "00000000-0000-4000-8000-000000000000" },
        },
        null,
        2,
      ),
    [],
  );
  const listResponseDemo = useMemo(
    () =>
      JSON.stringify(
        {
          data: [],
          meta: { requestId: "00000000-0000-4000-8000-000000000000" },
        },
        null,
        2,
      ),
    [],
  );

  if (loading && !status) {
    return (
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        <Loader2 className="h-4 w-4 animate-spin" />
        {t("settings.advanced.managementApi.loading")}
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div className="rounded-lg border border-border bg-muted/40 p-4 space-y-4">
        <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
          <div className="space-y-2">
            <div className="flex flex-wrap items-center gap-2">
              <ShieldCheck className="h-4 w-4 text-emerald-500" />
              <Label>{t("settings.advanced.managementApi.title")}</Label>
              <Badge variant={status?.running ? "default" : "secondary"}>
                <Activity
                  className={`mr-1.5 h-3 w-3 ${status?.running ? "animate-pulse" : ""}`}
                />
                {status?.running
                  ? t("settings.advanced.managementApi.running")
                  : t("settings.advanced.managementApi.stopped")}
              </Badge>
              <Badge variant={value.enabled ? "outline" : "secondary"}>
                {value.enabled
                  ? t("settings.advanced.managementApi.autoStart")
                  : t("settings.advanced.managementApi.manual")}
              </Badge>
            </div>
            <p className="font-mono text-xs text-muted-foreground">{baseUrl}</p>
            <p className="text-xs text-muted-foreground">
              {t("settings.advanced.managementApi.rootDocsAvailable")}{" "}
              <span className="font-mono">{docsUrl || "http://host:port"}</span>
            </p>
          </div>
          <div className="flex flex-wrap gap-2">
            <Button
              size="sm"
              variant="outline"
              onClick={() =>
                settingsApi.openExternal(docsUrl).catch((error) => {
                  toast.error(String(error));
                })
              }
            >
              <ExternalLink className="mr-2 h-4 w-4" />
              {t("settings.advanced.managementApi.docs")}
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={() =>
                copyText(baseUrl).then(() =>
                  toast.success(t("settings.advanced.managementApi.copied")),
                )
              }
            >
              <Copy className="mr-2 h-4 w-4" />
              URL
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={() =>
                copyText(curlExample).then(() =>
                  toast.success(t("settings.advanced.managementApi.copied")),
                )
              }
            >
              <Copy className="mr-2 h-4 w-4" />
              curl
            </Button>
            {status?.running ? (
              <Button
                size="sm"
                variant="outline"
                onClick={handleStop}
                disabled={busy}
              >
                <Square className="mr-2 h-4 w-4" />
                {t("settings.advanced.managementApi.stop")}
              </Button>
            ) : (
              <Button size="sm" onClick={handleStart} disabled={busy}>
                <Play className="mr-2 h-4 w-4" />
                {t("settings.advanced.managementApi.start")}
              </Button>
            )}
            <Button
              size="sm"
              variant="outline"
              onClick={handleRestart}
              disabled={busy}
            >
              <RefreshCw className="mr-2 h-4 w-4" />
              {t("settings.advanced.managementApi.restart")}
            </Button>
          </div>
        </div>

        <div className="grid gap-3 md:grid-cols-4">
          <div className="flex items-center justify-between rounded-md border border-border bg-background/60 px-3 py-2">
            <div className="space-y-0.5">
              <Label className="text-sm">
                {t("settings.advanced.managementApi.service")}
              </Label>
              <p className="text-xs text-muted-foreground">
                {status?.running
                  ? t("settings.advanced.managementApi.acceptingRequests")
                  : t("settings.advanced.managementApi.stopped")}
              </p>
            </div>
            <Switch
              checked={Boolean(status?.running)}
              onCheckedChange={(checked) =>
                checked ? void handleStart() : void handleStop()
              }
              disabled={busy}
            />
          </div>
          <div className="flex items-center justify-between rounded-md border border-border bg-background/60 px-3 py-2">
            <div className="space-y-0.5">
              <Label className="text-sm">
                {t("settings.advanced.managementApi.startByDefault")}
              </Label>
              <p className="text-xs text-muted-foreground">
                {t("settings.advanced.managementApi.applyOnNextLaunch")}
              </p>
            </div>
            <Switch
              checked={value.enabled}
              onCheckedChange={(checked) =>
                void updateConfig({ enabled: checked })
              }
            />
          </div>
          <div className="rounded-md border border-border bg-background/60 px-3 py-2">
            <p className="text-xs text-muted-foreground">
              {t("settings.advanced.managementApi.activeTokens")}
            </p>
            <p className="mt-1 text-lg font-semibold">
              {status?.tokenCount ??
                tokens.filter((token) => !token.revokedAt).length}
            </p>
          </div>
          <div className="rounded-md border border-border bg-background/60 px-3 py-2">
            <p className="text-xs text-muted-foreground">
              {t("settings.advanced.managementApi.auditLogs")}
            </p>
            <p className="mt-1 text-lg font-semibold">{auditLogs.length}</p>
          </div>
        </div>
      </div>

      <div className="grid gap-4 md:grid-cols-3">
        <div className="space-y-2">
          <Label>{t("settings.advanced.managementApi.listenAddress")}</Label>
          <Input
            value={value.listenAddress}
            disabled={!value.lanEnabled}
            onChange={(event) =>
              void updateConfig({ listenAddress: event.target.value })
            }
          />
        </div>
        <div className="space-y-2">
          <Label>{t("settings.advanced.managementApi.port")}</Label>
          <Input
            type="number"
            min={1}
            max={65535}
            value={value.port}
            onChange={(event) =>
              void updateConfig({ port: Number(event.target.value) || 15722 })
            }
          />
        </div>
        <div className="space-y-3">
          <div className="flex items-center justify-between rounded-lg border border-border/60 px-3 py-2">
            <Label>{t("settings.advanced.managementApi.pairing")}</Label>
            <Switch
              checked={value.pairingEnabled}
              onCheckedChange={(checked) =>
                void updateConfig({ pairingEnabled: checked })
              }
            />
          </div>
          <div className="flex items-center justify-between rounded-lg border border-border/60 px-3 py-2">
            <Label>{t("settings.advanced.managementApi.lanMode")}</Label>
            <Switch
              checked={value.lanEnabled}
              onCheckedChange={(checked) =>
                void updateConfig({ lanEnabled: checked })
              }
            />
          </div>
        </div>
      </div>

      {value.lanEnabled ? (
        <div className="grid gap-4 md:grid-cols-2">
          <div className="space-y-2">
            <Label>{t("settings.advanced.managementApi.allowedCidrs")}</Label>
            <Textarea
              value={joinList(value.allowedCidrs)}
              onChange={(event) =>
                void updateConfig({
                  allowedCidrs: splitList(event.target.value),
                })
              }
            />
          </div>
          <div className="space-y-2">
            <Label>{t("settings.advanced.managementApi.corsOrigins")}</Label>
            <Textarea
              value={joinList(value.corsOrigins)}
              onChange={(event) =>
                void updateConfig({
                  corsOrigins: splitList(event.target.value),
                })
              }
            />
          </div>
        </div>
      ) : null}

      <div className="space-y-3 rounded-lg border border-border/60 p-4">
        <div className="flex items-center gap-2">
          <Code2 className="h-4 w-4 text-blue-500" />
          <Label>{t("settings.advanced.managementApi.examples")}</Label>
        </div>
        <div className="grid gap-3 md:grid-cols-2">
          <div className="space-y-2">
            <div className="flex items-center justify-between gap-2">
              <p className="text-xs font-medium text-muted-foreground">
                {t("settings.advanced.managementApi.requestDemo")}
              </p>
              <Button
                size="sm"
                variant="outline"
                onClick={() =>
                  copyText(providerRequestDemo).then(() =>
                    toast.success(t("settings.advanced.managementApi.copied")),
                  )
                }
              >
                <Copy className="mr-2 h-4 w-4" />
                {t("common.copy")}
              </Button>
            </div>
            <pre className="max-h-40 overflow-auto rounded-md border border-border/50 bg-muted/40 p-3 text-xs whitespace-pre-wrap break-words">
              {providerRequestDemo}
            </pre>
            <pre className="max-h-40 overflow-auto rounded-md border border-border/50 bg-muted/40 p-3 text-xs whitespace-pre-wrap break-words">
              {tokenRequestDemo}
            </pre>
          </div>
          <div className="space-y-2">
            <div className="flex items-center justify-between gap-2">
              <p className="text-xs font-medium text-muted-foreground">
                {t("settings.advanced.managementApi.responseDemo")}
              </p>
              <Button
                size="sm"
                variant="outline"
                onClick={() =>
                  copyText(listResponseDemo).then(() =>
                    toast.success(t("settings.advanced.managementApi.copied")),
                  )
                }
              >
                <Copy className="mr-2 h-4 w-4" />
                {t("common.copy")}
              </Button>
            </div>
            <pre className="max-h-40 overflow-auto rounded-md border border-border/50 bg-muted/40 p-3 text-xs whitespace-pre-wrap break-words">
              {listResponseDemo}
            </pre>
            <pre className="max-h-40 overflow-auto rounded-md border border-border/50 bg-muted/40 p-3 text-xs whitespace-pre-wrap break-words">
              {pairingResponseDemo}
            </pre>
          </div>
        </div>
      </div>

      <div className="space-y-3 rounded-lg border border-border/60 p-4">
        <div className="flex items-center gap-2">
          <KeyRound className="h-4 w-4 text-amber-500" />
          <Label>{t("settings.advanced.managementApi.tokens")}</Label>
          <Badge variant="secondary">{tokens.length}</Badge>
        </div>
        <div className="grid gap-3 md:grid-cols-[220px_1fr_auto]">
          <Input
            value={tokenName}
            onChange={(event) => setTokenName(event.target.value)}
          />
          <Input
            value={scopeText}
            placeholder={SCOPE_HINT}
            onChange={(event) => setScopeText(event.target.value)}
          />
          <Button onClick={handleCreateToken} disabled={busy}>
            <KeyRound className="mr-2 h-4 w-4" />
            {t("settings.advanced.managementApi.create")}
          </Button>
        </div>
        {rawToken ? (
          <div className="rounded-lg border border-amber-500/40 bg-amber-500/10 p-3">
            <div className="mb-2 flex items-center justify-between gap-2">
              <span className="text-sm font-medium">
                {t("settings.advanced.managementApi.newToken")}
              </span>
              <Button
                size="sm"
                variant="outline"
                onClick={() =>
                  copyText(rawToken).then(() =>
                    toast.success(t("settings.advanced.managementApi.copied")),
                  )
                }
              >
                <Copy className="mr-2 h-4 w-4" />
                {t("common.copy")}
              </Button>
            </div>
            <p className="break-all font-mono text-xs">{rawToken}</p>
          </div>
        ) : null}
        <div className="space-y-2">
          {tokens.map((token) => (
            <div
              key={token.id}
              className="flex flex-col gap-3 rounded-lg border border-border/50 px-3 py-2 sm:flex-row sm:items-center sm:justify-between"
            >
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="font-medium">{token.name}</span>
                  <Badge variant={token.revokedAt ? "secondary" : "outline"}>
                    {token.revokedAt
                      ? t("settings.advanced.managementApi.revoked")
                      : t("settings.advanced.managementApi.active")}
                  </Badge>
                </div>
                <p className="mt-1 truncate font-mono text-xs text-muted-foreground">
                  {token.scopes.join(", ")}
                </p>
                <p className="mt-1 text-xs text-muted-foreground">
                  {t("settings.advanced.managementApi.lastUsed", {
                    value: formatDate(
                      token.lastUsedAt,
                      t("settings.advanced.managementApi.never"),
                    ),
                  })}
                </p>
              </div>
              <Button
                size="sm"
                variant="outline"
                disabled={busy || Boolean(token.revokedAt)}
                onClick={() => handleRevoke(token.id)}
              >
                <Trash2 className="mr-2 h-4 w-4" />
                {t("settings.advanced.managementApi.revoke")}
              </Button>
            </div>
          ))}
        </div>
      </div>

      <div className="space-y-3 rounded-lg border border-border/60 p-4">
        <div className="flex items-center justify-between">
          <Label>{t("settings.advanced.managementApi.pairingRequests")}</Label>
          <Button size="sm" variant="outline" onClick={refresh}>
            <RefreshCw className="mr-2 h-4 w-4" />
            {t("common.refresh")}
          </Button>
        </div>
        {pairings.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            {t("settings.advanced.managementApi.noPendingPairing")}
          </p>
        ) : (
          <div className="space-y-2">
            {pairings.map((pairing) => (
              <div
                key={pairing.id}
                className="flex flex-col gap-3 rounded-lg border border-border/50 px-3 py-2 sm:flex-row sm:items-center sm:justify-between"
              >
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="font-medium">{pairing.clientName}</span>
                    <Badge variant="secondary">
                      {t(
                        `settings.advanced.managementApi.pairingStatus.${pairing.status}`,
                        pairing.status,
                      )}
                    </Badge>
                  </div>
                  <p className="mt-1 truncate font-mono text-xs text-muted-foreground">
                    {pairing.requestedScopes.join(", ")}
                  </p>
                  <p className="mt-1 text-xs text-muted-foreground">
                    {t("settings.advanced.managementApi.expires", {
                      value: formatDate(
                        pairing.expiresAt,
                        t("settings.advanced.managementApi.never"),
                      ),
                    })}
                  </p>
                </div>
                {pairing.status === "pending" ? (
                  <div className="flex gap-2">
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => handleRejectPairing(pairing.id)}
                      disabled={busy}
                    >
                      <X className="mr-2 h-4 w-4" />
                      {t("settings.advanced.managementApi.reject")}
                    </Button>
                    <Button
                      size="sm"
                      onClick={() => handleApprovePairing(pairing)}
                      disabled={busy}
                    >
                      <Check className="mr-2 h-4 w-4" />
                      {t("settings.advanced.managementApi.approve")}
                    </Button>
                  </div>
                ) : null}
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="space-y-3 rounded-lg border border-border/60 p-4">
        <div className="flex items-center justify-between gap-3">
          <div className="flex items-center gap-2">
            <FileText className="h-4 w-4 text-cyan-500" />
            <Label>{t("settings.advanced.managementApi.auditLogs")}</Label>
            <Badge variant="secondary">{auditLogs.length}</Badge>
          </div>
          <div className="flex gap-2">
            <Button size="sm" variant="outline" onClick={refresh}>
              <RefreshCw className="mr-2 h-4 w-4" />
              {t("common.refresh")}
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={handleClearLogs}
              disabled={busy}
            >
              <Trash2 className="mr-2 h-4 w-4" />
              {t("common.clear")}
            </Button>
          </div>
        </div>
        {auditLogs.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            {t("settings.advanced.managementApi.noAuditRecords")}
          </p>
        ) : (
          <div className="overflow-x-auto rounded-lg border border-border/50">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>
                    {t("settings.advanced.managementApi.time")}
                  </TableHead>
                  <TableHead>
                    {t("settings.advanced.managementApi.method")}
                  </TableHead>
                  <TableHead>
                    {t("settings.advanced.managementApi.path")}
                  </TableHead>
                  <TableHead>
                    {t("settings.advanced.managementApi.status")}
                  </TableHead>
                  <TableHead>
                    {t("settings.advanced.managementApi.scope")}
                  </TableHead>
                  <TableHead>
                    {t("settings.advanced.managementApi.token")}
                  </TableHead>
                  <TableHead>
                    {t("settings.advanced.managementApi.ip")}
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {auditLogs.map((log) => (
                  <TableRow key={log.id}>
                    <TableCell className="whitespace-nowrap text-xs text-muted-foreground">
                      {formatDate(
                        log.createdAt,
                        t("settings.advanced.managementApi.never"),
                      )}
                    </TableCell>
                    <TableCell className="font-mono text-xs">
                      {log.method}
                    </TableCell>
                    <TableCell className="font-mono text-xs">
                      {log.path}
                    </TableCell>
                    <TableCell>
                      <Badge
                        variant={
                          log.status >= 200 && log.status < 300
                            ? "default"
                            : "secondary"
                        }
                      >
                        {log.status}
                      </Badge>
                    </TableCell>
                    <TableCell className="font-mono text-xs">
                      {log.scope ?? "-"}
                    </TableCell>
                    <TableCell className="font-mono text-xs">
                      {log.tokenId ?? "-"}
                    </TableCell>
                    <TableCell className="font-mono text-xs">
                      {log.remoteIp ?? "-"}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        )}
      </div>
    </div>
  );
}
