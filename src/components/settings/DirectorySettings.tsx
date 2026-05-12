import { useMemo } from "react";
import { FolderSearch, Loader2, Undo2 } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { useTranslation } from "react-i18next";
import type { AppId } from "@/lib/api";
import type { ResolvedDirectories } from "@/hooks/useSettings";
import type { CliDetectionItem, CliDetectionMap } from "@/hooks/useDirectorySettings";

type DirectoryAppId = Exclude<AppId, "claude-desktop">;

interface DirectorySettingsProps {
  appConfigDir?: string;
  resolvedDirs: ResolvedDirectories;
  cliDetections: CliDetectionMap;
  cliDetectionMeta: {
    isLoading: boolean;
    wslInstalled: boolean;
    wslDistro?: string;
  };
  onAppConfigChange: (value?: string) => void;
  onBrowseAppConfig: () => Promise<void>;
  onResetAppConfig: () => Promise<void>;
  claudeDir?: string;
  claudeWslDir?: string;
  codexDir?: string;
  codexWslDir?: string;
  geminiDir?: string;
  geminiWslDir?: string;
  opencodeDir?: string;
  opencodeWslDir?: string;
  openclawWslDir?: string;
  openclawDir?: string;
  hermesDir?: string;
  onDirectoryChange: (app: DirectoryAppId, value?: string) => void;
  onClaudeWslDirectoryChange: (value?: string) => void;
  onBrowseDirectory: (app: DirectoryAppId) => Promise<void>;
  onBrowseClaudeWslDirectory: () => Promise<void>;
  onResetDirectory: (app: DirectoryAppId) => Promise<void>;
  onResetClaudeWslDirectory: () => Promise<void>;
  onWslDirectoryChange: (app: AppId, value?: string) => void;
  onBrowseWslDirectory: (app: AppId) => Promise<void>;
  onResetWslDirectory: (app: AppId) => Promise<void>;
}

