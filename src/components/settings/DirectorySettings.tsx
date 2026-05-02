import { useMemo, useState } from "react";
import { FolderSearch, Undo2 } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import { useTranslation } from "react-i18next";
import type { AppId } from "@/lib/api";
import type { ConfigDirProfile } from "@/types";
import type { ResolvedDirectories } from "@/hooks/useSettings";

interface DirectorySettingsProps {
  appConfigDir?: string;
  resolvedDirs: ResolvedDirectories;
  onAppConfigChange: (value?: string) => void;
  onBrowseAppConfig: () => Promise<void>;
  onResetAppConfig: () => Promise<void>;
  claudeDir?: string;
  codexDir?: string;
  geminiDir?: string;
  opencodeDir?: string;
  openclawDir?: string;
  hermesDir?: string;
  onDirectoryChange: (app: AppId, value?: string) => void;
  onBrowseDirectory: (app: AppId) => Promise<void>;
  onResetDirectory: (app: AppId) => Promise<void>;
  // Profile management
  profiles?: ConfigDirProfile[];
  activeProfileId?: string;
  onCreateProfile?: (name: string) => Promise<ConfigDirProfile>;
  onUpdateProfile?: (profile: ConfigDirProfile) => Promise<void>;
  onDeleteProfile?: (id: string) => Promise<void>;
  onSwitchProfile?: (id: string) => Promise<void>;
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
  opencodeDir,
  openclawDir,
  hermesDir,
  onDirectoryChange,
  onBrowseDirectory,
  onResetDirectory,
  profiles,
  activeProfileId,
  onCreateProfile,
  onUpdateProfile,
  onDeleteProfile,
  onSwitchProfile,
}: DirectorySettingsProps) {
  const { t } = useTranslation();
  const [showCreateDialog, setShowCreateDialog] = useState(false);
  const [createInput, setCreateInput] = useState("");
  const [showRenameDialog, setShowRenameDialog] = useState(false);
  const [renameInput, setRenameInput] = useState("");

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
        </header>

        {/* Profile 选择器 - 放在配置目录覆盖区块内，Claude Code 配置目录前面 */}
        {profiles && profiles.length > 0 && (
          <div className="rounded-lg border border-border/50 p-4">
            <h4 className="mb-3 text-sm font-medium text-muted-foreground">
              {t("settings.profile.sectionTitle")}
            </h4>
            <div className="flex gap-2">
              <select
                value={activeProfileId || ""}
                onChange={(e) => onSwitchProfile?.(e.target.value)}
                className="flex-1 rounded-md border border-input bg-background px-3 py-2 text-sm"
              >
                {profiles.map((profile) => (
                  <option key={profile.id} value={profile.id}>
                    {profile.name}
                  </option>
                ))}
              </select>
              <button
                type="button"
                onClick={() => {
                  if (activeProfileId) {
                    const profile = profiles.find((p) => p.id === activeProfileId);
                    if (profile) {
                      setRenameInput(profile.name);
                      setShowRenameDialog(true);
                    }
                  }
                }}
                disabled={!activeProfileId}
                className="rounded-md border border-input bg-background px-3 py-2 text-sm hover:bg-accent disabled:opacity-50"
              >
                {t("settings.profile.rename")}
              </button>
              <button
                type="button"
                onClick={() => {
                  setCreateInput("");
                  setShowCreateDialog(true);
                }}
                className="rounded-md border border-input bg-background px-3 py-2 text-sm hover:bg-accent"
              >
                {t("settings.profile.create")}
              </button>
              <button
                type="button"
                onClick={() => {
                  if (activeProfileId && profiles.length > 1) {
                    onDeleteProfile?.(activeProfileId);
                  }
                }}
                disabled={!activeProfileId || profiles.length <= 1}
                className="rounded-md border border-input bg-background px-3 py-2 text-sm hover:bg-accent disabled:opacity-50"
              >
                {t("settings.profile.delete")}
              </button>
            </div>
          </div>
        )}

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
      </section>

      {/* Create Profile Dialog */}
      <Dialog open={showCreateDialog} onOpenChange={setShowCreateDialog}>
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle>{t("settings.profile.createTitle")}</DialogTitle>
          </DialogHeader>
          <div className="py-4">
            <Input
              value={createInput}
              onChange={(e) => setCreateInput(e.target.value)}
              placeholder={t("settings.profile.namePlaceholder")}
            />
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setShowCreateDialog(false)}>
              {t("common.cancel")}
            </Button>
            <Button
              onClick={() => {
                if (createInput.trim() && onCreateProfile) {
                  onCreateProfile(createInput.trim());
                }
                setShowCreateDialog(false);
              }}
              disabled={!createInput.trim()}
            >
              {t("common.create")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Rename Profile Dialog */}
      <Dialog open={showRenameDialog} onOpenChange={setShowRenameDialog}>
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle>{t("settings.profile.renameTitle")}</DialogTitle>
          </DialogHeader>
          <div className="py-4">
            <Input
              value={renameInput}
              onChange={(e) => setRenameInput(e.target.value)}
              placeholder={t("settings.profile.namePlaceholder")}
            />
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setShowRenameDialog(false)}>
              {t("common.cancel")}
            </Button>
            <Button
              onClick={() => {
                if (renameInput.trim() && activeProfileId && onUpdateProfile) {
                  const profile = profiles?.find((p) => p.id === activeProfileId);
                  if (profile) {
                    onUpdateProfile({ ...profile, name: renameInput.trim() });
                  }
                }
                setShowRenameDialog(false);
              }}
              disabled={!renameInput.trim()}
            >
              {t("common.save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
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
