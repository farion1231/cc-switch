import { useMemo } from "react";
import { FolderSearch, Undo2 } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { HoverTip } from "@/components/ui/hover-tip";
import { useTranslation } from "react-i18next";
import type { AppId } from "@/lib/api";
import type { ResolvedDirectories } from "@/hooks/useSettings";

export type DirectoryAppId = Exclude<AppId, "claude-desktop" | "mcode">;

interface DirectorySettingsProps {
  appConfigDir?: string;
  resolvedDirs: ResolvedDirectories;
  onAppConfigChange: (value?: string) => void;
  onBrowseAppConfig: () => Promise<void>;
  onResetAppConfig: () => Promise<void>;
  claudeDir?: string;
  codexDir?: string;
  geminiDir?: string;
  grokDir?: string;
  opencodeDir?: string;
  openclawDir?: string;
  hermesDir?: string;
  piDir?: string;
  onDirectoryChange: (app: DirectoryAppId, value?: string) => void;
  onBrowseDirectory: (app: DirectoryAppId) => Promise<void>;
  onResetDirectory: (app: DirectoryAppId) => Promise<void>;
}

export function DirectorySettings({
  appConfigDir,
  resolvedDirs,
  onAppConfigChange,
  onBrowseAppConfig,
  onResetAppConfig,
  claudeDir,
  codexDir,
  geminiDir,
  grokDir,
  opencodeDir,
  openclawDir,
  hermesDir,
  piDir,
  onDirectoryChange,
  onBrowseDirectory,
  onResetDirectory,
}: DirectorySettingsProps) {
  const { t } = useTranslation();

  return (
    <div className="space-y-6">
      {/* CC Switch 配置目录 - 独立区块 */}
      <section className="space-y-4">
        <header className="space-y-1">
          <h3 className="text-sm font-medium">{t("settings.appConfigDir")}</h3>
          <p className="text-xs text-fg-2">
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
          <HoverTip content={t("settings.browseDirectory")}>
            <Button
              type="button"
              variant="outline"
              size="icon"
              onClick={onBrowseAppConfig}
              aria-label={t("settings.browseDirectory")}
            >
              <FolderSearch className="h-4 w-4" />
            </Button>
          </HoverTip>
          <HoverTip content={t("settings.resetDefault")}>
            <Button
              type="button"
              variant="outline"
              size="icon"
              onClick={onResetAppConfig}
              aria-label={t("settings.resetDefault")}
            >
              <Undo2 className="h-4 w-4" />
            </Button>
          </HoverTip>
        </div>
      </section>

      {/* Claude/Codex 配置目录 - 独立区块 */}
      <section className="space-y-4">
        <header className="space-y-1">
          <h3 className="text-sm font-medium">
            {t("settings.configDirectoryOverride")}
          </h3>
          <p className="text-xs text-fg-2">
            {t("settings.configDirectoryDescription")}
          </p>
        </header>

        <DirectoryInput
          label={t("settings.claudeConfigDir")}
          description={undefined}
          value={claudeDir}
          resolvedValue={resolvedDirs.claude}
          placeholder={t("settings.browsePlaceholderClaude")}
          onChange={(val) => onDirectoryChange("claude", val)}
          onBrowse={() => onBrowseDirectory("claude")}
          onReset={() => onResetDirectory("claude")}
        />

        <DirectoryInput
          label={t("settings.codexConfigDir")}
          description={undefined}
          value={codexDir}
          resolvedValue={resolvedDirs.codex}
          placeholder={t("settings.browsePlaceholderCodex")}
          onChange={(val) => onDirectoryChange("codex", val)}
          onBrowse={() => onBrowseDirectory("codex")}
          onReset={() => onResetDirectory("codex")}
        />

        <DirectoryInput
          label={t("settings.geminiConfigDir")}
          description={undefined}
          value={geminiDir}
          resolvedValue={resolvedDirs.gemini}
          placeholder={t("settings.browsePlaceholderGemini")}
          onChange={(val) => onDirectoryChange("gemini", val)}
          onBrowse={() => onBrowseDirectory("gemini")}
          onReset={() => onResetDirectory("gemini")}
        />

        <DirectoryInput
          label={t("settings.grokConfigDir")}
          description={undefined}
          value={grokDir}
          resolvedValue={resolvedDirs.grokbuild}
          placeholder={t("settings.browsePlaceholderGrok")}
          onChange={(val) => onDirectoryChange("grokbuild", val)}
          onBrowse={() => onBrowseDirectory("grokbuild")}
          onReset={() => onResetDirectory("grokbuild")}
        />

        <DirectoryInput
          label={t("settings.opencodeConfigDir")}
          description={undefined}
          value={opencodeDir}
          resolvedValue={resolvedDirs.opencode}
          placeholder={t("settings.browsePlaceholderOpencode")}
          onChange={(val) => onDirectoryChange("opencode", val)}
          onBrowse={() => onBrowseDirectory("opencode")}
          onReset={() => onResetDirectory("opencode")}
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

        <DirectoryInput
          label={t("settings.piConfigDir")}
          description={undefined}
          value={piDir}
          resolvedValue={resolvedDirs.pi}
          placeholder={t("settings.browsePlaceholderPi")}
          onChange={(val) => onDirectoryChange("pi", val)}
          onBrowse={() => onBrowseDirectory("pi")}
          onReset={() => onResetDirectory("pi")}
        />
      </section>
    </div>
  );
}

export interface DirectoryInputProps {
  label: string;
  description?: string;
  value?: string;
  resolvedValue: string;
  placeholder?: string;
  onChange: (value?: string) => void;
  onBrowse: () => Promise<void>;
  onReset: () => Promise<void>;
}

export function DirectoryInput({
  label,
  description,
  value,
  resolvedValue,
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
      {label || description ? (
        <div className="space-y-1">
          {label ? (
            <p className="text-xs font-medium text-fg-1">{label}</p>
          ) : null}
          {description ? (
            <p className="text-xs text-fg-2">{description}</p>
          ) : null}
        </div>
      ) : null}
      <div className="flex items-center gap-2">
        <Input
          value={displayValue}
          placeholder={placeholder}
          className="text-xs"
          onChange={(event) => onChange(event.target.value)}
        />
        <HoverTip content={t("settings.browseDirectory")}>
          <Button
            type="button"
            variant="outline"
            size="icon"
            onClick={onBrowse}
            aria-label={t("settings.browseDirectory")}
          >
            <FolderSearch className="h-4 w-4" />
          </Button>
        </HoverTip>
        <HoverTip content={t("settings.resetDefault")}>
          <Button
            type="button"
            variant="outline"
            size="icon"
            onClick={onReset}
            aria-label={t("settings.resetDefault")}
          >
            <Undo2 className="h-4 w-4" />
          </Button>
        </HoverTip>
      </div>
    </div>
  );
}