export function DirectorySettings({
  appConfigDir,
  resolvedDirs,
  cliDetections,
  cliDetectionMeta,
  onAppConfigChange,
  onBrowseAppConfig,
  onResetAppConfig,
  claudeDir,
  claudeWslDir,
  codexDir,
  codexWslDir,
  geminiDir,
  geminiWslDir,
  opencodeDir,
  opencodeWslDir,
  openclawWslDir,
  openclawDir,
  hermesDir,
  onDirectoryChange,
  onClaudeWslDirectoryChange,
  onBrowseDirectory,
  onBrowseClaudeWslDirectory,
  onResetDirectory,
  onResetClaudeWslDirectory,
  onWslDirectoryChange,
  onBrowseWslDirectory,
  onResetWslDirectory,
}: DirectorySettingsProps) {
  const { t } = useTranslation();

  return (
    <div className="space-y-6">
      {/* CC Switch 配置目录 - 独立区块 */}
      <section className="space-y-4">
        <header className="space-y-1">
          <h3 className="text-sm font-medium">{t("settings.appConfigDir")}</h3>
          <p className="text-xs text-muted-foreground">
            {t("settings.appConfigDirDescription")}
          </p>
        </header>

        <div className="flex items-center gap-2">
          <Input
            value={appConfigDir ?? resolvedDirs.appConfig ?? ""}
            placeholder={t("settings.browsePlaceholderApp")}
            className="text-xs"
            onChange={(event) => onAppConfigChange(event.target.value)}
          />
          <Button
            type="button"
            variant="outline"
            size="icon"
            onClick={onBrowseAppConfig}
            title={t("settings.browseDirectory")}
          >
            <FolderSearch className="h-4 w-4" />
          </Button>
          <Button
            type="button"
            variant="outline"
            size="icon"
            onClick={onResetAppConfig}
            title={t("settings.resetDefault")}
          >
            <Undo2 className="h-4 w-4" />
          </Button>
        </div>
      </section>

      {/* Claude/Codex 配置目录 - 独立区块 */}
      <section className="space-y-4">
        <header className="space-y-1">
          <h3 className="text-sm font-medium">
            {t("settings.configDirectoryOverride")}
          </h3>
          <p className="text-xs text-muted-foreground">
            {t("settings.configDirectoryDescription")}
          </p>
          <DetectionSummary
            isLoading={cliDetectionMeta.isLoading}
            wslInstalled={cliDetectionMeta.wslInstalled}
            wslDistro={cliDetectionMeta.wslDistro}
          />
        </header>

        <DirectoryInput
          app="claude"
          label={t("settings.claudeConfigDir")}
          description={undefined}
          value={claudeDir}
          resolvedValue={resolvedDirs.claude}
          detection={cliDetections.claude}
          placeholder={t("settings.browsePlaceholderClaude")}
          onChange={(val) => onDirectoryChange("claude", val)}
          onBrowse={() => onBrowseDirectory("claude")}
          onReset={() => onResetDirectory("claude")}
        />

        <DirectoryInput
          app="claude"
          label={t("settings.claudeConfigDirWsl")}
          description={t("settings.claudeConfigDirWslDescription")}
          value={claudeWslDir}
          resolvedValue=""
          detection={cliDetections.claude}
          placeholder={t("settings.browsePlaceholderClaudeWsl")}
          onChange={(val) => onClaudeWslDirectoryChange(val)}
          onBrowse={onBrowseClaudeWslDirectory}
          onReset={onResetClaudeWslDirectory}
        />

        <DirectoryInput
          app="codex"
          label={t("settings.codexConfigDir")}
          description={undefined}
          value={codexDir}
          resolvedValue={resolvedDirs.codex}
          detection={cliDetections.codex}
          placeholder={t("settings.browsePlaceholderCodex")}
          onChange={(val) => onDirectoryChange("codex", val)}
          onBrowse={() => onBrowseDirectory("codex")}
          onReset={() => onResetDirectory("codex")}
        />

        <DirectoryInput
          app="codex"
          label={t("settings.codexConfigDirWsl")}
          description={t("settings.codexConfigDirWslDescription")}
          value={codexWslDir}
          resolvedValue=""
          detection={cliDetections.codex}
          placeholder={t("settings.browsePlaceholderCodexWsl")}
          onChange={(val) => onWslDirectoryChange("codex", val)}
          onBrowse={() => onBrowseWslDirectory("codex")}
          onReset={() => onResetWslDirectory("codex")}
        />

        <DirectoryInput
          app="gemini"
          label={t("settings.geminiConfigDir")}
          description={undefined}
          value={geminiDir}
          resolvedValue={resolvedDirs.gemini}
          detection={cliDetections.gemini}
          placeholder={t("settings.browsePlaceholderGemini")}
          onChange={(val) => onDirectoryChange("gemini", val)}
          onBrowse={() => onBrowseDirectory("gemini")}
          onReset={() => onResetDirectory("gemini")}
        />

        <DirectoryInput
          app="gemini"
          label={t("settings.geminiConfigDirWsl")}
          description={t("settings.geminiConfigDirWslDescription")}
          value={geminiWslDir}
          resolvedValue=""
          detection={cliDetections.gemini}
          placeholder={t("settings.browsePlaceholderGeminiWsl")}
          onChange={(val) => onWslDirectoryChange("gemini", val)}
          onBrowse={() => onBrowseWslDirectory("gemini")}
          onReset={() => onResetWslDirectory("gemini")}
        />

        <DirectoryInput
          app="opencode"
          label={t("settings.opencodeConfigDir")}
          description={undefined}
          value={opencodeDir}
          resolvedValue={resolvedDirs.opencode}
          detection={cliDetections.opencode}
          placeholder={t("settings.browsePlaceholderOpencode")}
          onChange={(val) => onDirectoryChange("opencode", val)}
          onBrowse={() => onBrowseDirectory("opencode")}
          onReset={() => onResetDirectory("opencode")}
        />

        <DirectoryInput
          app="opencode"
          label={t("settings.opencodeConfigDirWsl")}
          description={t("settings.opencodeConfigDirWslDescription")}
          value={opencodeWslDir}
          resolvedValue=""
          detection={cliDetections.opencode}
          placeholder={t("settings.browsePlaceholderOpencodeWsl")}
          onChange={(val) => onWslDirectoryChange("opencode", val)}
          onBrowse={() => onBrowseWslDirectory("opencode")}
          onReset={() => onResetWslDirectory("opencode")}
        />

        <DirectoryInput
          app="openclaw"
          label={t("settings.openclawConfigDirWsl")}
          description={t("settings.openclawConfigDirWslDescription")}
          value={openclawWslDir}
          resolvedValue=""
          placeholder={t("settings.browsePlaceholderOpenclawWsl")}
          onChange={(val) => onWslDirectoryChange("openclaw", val)}
          onBrowse={() => onBrowseWslDirectory("openclaw")}
          onReset={() => onResetWslDirectory("openclaw")}
        />

        <DirectoryInput
          label={t("settings.openclawConfigDir")}
          description={undefined}
          value={openclawDir}
          resolvedValue={resolvedDirs.openclaw}
          placeholder={t("settings.browsePlaceholderOpenclaw")}
          onChange={(val) => onDirectoryChange("openclaw", val)}
          onBrowse={() => onBrowseDirectory("openclaw")}
          onReset={() => onResetDirectory("openclaw")}
        />

        <DirectoryInput
          label={t("settings.hermesConfigDir")}
          description={undefined}
          value={hermesDir}
          resolvedValue={resolvedDirs.hermes}
          placeholder={t("settings.browsePlaceholderHermes")}
          onChange={(val) => onDirectoryChange("hermes", val)}
          onBrowse={() => onBrowseDirectory("hermes")}
          onReset={() => onResetDirectory("hermes")}
        />
      </section>
    </div>
  );
}

