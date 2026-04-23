import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Monitor, RefreshCcw, Copy, Download, Info } from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { ToggleRow } from "@/components/ui/toggle-row";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { settingsApi, type ClaudeDesktopExportFormat } from "@/lib/api";
import { copyText } from "@/lib/clipboard";
import type { SettingsFormState } from "@/hooks/useSettings";
import { isLinux, isMac, isWindows } from "@/lib/platform";
import { useTranslation } from "react-i18next";

interface ClaudeDesktopSettingsSectionProps {
  settings: SettingsFormState;
  onAutoSave: (updates: Partial<SettingsFormState>) => Promise<void>;
}

const EXPORT_LABELS: Record<ClaudeDesktopExportFormat, string> = {
  json: "JSON",
  mobileconfig: ".mobileconfig",
  reg: ".reg",
};

export function ClaudeDesktopSettingsSection({
  settings,
  onAutoSave,
}: ClaudeDesktopSettingsSectionProps) {
  const { t } = useTranslation();
  const isMacPlatform = isMac();
  const isWindowsPlatform = isWindows();
  const isLinuxPlatform = isLinux();
  const isSupportedPlatform = isMacPlatform || isWindowsPlatform;
  const [headersInput, setHeadersInput] = useState(
    (settings.claudeDesktopGatewayHeaders ?? []).join("\n"),
  );
  const [isExporting, setIsExporting] =
    useState<ClaudeDesktopExportFormat | null>(null);
  const [isSavingHeaders, setIsSavingHeaders] = useState(false);
  const [isOpeningInstallSettings, setIsOpeningInstallSettings] =
    useState(false);

  useEffect(() => {
    setHeadersInput((settings.claudeDesktopGatewayHeaders ?? []).join("\n"));
  }, [settings.claudeDesktopGatewayHeaders]);

  const previewQuery = useQuery({
    queryKey: [
      "claudeDesktopPreview",
      settings.claudeDesktopGatewayAuthScheme,
      settings.claudeDesktopGatewayHeaders,
      settings.claudeDesktopCodeTabEnabled,
      settings.claudeDesktopLocalMcpEnabled,
      settings.claudeDesktopIncludeManagedMcp,
    ],
    queryFn: async () => settingsApi.getClaudeDesktopPreview(),
    refetchOnWindowFocus: false,
    enabled: isSupportedPlatform,
  });

  const statusQuery = useQuery({
    queryKey: [
      "claudeDesktopStatus",
      settings.claudeDesktopGatewayAuthScheme,
      settings.claudeDesktopGatewayHeaders,
      settings.claudeDesktopCodeTabEnabled,
      settings.claudeDesktopLocalMcpEnabled,
      settings.claudeDesktopIncludeManagedMcp,
    ],
    queryFn: async () => settingsApi.getClaudeDesktopModeStatus(),
    refetchOnWindowFocus: false,
    enabled: isSupportedPlatform,
  });

  const persistHeaders = async () => {
    const nextHeaders = headersInput
      .split("\n")
      .map((line) => line.trim())
      .filter(Boolean);
    const currentHeaders = settings.claudeDesktopGatewayHeaders ?? [];
    if (JSON.stringify(nextHeaders) === JSON.stringify(currentHeaders)) {
      return;
    }

    setIsSavingHeaders(true);
    try {
      await onAutoSave({ claudeDesktopGatewayHeaders: nextHeaders });
    } finally {
      setIsSavingHeaders(false);
    }
  };

  const handleCopy = async () => {
    const text = previewQuery.data?.configJson;
    if (!text) return;
    try {
      await copyText(text);
      toast.success(t("settings.claudeDesktop.toasts.jsonCopied"), {
        closeButton: true,
      });
    } catch (error) {
      toast.error(
        t("settings.claudeDesktop.toasts.copyFailed", {
          error: error instanceof Error ? error.message : String(error),
        }),
      );
    }
  };

  const handleExport = async (format: ClaudeDesktopExportFormat) => {
    try {
      setIsExporting(format);
      const filePath = await settingsApi.saveClaudeDesktopExportDialog(format);
      if (!filePath) return;
      await settingsApi.exportClaudeDesktopConfig(format, filePath);
      if (format === "mobileconfig" && isMacPlatform) {
        await settingsApi.installClaudeDesktopMobileconfig(filePath);
        toast.success(t("settings.claudeDesktop.toasts.mobileconfigOpened"), {
          closeButton: true,
        });
        void statusQuery.refetch();
        return;
      }
      toast.success(
        t("settings.claudeDesktop.toasts.exportSuccess", {
          format: EXPORT_LABELS[format],
        }),
        { closeButton: true },
      );
    } catch (error) {
      toast.error(
        t("settings.claudeDesktop.toasts.exportFailed", {
          error: error instanceof Error ? error.message : String(error),
        }),
      );
    } finally {
      setIsExporting(null);
    }
  };

  const handleOpenInstallSettings = async () => {
    try {
      setIsOpeningInstallSettings(true);
      await settingsApi.openClaudeDesktopInstallSettings();
      toast.success(t("settings.claudeDesktop.toasts.installPageOpened"), {
        closeButton: true,
      });
    } catch (error) {
      toast.error(
        t("settings.claudeDesktop.toasts.openInstallFailed", {
          error: error instanceof Error ? error.message : String(error),
        }),
      );
    } finally {
      setIsOpeningInstallSettings(false);
    }
  };

  const preview = previewQuery.data;
  const status = statusQuery.data;
  const previewWarnings: string[] = [];
  if (preview) {
    if (!preview.currentProviderId) {
      previewWarnings.push(
        t("settings.claudeDesktop.warnings.noCurrentProvider"),
      );
    }
    if (!preview.localProxyEnabled) {
      previewWarnings.push(
        t("settings.claudeDesktop.warnings.localProxyDisabled"),
      );
    }
    if (!preview.proxyRunning) {
      previewWarnings.push(
        t("settings.claudeDesktop.warnings.proxyNotRunning"),
      );
    }
    if (preview.managedMcpCount === 0) {
      previewWarnings.push(t("settings.claudeDesktop.warnings.noManagedMcp"));
    }
  }
  const statusWarnings: string[] = [];
  if (status) {
    if (!status.supported) {
      statusWarnings.push(
        t(
          isWindowsPlatform
            ? "settings.claudeDesktop.warnings.windowsAutoDetectionUnavailable"
            : "settings.claudeDesktop.warnings.autoDetectionUnavailable",
        ),
      );
    } else {
      if (!status.appInstalled) {
        statusWarnings.push(
          t("settings.claudeDesktop.warnings.appNotInstalled"),
        );
      }
      if (!status.managedConfigExists) {
        statusWarnings.push(
          t("settings.claudeDesktop.warnings.managedConfigMissing"),
        );
      } else {
        if (!status.thirdPartyModeEnabled) {
          statusWarnings.push(
            t("settings.claudeDesktop.warnings.thirdPartyModeMissing"),
          );
        } else if (!status.gatewayModeEnabled) {
          statusWarnings.push(
            t("settings.claudeDesktop.warnings.notGatewayMode", {
              provider: status.currentInferenceProvider ?? t("common.unknown"),
            }),
          );
        } else if (!status.matchesExpectedGateway) {
          statusWarnings.push(
            t("settings.claudeDesktop.warnings.gatewayMismatch"),
          );
        }
        statusWarnings.push(
          t("settings.claudeDesktop.warnings.restartReminder"),
        );
      }
    }
  }
  const statusBadgeLabel = !status
    ? t("settings.claudeDesktop.statusLoading")
    : !status.supported
      ? t("settings.claudeDesktop.statusUnsupported")
      : status.thirdPartyModeEnabled
        ? t("settings.claudeDesktop.statusEnabled")
        : t("settings.claudeDesktop.statusDisabled");
  const statusBadgeVariant =
    status?.supported === false
      ? "secondary"
      : status?.thirdPartyModeEnabled
        ? "default"
        : "secondary";

  if (isLinuxPlatform || !isSupportedPlatform) {
    return (
      <Alert className="border-amber-500/30 bg-amber-500/5">
        <Info className="h-4 w-4" />
        <AlertTitle>{t("settings.claudeDesktop.unsupportedTitle")}</AlertTitle>
        <AlertDescription>
          {t("settings.claudeDesktop.platformUnsupportedDescription")}
        </AlertDescription>
      </Alert>
    );
  }

  return (
    <div className="space-y-6">
      <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(320px,360px)]">
        <section className="space-y-4 rounded-xl border border-border bg-card/40 p-4">
          <div className="space-y-1">
            <h4 className="text-sm font-semibold">
              {t("settings.claudeDesktop.configTitle")}
            </h4>
            <p className="text-xs text-muted-foreground">
              {t("settings.claudeDesktop.configDescription")}
            </p>
          </div>

          <div className="grid gap-4 md:grid-cols-2">
            <div className="space-y-2">
              <Label htmlFor="claude-desktop-auth-scheme">
                {t("settings.claudeDesktop.gatewayAuthScheme")}
              </Label>
              <Select
                value={settings.claudeDesktopGatewayAuthScheme ?? "x-api-key"}
                onValueChange={(value) =>
                  void onAutoSave({
                    claudeDesktopGatewayAuthScheme: value as
                      | "auto"
                      | "x-api-key"
                      | "bearer",
                  })
                }
              >
                <SelectTrigger id="claude-desktop-auth-scheme">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="x-api-key">x-api-key</SelectItem>
                  <SelectItem value="bearer">bearer</SelectItem>
                  <SelectItem value="auto">auto</SelectItem>
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-2">
              <Label htmlFor="claude-desktop-api-key">
                {t("settings.claudeDesktop.gatewayApiKey")}
              </Label>
              <Input
                id="claude-desktop-api-key"
                value={preview?.gatewayApiKey ?? "PROXY_MANAGED"}
                readOnly
              />
            </div>
          </div>

          <div className="space-y-2">
            <Label htmlFor="claude-desktop-headers">
              {t("settings.claudeDesktop.gatewayHeaders")}
            </Label>
            <Textarea
              id="claude-desktop-headers"
              value={headersInput}
              onChange={(event) => setHeadersInput(event.target.value)}
              onBlur={() => void persistHeaders()}
              rows={5}
              placeholder={t(
                "settings.claudeDesktop.gatewayHeadersPlaceholder",
              )}
            />
            <p className="text-xs text-muted-foreground">
              {t("settings.claudeDesktop.gatewayHeadersHint")}
            </p>
            {isSavingHeaders ? (
              <p className="text-xs text-muted-foreground">
                {t("settings.claudeDesktop.savingHeaders")}
              </p>
            ) : null}
          </div>

          <div className="space-y-3">
            <ToggleRow
              icon={<Monitor className="h-4 w-4 text-blue-500" />}
              title={t("settings.claudeDesktop.codeTabTitle")}
              description={t("settings.claudeDesktop.codeTabDescription")}
              checked={settings.claudeDesktopCodeTabEnabled ?? true}
              onCheckedChange={(checked) =>
                void onAutoSave({ claudeDesktopCodeTabEnabled: checked })
              }
            />
            <ToggleRow
              icon={<Monitor className="h-4 w-4 text-emerald-500" />}
              title={t("settings.claudeDesktop.localMcpTitle")}
              description={t("settings.claudeDesktop.localMcpDescription")}
              checked={settings.claudeDesktopLocalMcpEnabled ?? false}
              onCheckedChange={(checked) =>
                void onAutoSave({ claudeDesktopLocalMcpEnabled: checked })
              }
            />
            <ToggleRow
              icon={<Monitor className="h-4 w-4 text-amber-500" />}
              title={t("settings.claudeDesktop.managedMcpTitle")}
              description={t("settings.claudeDesktop.managedMcpDescription")}
              checked={settings.claudeDesktopIncludeManagedMcp ?? true}
              onCheckedChange={(checked) =>
                void onAutoSave({ claudeDesktopIncludeManagedMcp: checked })
              }
            />
          </div>
        </section>

        <section className="space-y-4 rounded-xl border border-border bg-card/40 p-4">
          <div className="space-y-2">
            <h4 className="text-sm font-semibold">
              {t("settings.claudeDesktop.currentStatusTitle")}
            </h4>
            <div className="flex flex-wrap gap-2">
              <Badge variant={preview?.proxyRunning ? "default" : "secondary"}>
                {preview?.proxyRunning
                  ? t("settings.claudeDesktop.proxyRunning")
                  : t("settings.claudeDesktop.proxyStopped")}
              </Badge>
              <Badge
                variant={preview?.localProxyEnabled ? "default" : "secondary"}
              >
                {preview?.localProxyEnabled
                  ? t("settings.claudeDesktop.localProxyEnabled")
                  : t("settings.claudeDesktop.localProxyDisabled")}
              </Badge>
              <Badge variant="secondary">
                {t("settings.claudeDesktop.mcpCount", {
                  count: preview?.managedMcpCount ?? 0,
                })}
              </Badge>
              <Badge variant={statusBadgeVariant}>{statusBadgeLabel}</Badge>
            </div>
          </div>

          <div className="space-y-2 text-xs text-muted-foreground">
            <p>
              {t("settings.claudeDesktop.providerLabel")}{" "}
              {preview?.currentProviderName ??
                t("settings.claudeDesktop.noClaudeProviderSelected")}
            </p>
            <p>
              {t("settings.claudeDesktop.gatewayLabel")}{" "}
              {preview?.gatewayBaseUrl ?? t("settings.claudeDesktop.loading")}
            </p>
            {isMacPlatform ? (
              <p>
                {t("settings.claudeDesktop.domainLabel")}{" "}
                {preview?.domain ?? "com.anthropic.claudefordesktop"}
              </p>
            ) : null}
            {isWindowsPlatform ? (
              <p>
                {t("settings.claudeDesktop.windowsRegistryLabel")}{" "}
                {preview?.registryPath ?? "HKCU\\SOFTWARE\\Policies\\Claude"}
              </p>
            ) : null}
            {status?.supported ? (
              <>
                <p>
                  {t("settings.claudeDesktop.managedConfigLabel")}{" "}
                  {status.managedConfigPath ??
                    t("settings.claudeDesktop.noInstalledConfig")}
                </p>
                <p>
                  {t("settings.claudeDesktop.installedProviderLabel")}{" "}
                  {status.currentInferenceProvider ??
                    t("settings.claudeDesktop.notInThirdPartyMode")}
                </p>
                <p>
                  {t("settings.claudeDesktop.installedGatewayLabel")}{" "}
                  {status.currentGatewayBaseUrl ??
                    t("settings.claudeDesktop.noInstalledGateway")}
                </p>
              </>
            ) : null}
          </div>

          {statusWarnings.length || previewWarnings.length ? (
            <div className="space-y-2 rounded-lg border border-amber-500/20 bg-amber-500/10 p-3">
              {statusWarnings.map((warning) => (
                <p key={`status-${warning}`} className="text-xs text-amber-700">
                  {warning}
                </p>
              ))}
              {previewWarnings.map((warning) => (
                <p key={warning} className="text-xs text-amber-700">
                  {warning}
                </p>
              ))}
            </div>
          ) : null}

          <div className="flex flex-wrap gap-2">
            <Button
              variant="outline"
              onClick={() => {
                void previewQuery.refetch();
                void statusQuery.refetch();
              }}
              disabled={previewQuery.isFetching || statusQuery.isFetching}
            >
              <RefreshCcw className="mr-2 h-4 w-4" />
              {t("settings.claudeDesktop.refreshStatus")}
            </Button>
            <Button variant="outline" onClick={() => void handleCopy()}>
              <Copy className="mr-2 h-4 w-4" />
              {t("settings.claudeDesktop.copyJson")}
            </Button>
            {isMacPlatform ? (
              <Button
                variant="outline"
                onClick={() => void handleExport("mobileconfig")}
                disabled={isExporting !== null}
              >
                <Download className="mr-2 h-4 w-4" />
                {t("settings.claudeDesktop.exportAndInstallMobileconfig")}
              </Button>
            ) : null}
            {isWindowsPlatform ? (
              <Button
                variant="outline"
                onClick={() => void handleExport("reg")}
                disabled={isExporting !== null}
              >
                <Download className="mr-2 h-4 w-4" />
                {t("settings.claudeDesktop.exportReg")}
              </Button>
            ) : null}
          </div>

          {isMacPlatform ? (
            <Alert className="border-blue-500/30 bg-blue-500/5">
              <Info className="h-4 w-4" />
              <AlertTitle>{t("settings.claudeDesktop.macos.title")}</AlertTitle>
              <AlertDescription className="space-y-2 text-sm">
                <p>{t("settings.claudeDesktop.macos.step1")}</p>
                <p>
                  {t("settings.claudeDesktop.macos.step2Prefix")}{" "}
                  <Button
                    variant="link"
                    size="sm"
                    className="inline h-auto p-0 align-baseline text-xs"
                    onClick={() => void handleOpenInstallSettings()}
                    disabled={isExporting !== null || isOpeningInstallSettings}
                  >
                    {t("settings.claudeDesktop.macos.deviceManagementLink")}
                  </Button>{" "}
                  {t("settings.claudeDesktop.macos.step2Suffix")}
                </p>
                <p>{t("settings.claudeDesktop.macos.step3")}</p>
              </AlertDescription>
            </Alert>
          ) : null}
          {isWindowsPlatform ? (
            <Alert className="border-blue-500/30 bg-blue-500/5">
              <Info className="h-4 w-4" />
              <AlertTitle>
                {t("settings.claudeDesktop.windows.title")}
              </AlertTitle>
              <AlertDescription className="space-y-1 text-sm">
                <p>{t("settings.claudeDesktop.windows.step1")}</p>
                <p>
                  {t("settings.claudeDesktop.windows.step2Prefix")}{" "}
                  <code className="font-mono text-xs">
                    HKCU\SOFTWARE\Policies\Claude
                  </code>
                  {t("settings.claudeDesktop.windows.step2Suffix")}
                </p>
                <p>{t("settings.claudeDesktop.windows.step3")}</p>
              </AlertDescription>
            </Alert>
          ) : null}
        </section>
      </div>

      <section className="space-y-3 rounded-xl border border-border bg-card/40 p-4">
        <div className="flex items-center justify-between gap-3">
          <div className="space-y-1">
            <h4 className="text-sm font-semibold">
              {t("settings.claudeDesktop.previewTitle")}
            </h4>
            <p className="text-xs text-muted-foreground">
              {t("settings.claudeDesktop.previewDescription")}
            </p>
          </div>
          {preview ? (
            <Badge variant="secondary">
              {t("settings.claudeDesktop.modelCount", {
                count: preview.inferenceModels.length,
              })}
            </Badge>
          ) : null}
        </div>
        <Textarea
          value={
            previewQuery.isLoading
              ? t("settings.claudeDesktop.previewLoading")
              : (preview?.configJson ??
                t("settings.claudeDesktop.previewUnavailable"))
          }
          readOnly
          rows={16}
          className="font-mono text-xs"
        />
      </section>
    </div>
  );
}
