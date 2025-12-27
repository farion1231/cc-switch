import { useMemo } from "react";
import { FolderSearch, Undo2, Plus, Trash2 } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { useTranslation } from "react-i18next";
import type { AppId } from "@/lib/api";
import type { ResolvedDirectories } from "@/hooks/useSettings";
import type { ConfigDirectorySet } from "@/types";

interface DirectorySettingsProps {
  appConfigDir?: string;
  resolvedDirs: ResolvedDirectories;
  onAppConfigChange: (value?: string) => void;
  onBrowseAppConfig: () => Promise<void>;
  onResetAppConfig: () => Promise<void>;
  claudeDir?: string;
  codexDir?: string;
  geminiDir?: string;
  onDirectoryChange: (app: AppId, value?: string) => void;
  onBrowseDirectory: (app: AppId) => Promise<void>;
  onResetDirectory: (app: AppId) => Promise<void>;
  configSets: ConfigDirectorySet[];
  onConfigSetNameChange: (setId: string, name: string) => void;
  onAddConfigSet: () => void;
  onRemoveConfigSet: (setId: string) => void;
  onConfigSetDirectoryChange: (setId: string, app: AppId, value?: string) => void;
  onBrowseConfigSetDirectory: (setId: string, app: AppId) => Promise<void>;
  onResetConfigSetDirectory: (setId: string, app: AppId) => Promise<void>;
}

const resolvedDirKey: Record<AppId, keyof ResolvedDirectories> = {
  claude: "claude",
  codex: "codex",
  gemini: "gemini",
};

export function DirectorySettings({
  appConfigDir,
  resolvedDirs,
  onAppConfigChange,
  onBrowseAppConfig,
  onResetAppConfig,
  claudeDir,
  codexDir,
  geminiDir,
  onDirectoryChange,
  onBrowseDirectory,
  onResetDirectory,
  configSets,
  onConfigSetNameChange,
  onAddConfigSet,
  onRemoveConfigSet,
  onConfigSetDirectoryChange,
  onBrowseConfigSetDirectory,
  onResetConfigSetDirectory,
}: DirectorySettingsProps) {
  const { t } = useTranslation();

  return (
    <>
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
        </header>

        <div className="space-y-4">
          {configSets.map((set, index) => {
            const isPrimary = index === 0;
            const claudeValue = isPrimary ? claudeDir : set.claudeConfigDir;
            const codexValue = isPrimary ? codexDir : set.codexConfigDir;
            const geminiValue = isPrimary ? geminiDir : set.geminiConfigDir;
            const resolvedValue = (app: AppId) =>
              isPrimary ? resolvedDirs[resolvedDirKey[app]] : "";

            return (
              <div
                key={set.id}
                className="space-y-4 rounded-xl border border-border-default/70 p-4"
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="flex-1 space-y-2">
                    <div className="flex items-center gap-2">
                      <p className="text-sm font-medium">
                        {isPrimary
                          ? t("settings.configSetPrimaryTitle")
                          : t("settings.configSetSecondaryTitle", {
                              index: index + 1,
                            })}
                      </p>
                      {isPrimary ? (
                        <span className="rounded-full bg-primary/10 px-2 py-0.5 text-xs font-semibold text-primary">
                          {t("settings.configSetPrimaryBadge")}
                        </span>
                      ) : null}
                    </div>
                    <Input
                      value={set.name ?? ""}
                      placeholder={t("settings.configSetNamePlaceholder")}
                      className="font-medium"
                      onChange={(event) =>
                        onConfigSetNameChange(set.id, event.target.value)
                      }
                    />
                    <p className="text-xs text-muted-foreground">
                      {isPrimary
                        ? t("settings.configSetPrimaryDescription")
                        : t("settings.configSetSecondaryDescription")}
                    </p>
                  </div>
                  {isPrimary ? null : (
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="text-muted-foreground hover:text-destructive"
                      onClick={() => onRemoveConfigSet(set.id)}
                      title={t("settings.removeConfigSet")}
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  )}
                </div>

                <div className="space-y-3">
                  <DirectoryInput
                    label={t("settings.claudeConfigDir")}
                    value={claudeValue}
                    resolvedValue={resolvedValue("claude")}
                    placeholder={t("settings.browsePlaceholderClaude")}
                    onChange={(val) =>
                      isPrimary
                        ? onDirectoryChange("claude", val)
                        : onConfigSetDirectoryChange(set.id, "claude", val)
                    }
                    onBrowse={() =>
                      isPrimary
                        ? onBrowseDirectory("claude")
                        : onBrowseConfigSetDirectory(set.id, "claude")
                    }
                    onReset={() =>
                      isPrimary
                        ? onResetDirectory("claude")
                        : onResetConfigSetDirectory(set.id, "claude")
                    }
                  />

                  <DirectoryInput
                    label={t("settings.codexConfigDir")}
                    value={codexValue}
                    resolvedValue={resolvedValue("codex")}
                    placeholder={t("settings.browsePlaceholderCodex")}
                    onChange={(val) =>
                      isPrimary
                        ? onDirectoryChange("codex", val)
                        : onConfigSetDirectoryChange(set.id, "codex", val)
                    }
                    onBrowse={() =>
                      isPrimary
                        ? onBrowseDirectory("codex")
                        : onBrowseConfigSetDirectory(set.id, "codex")
                    }
                    onReset={() =>
                      isPrimary
                        ? onResetDirectory("codex")
                        : onResetConfigSetDirectory(set.id, "codex")
                    }
                  />

                  <DirectoryInput
                    label={t("settings.geminiConfigDir")}
                    value={geminiValue}
                    resolvedValue={resolvedValue("gemini")}
                    placeholder={t("settings.browsePlaceholderGemini")}
                    onChange={(val) =>
                      isPrimary
                        ? onDirectoryChange("gemini", val)
                        : onConfigSetDirectoryChange(set.id, "gemini", val)
                    }
                    onBrowse={() =>
                      isPrimary
                        ? onBrowseDirectory("gemini")
                        : onBrowseConfigSetDirectory(set.id, "gemini")
                    }
                    onReset={() =>
                      isPrimary
                        ? onResetDirectory("gemini")
                        : onResetConfigSetDirectory(set.id, "gemini")
                    }
                  />
                </div>
              </div>
            );
          })}
        </div>

        <Button
          type="button"
          variant="outline"
          className="w-full gap-2"
          onClick={onAddConfigSet}
        >
          <Plus className="h-4 w-4" />
          {t("settings.addConfigSet")}
        </Button>
      </section>
    </>
  );
}

interface DirectoryInputProps {
  label: string;
  description?: string;
  value?: string;
  resolvedValue: string;
  placeholder?: string;
  onChange: (value?: string) => void;
  onBrowse: () => Promise<void>;
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