interface DirectoryInputProps {
  app: AppId;
  label: string;
  description?: string;
  value?: string;
  resolvedValue: string;
  detection?: CliDetectionItem;
  placeholder?: string;
  onChange: (value?: string) => void;
  onBrowse: () => Promise<void>;
  onReset: () => Promise<void>;
}

function DirectoryInput({
  app,
  label,
  description,
  value,
  resolvedValue,
  detection,
  placeholder,
  onChange,
  onBrowse,
  onReset,
}: DirectoryInputProps) {
  const { t } = useTranslation();
  const displayValue = useMemo(
    () => value ?? resolvedValue ?? "",
    [value, resolvedValue],
  );

  return (
    <div className="space-y-1.5">
      <div className="space-y-1">
        <p className="text-xs font-medium text-foreground">{label}</p>
        {description ? (
          <p className="text-xs text-muted-foreground">{description}</p>
        ) : null}
      </div>
      <div className="flex items-center gap-2">
        <Input
          value={displayValue}
          placeholder={placeholder}
          className="text-xs"
          onChange={(event) => onChange(event.target.value)}
        />
        <Button
          type="button"
          variant="outline"
          size="icon"
          onClick={onBrowse}
          title={t("settings.browseDirectory")}
        >
          <FolderSearch className="h-4 w-4" />
        </Button>
        <Button
          type="button"
          variant="outline"
          size="icon"
          onClick={onReset}
          title={t("settings.resetDefault")}
        >
          <Undo2 className="h-4 w-4" />
        </Button>
      </div>
      <DetectionDetails app={app} detection={detection} />
    </div>
  );
}

interface DetectionSummaryProps {
  isLoading: boolean;
  wslInstalled: boolean;
  wslDistro?: string;
}

function DetectionSummary({
  isLoading,
  wslInstalled,
  wslDistro,
}: DetectionSummaryProps) {
  const { t } = useTranslation();

  return (
    <div className="rounded-md border border-border/60 bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
      {isLoading ? (
        <span className="inline-flex items-center gap-2">
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
          {t("settings.cliDetection.loading")}
        </span>
      ) : wslInstalled ? (
        t("settings.cliDetection.wslInstalled", {
          distro: wslDistro ?? t("settings.cliDetection.unknownDistro"),
        })
      ) : (
        t("settings.cliDetection.nativeOnly")
      )}
    </div>
  );
}

interface DetectionDetailsProps {
  app: AppId;
  detection?: CliDetectionItem;
}

function DetectionDetails({ app, detection }: DetectionDetailsProps) {
  const { t } = useTranslation();

  if (!detection) {
    return (
      <p className="text-[11px] text-muted-foreground">
        {t("settings.cliDetection.noData")}
      </p>
    );
  }

  const nativeKey = detection.native.configExists
    ? "settings.cliDetection.nativeFound"
    : "settings.cliDetection.nativeDefault";

  return (
    <div className="space-y-1 rounded-md border border-border/50 bg-background/60 px-3 py-2 text-[11px]">
      <p className="text-foreground/90">
        {t(nativeKey, {
          env: t(`settings.cliDetection.env.${detection.native.envType}`),
          path: detection.native.configDir,
        })}
      </p>
      {detection.native.executablePath ? (
        <p className="text-muted-foreground break-all">
          {t("settings.cliDetection.executable", {
            path: detection.native.executablePath,
          })}
        </p>
      ) : null}

      {detection.wsl ? (
        <>
          <p className="text-foreground/90">
            {t("settings.cliDetection.wslPath", {
              distro: detection.wsl.distro,
              path: detection.wsl.configDir,
            })}
          </p>
          {detection.wsl.executablePath ? (
            <p className="text-muted-foreground break-all">
              {t("settings.cliDetection.executable", {
                path: detection.wsl.executablePath,
              })}
            </p>
          ) : null}
        </>
      ) : app === "claude" || app === "codex" || app === "gemini" || app === "opencode" ? (
        <p className="text-muted-foreground">
          {t("settings.cliDetection.noWslPath")}
        </p>
      ) : null}
    </div>
  );
}
