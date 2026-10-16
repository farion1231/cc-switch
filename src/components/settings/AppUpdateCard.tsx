import { Download, ExternalLink, Loader2, Sparkles } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { SessionMarkdown } from "@/components/sessions/reader/SessionMarkdown";
import { useUpdate } from "@/contexts/UpdateContext";
import { settingsApi } from "@/lib/api";
import { toast } from "@/lib/toast";
import { getUpdateReleaseNotes } from "@/lib/updateReleaseNotes";
import { extractErrorMessage } from "@/utils/errorUtils";

export function AppUpdateCard({ isPortable }: { isPortable: boolean }) {
  const { t, i18n } = useTranslation();
  const { updateInfo, error } = useUpdate();
  const [openingDownload, setOpeningDownload] = useState(false);
  if (!updateInfo) return null;

  const releaseNotes = getUpdateReleaseNotes(
    updateInfo,
    i18n.resolvedLanguage ?? i18n.language,
  );
  const publishedAt = updateInfo.pubDate ? new Date(updateInfo.pubDate) : null;
  const releaseUrl = `https://github.com/farion1231/cc-switch/releases/tag/v${encodeURIComponent(updateInfo.availableVersion.replace(/^v/, ""))}`;
  const openExternal = (url: string) => {
    void settingsApi.openExternal(url).catch(() => {
      toast.error(t("settings.openReleaseNotesFailed"));
    });
  };
  const openDownloadPage = async () => {
    setOpeningDownload(true);
    try {
      await settingsApi.openExternal(`${releaseUrl}#assets`);
    } catch (err) {
      toast.error(t("settings.openUpdateDownloadFailed"), {
        description: extractErrorMessage(err) || undefined,
        closeButton: true,
      });
    } finally {
      setOpeningDownload(false);
    }
  };

  return (
    <section aria-labelledby="app-update-title" className="space-y-4 px-5 py-5">
      <div className="flex flex-wrap items-start gap-3">
        <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-control bg-action-soft text-action-text">
          <Sparkles className="h-4 w-4" aria-hidden="true" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h2 id="app-update-title" className="text-section text-fg-1">
              {t("settings.updateAvailable", {
                version: updateInfo.availableVersion,
              })}
            </h2>
            <span className="rounded bg-danger-soft px-1.5 py-0.5 text-badge text-danger-text">
              {t("settings.newVersion")}
            </span>
          </div>
          <p className="mt-1 text-caption text-fg-2">
            {t("settings.currentVersion")} v{updateInfo.currentVersion}
            {publishedAt && !Number.isNaN(publishedAt.getTime()) && (
              <time dateTime={updateInfo.pubDate} className="ms-3">
                {publishedAt.toLocaleDateString(i18n.language)}
              </time>
            )}
          </p>
        </div>
      </div>

      <div className="overflow-hidden rounded-control border border-border">
        <div className="flex items-center justify-between gap-3 border-b border-border bg-subtle px-4 py-2">
          <h3 className="text-strong text-fg-1">
            {t("settings.releaseNotes")}
          </h3>
          <Button
            type="button"
            variant="quiet"
            size="compact"
            onClick={() => openExternal(releaseUrl)}
          >
            <ExternalLink className="h-3.5 w-3.5" />
            {t("settings.viewReleaseNotes")}
          </Button>
        </div>
        <div
          role="region"
          aria-label={t("settings.releaseNotes")}
          tabIndex={0}
          className="max-h-96 overflow-y-auto px-4 py-3"
        >
          {releaseNotes ? (
            <SessionMarkdown content={releaseNotes} onOpenLink={openExternal} />
          ) : (
            <p className="text-body text-fg-2">
              {t("settings.noReleaseNotes")}
            </p>
          )}
        </div>
      </div>

      {error && (
        <p role="alert" className="break-words text-caption text-danger">
          {error}
        </p>
      )}
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="max-w-sm text-caption text-fg-2">
          {t(
            isPortable
              ? "settings.manualPortableUpdateHint"
              : "settings.manualUpdateHint",
          )}
        </p>
        <Button
          type="button"
          variant="solid"
          size="regular"
          disabled={openingDownload}
          onClick={() => void openDownloadPage()}
        >
          {openingDownload ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <Download className="h-3.5 w-3.5" />
          )}
          {t("settings.downloadNewVersion")}
        </Button>
      </div>
    </section>
  );
}
