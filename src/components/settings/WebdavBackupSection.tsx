import { useCallback, useMemo, useState } from "react";
import { Link2, UploadCloud, DownloadCloud, Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { settingsApi } from "@/lib/api";
import type { WebDavBackupSettings } from "@/types";

interface WebdavBackupSectionProps {
  config?: WebDavBackupSettings;
  onChange: (updates: Partial<WebDavBackupSettings>) => void;
}

const normalize = (value?: string) => {
  const trimmed = value?.trim();
  return trimmed && trimmed.length > 0 ? trimmed : undefined;
};

export function WebdavBackupSection({
  config,
  onChange,
}: WebdavBackupSectionProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [isTesting, setIsTesting] = useState(false);
  const [isBackingUp, setIsBackingUp] = useState(false);
  const [isRestoring, setIsRestoring] = useState(false);

  const form = useMemo(
    () => ({
      url: config?.url ?? "",
      username: config?.username ?? "",
      password: config?.password ?? "",
      remotePath: config?.remotePath ?? "",
    }),
    [config],
  );

  const updateField = useCallback(
    (field: keyof WebDavBackupSettings, value: string) => {
      onChange({ [field]: value });
    },
    [onChange],
  );

  const buildPayload = useCallback(() => {
    const url = normalize(form.url);
    if (!url) {
      return null;
    }

    return {
      url,
      username: normalize(form.username),
      password: normalize(form.password),
      remotePath: normalize(form.remotePath),
    };
  }, [form]);

  const handleTest = useCallback(async () => {
    const payload = buildPayload();
    if (!payload) {
      toast.error(t("settings.webdavBackup.missingUrl"));
      return;
    }

    setIsTesting(true);
    try {
      await settingsApi.webdavTestConnection(payload);
      toast.success(t("settings.webdavBackup.testSuccess"));
    } catch (error) {
      toast.error(
        t("settings.webdavBackup.testFailed", {
          error: (error as Error)?.message ?? String(error),
        }),
      );
    } finally {
      setIsTesting(false);
    }
  }, [buildPayload, t]);

  const handleBackup = useCallback(async () => {
    const payload = buildPayload();
    if (!payload) {
      toast.error(t("settings.webdavBackup.missingUrl"));
      return;
    }

    setIsBackingUp(true);
    try {
      const result = await settingsApi.webdavBackupNow(payload);
      toast.success(t("settings.webdavBackup.backupSuccess"), {
        description: result.fileName ?? undefined,
        closeButton: true,
      });
    } catch (error) {
      toast.error(
        t("settings.webdavBackup.backupFailed", {
          error: (error as Error)?.message ?? String(error),
        }),
      );
    } finally {
      setIsBackingUp(false);
    }
  }, [buildPayload, t]);

  const handleRestore = useCallback(async () => {
    const payload = buildPayload();
    if (!payload) {
      toast.error(t("settings.webdavBackup.missingUrl"));
      return;
    }

    setIsRestoring(true);
    try {
      const result = await settingsApi.webdavRestoreLatest(payload);
      toast.success(t("settings.webdavBackup.restoreSuccess"), {
        description: result.fileName ?? undefined,
        closeButton: true,
      });

      // Invalidate all queries to refresh frontend data after restore
      await queryClient.invalidateQueries();
    } catch (error) {
      toast.error(
        t("settings.webdavBackup.restoreFailed", {
          error: (error as Error)?.message ?? String(error),
        }),
      );
    } finally {
      setIsRestoring(false);
    }
  }, [buildPayload, queryClient, t]);

  const isLoading = isTesting || isBackingUp || isRestoring;

  return (
    <section className="space-y-4">
      <header className="space-y-2">
        <h3 className="text-base font-semibold text-foreground">
          {t("settings.webdavBackup.title")}
        </h3>
        <p className="text-sm text-muted-foreground">
          {t("settings.webdavBackup.description")}
        </p>
      </header>

      <div className="space-y-4 rounded-xl glass-card p-6 border border-white/10">
        <div className="space-y-3">
          {/* Server URL */}
          <div className="flex items-center gap-4">
            <label className="w-40 text-xs font-medium text-foreground shrink-0">
              {t("settings.webdavBackup.url")}
            </label>
            <Input
              value={form.url}
              onChange={(event) => updateField("url", event.target.value)}
              placeholder={t("settings.webdavBackup.urlPlaceholder")}
              className="text-xs flex-1"
            />
          </div>

          {/* Username */}
          <div className="flex items-center gap-4">
            <label className="w-40 text-xs font-medium text-foreground shrink-0">
              {t("settings.webdavBackup.username")}
            </label>
            <Input
              value={form.username}
              onChange={(event) => updateField("username", event.target.value)}
              placeholder={t("settings.webdavBackup.usernamePlaceholder")}
              className="text-xs flex-1"
            />
          </div>

          {/* Password */}
          <div className="flex items-center gap-4">
            <label className="w-40 text-xs font-medium text-foreground shrink-0">
              {t("settings.webdavBackup.password")}
            </label>
            <Input
              type="password"
              value={form.password}
              onChange={(event) => updateField("password", event.target.value)}
              placeholder={t("settings.webdavBackup.passwordPlaceholder")}
              className="text-xs flex-1"
              autoComplete="off"
            />
          </div>

          {/* Remote Path */}
          <div className="flex items-center gap-4">
            <label className="w-40 text-xs font-medium text-foreground shrink-0">
              {t("settings.webdavBackup.remotePath")}
            </label>
            <Input
              value={form.remotePath}
              onChange={(event) => updateField("remotePath", event.target.value)}
              placeholder={t("settings.webdavBackup.remotePathPlaceholder")}
              className="text-xs flex-1"
            />
          </div>
        </div>

        {/* Buttons */}
        <div className="flex flex-wrap items-center gap-3 pt-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={handleTest}
            disabled={isLoading}
          >
            {isTesting ? (
              <span className="inline-flex items-center gap-2">
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                {t("settings.webdavBackup.testing")}
              </span>
            ) : (
              <span className="inline-flex items-center gap-2">
                <Link2 className="h-3.5 w-3.5" />
                {t("settings.webdavBackup.test")}
              </span>
            )}
          </Button>

          <Button
            type="button"
            size="sm"
            onClick={handleBackup}
            disabled={isLoading}
          >
            {isBackingUp ? (
              <span className="inline-flex items-center gap-2">
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                {t("settings.webdavBackup.backingUp")}
              </span>
            ) : (
              <span className="inline-flex items-center gap-2">
                <UploadCloud className="h-3.5 w-3.5" />
                {t("settings.webdavBackup.backupNow")}
              </span>
            )}
          </Button>

          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={handleRestore}
            disabled={isLoading}
          >
            {isRestoring ? (
              <span className="inline-flex items-center gap-2">
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                {t("settings.webdavBackup.restoring")}
              </span>
            ) : (
              <span className="inline-flex items-center gap-2">
                <DownloadCloud className="h-3.5 w-3.5" />
                {t("settings.webdavBackup.restoreNow")}
              </span>
            )}
          </Button>
        </div>
      </div>
    </section>
  );
}
