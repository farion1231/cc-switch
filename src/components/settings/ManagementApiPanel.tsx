import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Check,
  Copy,
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
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { copyText } from "@/lib/clipboard";
import {
  settingsApi,
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

function formatDate(value?: number | null): string {
  if (!value) return "Never";
  return new Date(value).toLocaleString();
}

export function ManagementApiPanel({
  config,
  onChange,
}: ManagementApiPanelProps) {
  const [status, setStatus] = useState<ManagementApiStatus | null>(null);
  const [tokens, setTokens] = useState<ApiTokenRecord[]>([]);
  const [pairings, setPairings] = useState<ApiPairingSessionRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [tokenName, setTokenName] = useState("Local automation");
  const [scopeText, setScopeText] = useState(DEFAULT_SCOPES.join(", "));
  const [rawToken, setRawToken] = useState<string | null>(null);

  const value = useMemo(
    () => ({ ...DEFAULT_CONFIG, ...(config ?? {}) }),
    [config],
  );

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [nextStatus, nextTokens, nextPairings] = await Promise.all([
        settingsApi.getManagementApiStatus(),
        settingsApi.listManagementApiTokens(),
        settingsApi.listManagementApiPairingSessions(false),
      ]);
      setStatus(nextStatus);
      setTokens(nextTokens);
      setPairings(nextPairings);
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
      toast.error("Token name and scopes are required");
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

  const baseUrl =
    status?.baseUrl ?? `http://${value.listenAddress}:${value.port}/v1`;
  const curlExample = `curl -H "Authorization: Bearer <token>" ${baseUrl}/apps/codex/providers`;

  if (loading && !status) {
    return (
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        <Loader2 className="h-4 w-4 animate-spin" />
        Loading Management API
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-4 rounded-lg border border-border/60 bg-muted/20 p-4 sm:flex-row sm:items-center sm:justify-between">
        <div className="space-y-2">
          <div className="flex items-center gap-2">
            <ShieldCheck className="h-4 w-4 text-emerald-500" />
            <Label>Management API</Label>
            <Badge variant={status?.running ? "default" : "secondary"}>
              {status?.running ? "Running" : "Stopped"}
            </Badge>
          </div>
          <p className="font-mono text-xs text-muted-foreground">{baseUrl}</p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button
            size="sm"
            variant="outline"
            onClick={() =>
              copyText(baseUrl).then(() => toast.success("Copied"))
            }
          >
            <Copy className="mr-2 h-4 w-4" />
            URL
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={() =>
              copyText(curlExample).then(() => toast.success("Copied"))
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
              Stop
            </Button>
          ) : (
            <Button size="sm" onClick={handleStart} disabled={busy}>
              <Play className="mr-2 h-4 w-4" />
              Start
            </Button>
          )}
          <Button
            size="sm"
            variant="outline"
            onClick={handleRestart}
            disabled={busy}
          >
            <RefreshCw className="mr-2 h-4 w-4" />
            Restart
          </Button>
        </div>
      </div>

      <div className="grid gap-4 md:grid-cols-3">
        <div className="space-y-2">
          <Label>Listen address</Label>
          <Input
            value={value.listenAddress}
            disabled={!value.lanEnabled}
            onChange={(event) =>
              void updateConfig({ listenAddress: event.target.value })
            }
          />
        </div>
        <div className="space-y-2">
          <Label>Port</Label>
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
            <Label>Pairing</Label>
            <Switch
              checked={value.pairingEnabled}
              onCheckedChange={(checked) =>
                void updateConfig({ pairingEnabled: checked })
              }
            />
          </div>
          <div className="flex items-center justify-between rounded-lg border border-border/60 px-3 py-2">
            <Label>LAN mode</Label>
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
            <Label>Allowed CIDRs</Label>
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
            <Label>CORS origins</Label>
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
          <KeyRound className="h-4 w-4 text-amber-500" />
          <Label>Tokens</Label>
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
            Create
          </Button>
        </div>
        {rawToken ? (
          <div className="rounded-lg border border-amber-500/40 bg-amber-500/10 p-3">
            <div className="mb-2 flex items-center justify-between gap-2">
              <span className="text-sm font-medium">New token</span>
              <Button
                size="sm"
                variant="outline"
                onClick={() =>
                  copyText(rawToken).then(() => toast.success("Copied"))
                }
              >
                <Copy className="mr-2 h-4 w-4" />
                Copy
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
                    {token.revokedAt ? "Revoked" : "Active"}
                  </Badge>
                </div>
                <p className="mt-1 truncate font-mono text-xs text-muted-foreground">
                  {token.scopes.join(", ")}
                </p>
                <p className="mt-1 text-xs text-muted-foreground">
                  Last used: {formatDate(token.lastUsedAt)}
                </p>
              </div>
              <Button
                size="sm"
                variant="outline"
                disabled={busy || Boolean(token.revokedAt)}
                onClick={() => handleRevoke(token.id)}
              >
                <Trash2 className="mr-2 h-4 w-4" />
                Revoke
              </Button>
            </div>
          ))}
        </div>
      </div>

      <div className="space-y-3 rounded-lg border border-border/60 p-4">
        <div className="flex items-center justify-between">
          <Label>Pairing Requests</Label>
          <Button size="sm" variant="outline" onClick={refresh}>
            <RefreshCw className="mr-2 h-4 w-4" />
            Refresh
          </Button>
        </div>
        {pairings.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No pending pairing requests.
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
                    <Badge variant="secondary">{pairing.status}</Badge>
                  </div>
                  <p className="mt-1 truncate font-mono text-xs text-muted-foreground">
                    {pairing.requestedScopes.join(", ")}
                  </p>
                  <p className="mt-1 text-xs text-muted-foreground">
                    Expires: {formatDate(pairing.expiresAt)}
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
                      Reject
                    </Button>
                    <Button
                      size="sm"
                      onClick={() => handleApprovePairing(pairing)}
                      disabled={busy}
                    >
                      <Check className="mr-2 h-4 w-4" />
                      Approve
                    </Button>
                  </div>
                ) : null}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
