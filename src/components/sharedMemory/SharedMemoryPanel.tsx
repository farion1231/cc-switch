import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  CloudDownload,
  CloudUpload,
  ExternalLink,
  Loader2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import MarkdownEditor from "@/components/MarkdownEditor";
import {
  sharedMemoryApi,
  type SharedMemorySnapshot,
} from "@/lib/api/sharedMemory";
import { settingsApi } from "@/lib/api/settings";
import { useDarkMode } from "@/hooks/useDarkMode";
import { extractErrorMessage } from "@/utils/errorUtils";

const DEFAULT_URL = "https://codex-memory.1716775457.workers.dev";

const SharedMemoryPanel: React.FC = () => {
  const { t } = useTranslation();
  const darkMode = useDarkMode();

  const [settings, setSettings] = useState({ url: DEFAULT_URL, token: "" });
  const [settingsLoaded, setSettingsLoaded] = useState(false);
  const [content, setContent] = useState("");
  const [snapshot, setSnapshot] = useState<SharedMemorySnapshot | null>(null);
  const [fetching, setFetching] = useState(false);
  const [pushing, setPushing] = useState(false);
  const [savingSettings, setSavingSettings] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const saved = await sharedMemoryApi.getSettings();
        if (cancelled) return;
        setSettings({
          url: saved.url || DEFAULT_URL,
          token: saved.token || "",
        });
        if (saved.lastSyncAt) {
          setSnapshot({
            ok: true,
            updatedAt: saved.lastSyncAt,
            bytes: saved.lastSyncBytes ?? 0,
            content: "",
          });
        }
      } catch (error) {
        if (!cancelled) {
          toast.error(t("sharedMemory.loadSettingsFailed"), {
            description: extractErrorMessage(error) || undefined,
          });
        }
      } finally {
        if (!cancelled) setSettingsLoaded(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [t]);

  const handleSaveSettings = useCallback(async () => {
    setSavingSettings(true);
    try {
      await sharedMemoryApi.saveSettings({
        url: settings.url,
        token: settings.token,
      });
      toast.success(t("sharedMemory.settingsSaved"));
    } catch (error) {
      toast.error(t("sharedMemory.settingsSaveFailed"), {
        description: extractErrorMessage(error) || undefined,
      });
    } finally {
      setSavingSettings(false);
    }
  }, [settings, t]);

  const handleFetch = useCallback(async () => {
    setFetching(true);
    try {
      const data = await sharedMemoryApi.fetch();
      setSnapshot(data);
      setContent(data.content ?? "");
      toast.success(t("sharedMemory.fetchSuccess"));
    } catch (error) {
      toast.error(t("sharedMemory.fetchFailed"), {
        description: extractErrorMessage(error) || undefined,
      });
    } finally {
      setFetching(false);
    }
  }, [t]);

  const handlePush = useCallback(async () => {
    setPushing(true);
    try {
      const data = await sharedMemoryApi.push(content);
      setSnapshot(data);
      toast.success(t("sharedMemory.pushSuccess"));
    } catch (error) {
      toast.error(t("sharedMemory.pushFailed"), {
        description: extractErrorMessage(error) || undefined,
      });
    } finally {
      setPushing(false);
    }
  }, [content, t]);

  const openWebEditor = useCallback(async () => {
    try {
      await settingsApi.openExternal(settings.url || DEFAULT_URL);
    } catch (error) {
      toast.error(t("sharedMemory.openWebFailed"), {
        description: extractErrorMessage(error) || undefined,
      });
    }
  }, [settings.url, t]);

  const lastUpdated = snapshot?.updatedAt
    ? new Date(snapshot.updatedAt).toLocaleString()
    : t("sharedMemory.neverUpdated");

  return (
    <div className="flex flex-col h-full gap-4 px-6 pt-4 pb-4">
      <div className="rounded-lg border bg-muted/20 p-4 flex flex-col gap-3">
        <div className="grid grid-cols-1 md:grid-cols-[1fr_1fr_auto] gap-3 items-end">
          <div className="flex flex-col gap-1.5">
            <Label>{t("sharedMemory.endpoint")}</Label>
            <Input
              value={settings.url}
              onChange={(event) =>
                setSettings({ ...settings, url: event.target.value })
              }
              placeholder={DEFAULT_URL}
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label>{t("sharedMemory.token")}</Label>
            <Input
              type="password"
              value={settings.token}
              onChange={(event) =>
                setSettings({ ...settings, token: event.target.value })
              }
              placeholder={t("sharedMemory.tokenPlaceholder")}
            />
          </div>
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => void handleSaveSettings()}
              disabled={savingSettings}
            >
              {savingSettings ? (
                <Loader2 className="w-3.5 h-3.5 animate-spin mr-1" />
              ) : null}
              {t("common.save")}
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => void openWebEditor()}
            >
              <ExternalLink className="w-3.5 h-3.5 mr-1" />
              {t("sharedMemory.openWeb")}
            </Button>
          </div>
        </div>
        <p className="text-xs text-muted-foreground">
          {t("sharedMemory.hint")}
        </p>
      </div>

      <div className="flex items-center justify-between gap-3 text-sm text-muted-foreground">
        <span>{t("sharedMemory.lastUpdated", { time: lastUpdated })}</span>
        <span>
          {snapshot
            ? t("sharedMemory.bytes", { count: snapshot.bytes })
            : t("sharedMemory.notSynced")}
        </span>
      </div>

      {!settingsLoaded ? (
        <div className="flex items-center justify-center h-64 text-muted-foreground">
          {t("prompts.loading")}
        </div>
      ) : (
        <div className="flex-1 min-h-0">
          <MarkdownEditor
            value={content}
            onChange={setContent}
            darkMode={darkMode}
            minHeight="calc(100vh - 420px)"
          />
        </div>
      )}

      <div className="flex items-center justify-between gap-3">
        <span className="text-xs text-muted-foreground">
          {t("sharedMemory.charCount", { count: content.length })}
        </span>
        <div className="flex items-center gap-3">
          <Button
            variant="outline"
            onClick={() => void handleFetch()}
            disabled={fetching || !settingsLoaded}
          >
            {fetching ? (
              <Loader2 className="w-4 h-4 animate-spin mr-1" />
            ) : (
              <CloudDownload className="w-4 h-4 mr-1" />
            )}
            {t("sharedMemory.fetch")}
          </Button>
          <Button
            onClick={() => void handlePush()}
            disabled={pushing || !settingsLoaded}
          >
            {pushing ? (
              <Loader2 className="w-4 h-4 animate-spin mr-1" />
            ) : (
              <CloudUpload className="w-4 h-4 mr-1" />
            )}
            {t("sharedMemory.push")}
          </Button>
        </div>
      </div>
    </div>
  );
};

export default SharedMemoryPanel;
