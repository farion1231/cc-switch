import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  AlertTriangle,
  Check,
  Copy,
  Loader2,
  RefreshCw,
  Smartphone,
} from "lucide-react";
import { toast } from "@/lib/toast";
import {
  lanWebApi,
  type LanWebMode,
  type LanWebStatus,
} from "@/lib/api/lanWeb";
import type { AppId } from "@/lib/api/types";
import { APP_ICON_MAP } from "@/config/appConfig";
import { Button } from "@/components/ui/button";
import { HoverTip } from "@/components/ui/hover-tip";
import { Input } from "@/components/ui/input";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import {
  SettingsBlock,
  SettingsCard,
  SettingsRow,
  SettingsSwitchRow,
} from "@/components/settings/SettingsLayout";
import { QrCodeSvg } from "@/components/settings/QrCodeSvg";
import { copyText } from "@/lib/clipboard";

const REMOTE_CONTROL_AGENT_IDS: readonly AppId[] = [
  "claude",
  "codex",
  "gemini",
  "grokbuild",
  "opencode",
  "agy",
  "copilot",
  "kimi",
  "pi",
  "omp",
];

function maskUrlToken(url: string): string {
  return url.replace(/#t=.*/, "#t=••••");
}

/**
 * Settings → Network: LAN Mobile Web UI configuration card.
 *
 * Mirrors Magpie's clean 3-mode Browser access UX (Off / This computer / Local
 * network) while enforcing CC Switch's explicit LAN security confirmation,
 * read-only default, masked URL copy, inline QR code, and instant token rotation.
 */
export function LanAccessSection() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<LanWebStatus | null>(null);
  const [portInput, setPortInput] = useState("15722");
  const [selectedUrlIdx, setSelectedUrlIdx] = useState(0);
  const [copiedUrl, setCopiedUrl] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirmLanOpen, setConfirmLanOpen] = useState(false);
  const [pendingRemoteApp, setPendingRemoteApp] = useState<AppId | null>(null);

  const loadStatus = useCallback(async () => {
    try {
      const next = await lanWebApi.getStatus();
      setStatus(next);
      setPortInput(String(next.port));
    } catch (err) {
      console.error("[LanAccessSection] Failed to load status:", err);
    }
  }, []);

  useEffect(() => {
    void loadStatus();
  }, [loadStatus]);

  const applyConfig = async (patch: {
    mode?: LanWebMode;
    port?: number;
    allowControl?: boolean;
    remoteControlApps?: string[];
  }) => {
    if (!status) return;
    setBusy(true);
    try {
      const next = await lanWebApi.updateConfig({
        mode: patch.mode ?? status.mode,
        port: patch.port ?? status.port,
        allowControl: patch.allowControl ?? status.allowControl,
        remoteControlApps:
          patch.remoteControlApps ?? status.remoteControlApps ?? [],
      });
      setStatus(next);
      setPortInput(String(next.port));
      setSelectedUrlIdx(0);
    } catch (err) {
      toast.error(
        err instanceof Error ? err.message : t("settings.lanWeb.saveFailed"),
      );
      await loadStatus();
    } finally {
      setBusy(false);
    }
  };

  const handleToggleRemoteApp = (app: AppId) => {
    if (!status) return;
    const current = status.remoteControlApps ?? [];
    const isEnabled = current.includes(app);
    if (isEnabled) {
      void applyConfig({
        remoteControlApps: current.filter((id) => id !== app),
      });
      return;
    }
    // Warn before enabling the first remote agent while exposed on plain-HTTP LAN.
    if (status.mode === "lan" && current.length === 0) {
      setPendingRemoteApp(app);
      return;
    }
    void applyConfig({
      remoteControlApps: [...current, app],
    });
  };

  const handleModeChange = (nextMode: LanWebMode) => {
    if (!status || nextMode === status.mode) return;
    if (nextMode === "lan") {
      setConfirmLanOpen(true);
      return;
    }
    void applyConfig({ mode: nextMode });
  };

  const handleApplyPort = () => {
    const parsed = Number(portInput.trim());
    if (!Number.isInteger(parsed) || parsed < 1024 || parsed > 65535) {
      toast.error(t("settings.lanWeb.invalidPort"));
      return;
    }
    void applyConfig({ port: parsed });
  };

  const handleRegenerateToken = async () => {
    setBusy(true);
    try {
      const next = await lanWebApi.regenerateToken();
      setStatus(next);
      toast.success(t("settings.lanWeb.tokenRotated"));
    } catch (err) {
      toast.error(
        err instanceof Error ? err.message : t("settings.lanWeb.saveFailed"),
      );
    } finally {
      setBusy(false);
    }
  };

  const handleCopy = async (url: string) => {
    try {
      await copyText(url);
      setCopiedUrl(url);
      toast.success(t("settings.lanWeb.linkCopied"));
      window.setTimeout(() => {
        setCopiedUrl((prev) => (prev === url ? null : prev));
      }, 1800);
    } catch {
      toast.error(t("settings.lanWeb.copyFailed"));
    }
  };

  const allUrls = status
    ? [
        ...(status.mode === "lan" ? status.lanUrls : []),
        ...(status.localUrl ? [status.localUrl] : []),
      ]
    : [];
  const qrUrl =
    allUrls[Math.min(selectedUrlIdx, Math.max(0, allUrls.length - 1))] ?? null;

  return (
    <>
      <SettingsBlock
        title={t("settings.lanWeb.title")}
        help={{
          title: t("settings.lanWeb.title"),
          body: t("settings.lanWeb.description"),
        }}
      >
        <SettingsCard>
          {/* Bind Mode */}
          <SettingsRow
            label={t("settings.lanWeb.modeLabel")}
            description={t("settings.lanWeb.modeDescription")}
          >
            <SegmentedControl<LanWebMode>
              aria-label={t("settings.lanWeb.modeLabel")}
              value={status?.mode ?? "off"}
              onValueChange={handleModeChange}
              items={[
                {
                  value: "off",
                  label: t("settings.lanWeb.modeOff"),
                  disabled: busy || !status,
                },
                {
                  value: "local",
                  label: t("settings.lanWeb.modeLocal"),
                  disabled: busy || !status,
                },
                {
                  value: "lan",
                  label: t("settings.lanWeb.modeLan"),
                  disabled: busy || !status,
                },
              ]}
            />
          </SettingsRow>

          {/* Port */}
          <SettingsRow
            label={t("settings.lanWeb.portLabel")}
            description={t("settings.lanWeb.portDescription")}
          >
            <div className="flex items-center gap-2">
              <Input
                type="number"
                min={1024}
                max={65535}
                value={portInput}
                onChange={(e) => setPortInput(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") handleApplyPort();
                }}
                className="w-28 font-mono tabular-nums"
                disabled={busy}
              />
              <Button
                variant="outline"
                size="compact"
                disabled={
                  busy || !status || portInput.trim() === String(status.port)
                }
                onClick={handleApplyPort}
              >
                {t("settings.lanWeb.applyPort")}
              </Button>
            </div>
          </SettingsRow>

          {/* Allow Remote Control Toggle */}
          <SettingsSwitchRow
            label={t("settings.lanWeb.allowControlLabel")}
            description={t("settings.lanWeb.allowControlDescription")}
            checked={status?.allowControl ?? false}
            disabled={busy || !status}
            onCheckedChange={(checked) =>
              void applyConfig({ allowControl: checked })
            }
          />

          {/* Per-Agent Remote Control Multi-App Selector */}
          <div className="border-t border-border px-4 py-3.5">
            <div className="flex flex-wrap items-start justify-between gap-2">
              <div className="space-y-0.5">
                <div className="text-sm font-medium text-foreground">
                  {t("settings.lanWeb.remoteAgentsLabel")}
                </div>
                <p className="text-xs text-muted-foreground">
                  {t("settings.lanWeb.remoteAgentsDescription")}
                </p>
              </div>
              <span className="rounded-full bg-muted px-2.5 py-0.5 text-[11px] font-medium text-muted-foreground">
                {(status?.remoteControlApps?.length ?? 0) > 0
                  ? t("settings.lanWeb.remoteAgentsEnabledCount", {
                      count: status?.remoteControlApps?.length ?? 0,
                    })
                  : t("settings.lanWeb.remoteAgentsAllOff")}
              </span>
            </div>

            <div
              className="mt-3 flex flex-wrap gap-1.5"
              role="group"
              aria-label={t("settings.lanWeb.remoteAgentsLabel")}
            >
              {REMOTE_CONTROL_AGENT_IDS.map((app) => {
                const { label, icon, activeClass } = APP_ICON_MAP[app];
                const enabled =
                  status?.remoteControlApps?.includes(app) ?? false;
                const installed =
                  status?.installedRemoteApps?.includes(app) ?? false;
                const statusText = installed
                  ? t("settings.lanWeb.remoteAgentInstalled")
                  : t("settings.lanWeb.remoteAgentNotInstalled");
                return (
                  <HoverTip
                    key={app}
                    content={`${label} · ${statusText}${enabled ? " ✓" : ""}`}
                    side="bottom"
                  >
                    <button
                      type="button"
                      disabled={busy || !status}
                      aria-label={label}
                      aria-pressed={enabled}
                      onClick={() => handleToggleRemoteApp(app)}
                      className={`inline-flex h-8 items-center gap-1.5 rounded-lg border px-2.5 text-xs font-medium transition-all disabled:cursor-not-allowed ${
                        enabled
                          ? `${activeClass} border-transparent`
                          : installed
                            ? "border-border bg-background/80 text-foreground/80 hover:bg-muted/70"
                            : "border-border/60 bg-muted/25 text-muted-foreground opacity-55 hover:opacity-85"
                      }`}
                    >
                      <span className="flex h-4 w-4 shrink-0 items-center justify-center">
                        {icon}
                      </span>
                      <span>{label}</span>
                      {enabled ? (
                        <Check className="h-3.5 w-3.5 shrink-0" />
                      ) : installed ? (
                        <span
                          className="h-1.5 w-1.5 shrink-0 rounded-full bg-emerald-500/70"
                          aria-hidden="true"
                        />
                      ) : null}
                    </button>
                  </HoverTip>
                );
              })}
            </div>
          </div>

          {/* Active Access Links + Inline QR Code */}
          {status && status.mode !== "off" && (
            <div className="border-t border-border px-4 py-4">
              <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
                <div className="min-w-0 flex-1 space-y-3">
                  <div className="flex flex-wrap items-center gap-2">
                    <span
                      className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-medium ${
                        status.running
                          ? "bg-emerald-500/15 text-emerald-700 dark:text-emerald-400"
                          : "bg-amber-500/15 text-amber-700 dark:text-amber-400"
                      }`}
                    >
                      <span
                        className={`h-1.5 w-1.5 rounded-full ${
                          status.running ? "bg-emerald-500" : "bg-amber-500"
                        }`}
                      />
                      {status.running
                        ? t("settings.lanWeb.statusRunning", {
                            addr: status.listenAddress ?? "",
                          })
                        : t("settings.lanWeb.statusStopped")}
                    </span>

                    <Button
                      variant="ghost"
                      size="compact"
                      disabled={busy}
                      onClick={() => void handleRegenerateToken()}
                      className="h-7 gap-1.5 text-xs"
                    >
                      {busy ? (
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                      ) : (
                        <RefreshCw className="h-3.5 w-3.5" />
                      )}
                      {t("settings.lanWeb.regenerateToken")}
                    </Button>
                  </div>

                  {status.error && (
                    <p className="text-xs text-destructive">{status.error}</p>
                  )}

                  {/* Masked copyable URLs */}
                  <div className="space-y-2">
                    {allUrls.map((url, idx) => (
                      <div
                        key={url}
                        className={`flex items-center justify-between gap-2 rounded-lg border px-3 py-2 text-xs ${
                          idx === selectedUrlIdx
                            ? "border-primary/40 bg-muted/60"
                            : "border-border bg-muted/30"
                        }`}
                      >
                        <button
                          type="button"
                          onClick={() => setSelectedUrlIdx(idx)}
                          className="min-w-0 flex-1 truncate text-left font-mono text-foreground"
                          title={maskUrlToken(url)}
                        >
                          {maskUrlToken(url)}
                        </button>
                        <Button
                          variant="ghost"
                          size="compact"
                          onClick={() => void handleCopy(url)}
                          className="h-7 shrink-0 gap-1 px-2 text-xs"
                        >
                          {copiedUrl === url ? (
                            <Check className="h-3.5 w-3.5 text-emerald-500" />
                          ) : (
                            <Copy className="h-3.5 w-3.5" />
                          )}
                          {copiedUrl === url
                            ? t("settings.lanWeb.copied")
                            : t("settings.lanWeb.copyLink")}
                        </Button>
                      </div>
                    ))}
                  </div>

                  {status.mode === "lan" && (
                    <div className="flex items-start gap-2 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-800 dark:text-amber-300">
                      <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
                      <span>{t("settings.lanWeb.lanWarning")}</span>
                    </div>
                  )}
                </div>

                {/* Inline SVG QR Code */}
                {qrUrl && (
                  <div className="flex shrink-0 flex-col items-center gap-1.5 rounded-xl border border-border bg-card p-3">
                    <QrCodeSvg value={qrUrl} size={144} className="rounded" />
                    <span className="inline-flex items-center gap-1 text-[11px] text-muted-foreground">
                      <Smartphone className="h-3 w-3" />
                      {t("settings.lanWeb.scanHint")}
                    </span>
                  </div>
                )}
              </div>
            </div>
          )}
        </SettingsCard>
      </SettingsBlock>

      <ConfirmDialog
        isOpen={confirmLanOpen}
        title={t("settings.lanWeb.confirmLanTitle")}
        message={
          (status?.remoteControlApps?.length ?? 0) > 0
            ? `${t("settings.lanWeb.confirmLanMessage")}\n\n${t(
                "settings.lanWeb.confirmLanRemoteNote",
                {
                  apps: status!.remoteControlApps
                    .map((id) => APP_ICON_MAP[id as AppId]?.label ?? id)
                    .join(", "),
                },
              )}`
            : t("settings.lanWeb.confirmLanMessage")
        }
        confirmText={t("settings.lanWeb.confirmLanButton")}
        cancelText={t("common.cancel")}
        variant="info"
        onConfirm={() => {
          setConfirmLanOpen(false);
          void applyConfig({ mode: "lan" });
        }}
        onCancel={() => setConfirmLanOpen(false)}
      />

      <ConfirmDialog
        isOpen={pendingRemoteApp !== null}
        title={t("settings.lanWeb.confirmRemoteTitle")}
        message={t("settings.lanWeb.confirmRemoteMessage", {
          app: pendingRemoteApp ? APP_ICON_MAP[pendingRemoteApp].label : "",
        })}
        confirmText={t("settings.lanWeb.confirmRemoteButton", {
          app: pendingRemoteApp ? APP_ICON_MAP[pendingRemoteApp].label : "",
        })}
        cancelText={t("common.cancel")}
        variant="info"
        onConfirm={() => {
          if (pendingRemoteApp && status) {
            const current = status.remoteControlApps ?? [];
            void applyConfig({
              remoteControlApps: current.includes(pendingRemoteApp)
                ? current
                : [...current, pendingRemoteApp],
            });
          }
          setPendingRemoteApp(null);
        }}
        onCancel={() => setPendingRemoteApp(null)}
      />
    </>
  );
}
