import { useMemo, useState } from "react";
import { FolderSearch, Undo2 } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { useTranslation } from "react-i18next";
import type { AppId } from "@/lib/api";
import type { ResolvedDirectories } from "@/hooks/useSettings";
import { useRuntimeQuery } from "@/lib/query";
import { ServerDirectoryPickerDialog } from "./ServerDirectoryPickerDialog";

interface DirectorySettingsProps {
  appConfigDir?: string;
  resolvedDirs: ResolvedDirectories;
  onAppConfigChange: (value?: string) => void;
  onResetAppConfig: () => Promise<void>;
  claudeDir?: string;
  codexDir?: string;
  geminiDir?: string;
  opencodeDir?: string;
  openclawDir?: string;
  hermesDir?: string;
  onDirectoryChange: (app: AppId, value?: string) => void;
  onResetDirectory: (app: AppId) => Promise<void>;
}

type PickerTarget =
  | { kind: "appConfig" }
  | { kind: "app"; app: AppId; value?: string; resolvedValue: string };

export function DirectorySettings({
  appConfigDir,
  resolvedDirs,
  onAppConfigChange,
  onResetAppConfig,
  claudeDir,
  codexDir,
  geminiDir,
  opencodeDir,
  openclawDir,
  hermesDir,
  onDirectoryChange,
  onResetDirectory,
}: DirectorySettingsProps) {
  const { t } = useTranslation();
  const { data: runtimeInfo } = useRuntimeQuery();
  const [pickerTarget, setPickerTarget] = useState<PickerTarget | null>(null);
  const canOverrideAppConfigDir =
    runtimeInfo?.backend.capabilities.appConfigDirOverride !== false;

  const pickerInitialPath = useMemo(() => {
    if (!pickerTarget) return undefined;
    if (pickerTarget.kind === "appConfig") {
      return appConfigDir ?? resolvedDirs.appConfig;
    }
    return pickerTarget.value ?? pickerTarget.resolvedValue;
  }, [appConfigDir, pickerTarget, resolvedDirs.appConfig]);

  const handlePickerSelect = (path: string) => {
    if (!pickerTarget) return;
    if (pickerTarget.kind === "appConfig") {
      onAppConfigChange(path);
    } else {
      onDirectoryChange(pickerTarget.app, path);
    }
  };

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
            disabled={!canOverrideAppConfigDir}
            onChange={(event) => onAppConfigChange(event.target.value)}
          />
          <Button
            type="button"
            variant="outline"
            size="icon"
            disabled={!canOverrideAppConfigDir}
            onClick={() => setPickerTarget({ kind: "appConfig" })}
            title={t("settings.browseDirectory")}
          >
            <FolderSearch className="h-4 w-4" />
          </Button>
          <Button
            type="button"
            variant="outline"
            size="icon"
            disabled={!canOverrideAppConfigDir}
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
        </header>

        <DirectoryInput
          label={t("settings.claudeConfigDir")}
          description={undefined}
          value={claudeDir}
          resolvedValue={resolvedDirs.claude}
          placeholder={t("settings.browsePlaceholderClaude")}
          onChange={(val) => onDirectoryChange("claude", val)}
          onBrowse={() =>
            setPickerTarget({
              kind: "app",
              app: "claude",
              value: claudeDir,
              resolvedValue: resolvedDirs.claude,
            })
          }
          onReset={() => onResetDirectory("claude")}
        />

        <DirectoryInput
          label={t("settings.codexConfigDir")}
          description={undefined}
          value={codexDir}
          resolvedValue={resolvedDirs.codex}
          placeholder={t("settings.browsePlaceholderCodex")}
          onChange={(val) => onDirectoryChange("codex", val)}
          onBrowse={() =>
            setPickerTarget({
              kind: "app",
              app: "codex",
              value: codexDir,
              resolvedValue: resolvedDirs.codex,
            })
          }
          onReset={() => onResetDirectory("codex")}
        />

        <DirectoryInput
          label={t("settings.geminiConfigDir")}
          description={undefined}
          value={geminiDir}
          resolvedValue={resolvedDirs.gemini}
          placeholder={t("settings.browsePlaceholderGemini")}
          onChange={(val) => onDirectoryChange("gemini", val)}
          onBrowse={() =>
            setPickerTarget({
              kind: "app",
              app: "gemini",
              value: geminiDir,
              resolvedValue: resolvedDirs.gemini,
            })
          }
          onReset={() => onResetDirectory("gemini")}
        />

        <DirectoryInput
          label={t("settings.opencodeConfigDir")}
          description={undefined}
          value={opencodeDir}
          resolvedValue={resolvedDirs.opencode}
          placeholder={t("settings.browsePlaceholderOpencode")}
          onChange={(val) => onDirectoryChange("opencode", val)}
          onBrowse={() =>
            setPickerTarget({
              kind: "app",
              app: "opencode",
              value: opencodeDir,
              resolvedValue: resolvedDirs.opencode,
            })
          }
          onReset={() => onResetDirectory("opencode")}
        />

        <DirectoryInput
          label={t("settings.openclawConfigDir")}
          description={undefined}
          value={openclawDir}
          resolvedValue={resolvedDirs.openclaw}
          placeholder={t("settings.browsePlaceholderOpenclaw")}
          onChange={(val) => onDirectoryChange("openclaw", val)}
          onBrowse={() =>
            setPickerTarget({
              kind: "app",
              app: "openclaw",
              value: openclawDir,
              resolvedValue: resolvedDirs.openclaw,
            })
          }
          onReset={() => onResetDirectory("openclaw")}
        />

        <DirectoryInput
          label={t("settings.hermesConfigDir")}
          description={undefined}
          value={hermesDir}
          resolvedValue={resolvedDirs.hermes}
          placeholder={t("settings.browsePlaceholderHermes")}
          onChange={(val) => onDirectoryChange("hermes", val)}
          onBrowse={() =>
            setPickerTarget({
              kind: "app",
              app: "hermes",
              value: hermesDir,
              resolvedValue: resolvedDirs.hermes,
            })
          }
          onReset={() => onResetDirectory("hermes")}
        />
      </section>

      <ServerDirectoryPickerDialog
        open={pickerTarget !== null}
        initialPath={pickerInitialPath}
        onOpenChange={(open) => {
          if (!open) setPickerTarget(null);
        }}
        onSelect={handlePickerSelect}
      />
    </div>
  );
}

interface DirectoryInputProps {
  label: string;
  description?: string;
  value?: string;
  resolvedValue: string;
  placeholder?: string;
  onChange: (value?: string) => void;
  onBrowse: () => void;
  onReset: () => Promise<void>;
}

function DirectoryInput({
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
    </div>
  );
}
