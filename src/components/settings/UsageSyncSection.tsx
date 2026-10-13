import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, RefreshCw } from "lucide-react";
import { toast } from "@/lib/toast";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Notice } from "@/components/ui/notice";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import {
  useResolveUsageSyncConflict,
  useRunUsageSync,
  useSaveUsageSyncSettings,
  useSetUsageSyncDeviceName,
  useUsageSyncEventBridge,
  useUsageSyncOverview,
} from "@/lib/query/usageSync";
import { extractErrorMessage } from "@/utils/errorUtils";
import type {
  UsageSyncBackend,
  UsageSyncDetailLevel,
  UsageSyncPreferences,
} from "@/types/usageSync";

const INTERVAL_OPTIONS = [5, 15, 30, 60, 360] as const;

function Row({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-start gap-4">
      <div className="w-40 shrink-0 text-xs font-medium text-fg-1">
        {label}
        {hint && (
          <span className="block text-badge font-normal text-fg-2">{hint}</span>
        )}
      </div>
      <div className="flex min-w-0 flex-1 items-center gap-2">{children}</div>
    </div>
  );
}

/**
 * 设置 → 数据 → 跨设备用量同步。
 *
 * 复用已配置的 WebDAV / S3 连接，只在这里选择承载用量数据的后端和共享粒度；
 * 与配置快照同步互不影响（远端目录为 usage-v1/，配置快照仍在 v2/）。
 */
export function UsageSyncSection() {
  const { t } = useTranslation();
  useUsageSyncEventBridge();
  const { data: overview, isLoading } = useUsageSyncOverview();
  const save = useSaveUsageSyncSettings();
  const runSync = useRunUsageSync();
  const setName = useSetUsageSyncDeviceName();
  const resolveConflict = useResolveUsageSyncConflict();
  const [nameDraft, setNameDraft] = useState("");
  const [confirmTakeOver, setConfirmTakeOver] = useState(false);

  useEffect(() => {
    setNameDraft(overview?.thisDevice.customName ?? "");
  }, [overview?.thisDevice.customName]);

  if (isLoading || !overview) {
    return <Loader2 className="h-4 w-4 animate-spin text-fg-3" />;
  }

  const { settings, availableBackends } = overview;
  const anyBackend = availableBackends.webdav || availableBackends.s3;
  // 只配了一个后端时默认用它；两个都配了必须明确选择，避免数据分叉
  const defaultBackend: UsageSyncBackend | undefined =
    settings.backend ??
    (availableBackends.webdav && !availableBackends.s3
      ? "webdav"
      : availableBackends.s3 && !availableBackends.webdav
        ? "s3"
        : undefined);

  const persist = async (patch: Partial<UsageSyncPreferences>) => {
    const next: UsageSyncPreferences = {
      enabled: settings.enabled,
      autoSync: settings.autoSync,
      backend: settings.backend ?? defaultBackend,
      detailLevel: settings.detailLevel,
      intervalMinutes: settings.intervalMinutes,
      ...patch,
    };
    try {
      await save.mutateAsync(next);
      return true;
    } catch (error) {
      toast.error(
        t("usageSync.settings.saveFailed", {
          error: extractErrorMessage(error) || String(error),
        }),
      );
      return false;
    }
  };

  const handleSyncNow = async () => {
    try {
      const report = await runSync.mutateAsync();
      const error =
        report.uploadError ??
        report.downloadError ??
        report.deviceErrors[0]?.error;
      if (error) {
        toast.warning(t("usageSync.syncPartial", { error }));
      } else {
        toast.success(
          t("usageSync.syncCompleted", { devices: report.devicesSeen }),
        );
      }
    } catch (error) {
      toast.error(
        t("usageSync.syncFailed", {
          error: extractErrorMessage(error) || String(error),
        }),
      );
    }
  };

  const handleResolve = async (resolution: "newIdentity" | "takeOver") => {
    try {
      await resolveConflict.mutateAsync(resolution);
      toast.success(t("usageSync.conflict.resolved"));
    } catch (error) {
      toast.error(extractErrorMessage(error) || String(error));
    }
  };

  const lastSync = settings.status.lastSyncAt
    ? new Date(settings.status.lastSyncAt * 1000).toLocaleString()
    : null;

  return (
    <div className="flex flex-col gap-4">
      <p className="m-0 text-xs text-fg-2">{t("usageSync.settings.intro")}</p>
      <ul className="m-0 flex list-disc flex-col gap-1 ps-5 text-xs text-fg-2">
        <li>{t("usageSync.settings.privacyShared")}</li>
        <li>{t("usageSync.settings.privacyNotShared")}</li>
        <li>{t("usageSync.settings.privacyCost")}</li>
      </ul>

      {!anyBackend && (
        <Notice tone="neutral" title={t("usageSync.settings.noBackendTitle")}>
          {t("usageSync.settings.noBackendBody")}
        </Notice>
      )}

      <Row
        label={t("usageSync.settings.enable")}
        hint={t("usageSync.settings.enableHint")}
      >
        <Switch
          checked={settings.enabled}
          disabled={!anyBackend || save.isPending}
          aria-label={t("usageSync.settings.enable")}
          onCheckedChange={(enabled) => {
            if (enabled && !defaultBackend) {
              toast.warning(t("usageSync.settings.chooseBackendFirst"));
              return;
            }
            void persist({ enabled });
          }}
        />
      </Row>

      <Row
        label={t("usageSync.settings.backend")}
        hint={t("usageSync.settings.backendHint")}
      >
        <Select
          value={defaultBackend ?? ""}
          onValueChange={(value) =>
            void persist({ backend: value as UsageSyncBackend })
          }
          disabled={!anyBackend || save.isPending}
        >
          <SelectTrigger
            className="flex-1 text-xs"
            aria-label={t("usageSync.settings.backend")}
          >
            <SelectValue placeholder={t("usageSync.settings.chooseBackend")} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="webdav" disabled={!availableBackends.webdav}>
              WebDAV
              {!availableBackends.webdav &&
                ` (${t("usageSync.settings.notConfigured")})`}
            </SelectItem>
            <SelectItem value="s3" disabled={!availableBackends.s3}>
              S3
              {!availableBackends.s3 &&
                ` (${t("usageSync.settings.notConfigured")})`}
            </SelectItem>
          </SelectContent>
        </Select>
      </Row>

      <Row
        label={t("usageSync.settings.detailLevel")}
        hint={t("usageSync.settings.detailLevelHint")}
      >
        <Select
          value={settings.detailLevel}
          onValueChange={(value) =>
            void persist({ detailLevel: value as UsageSyncDetailLevel })
          }
          disabled={save.isPending}
        >
          <SelectTrigger
            className="flex-1 text-xs"
            aria-label={t("usageSync.settings.detailLevel")}
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="records">
              {t("usageSync.settings.detailRecords")}
            </SelectItem>
            <SelectItem value="daily">
              {t("usageSync.settings.detailDaily")}
            </SelectItem>
          </SelectContent>
        </Select>
      </Row>

      <Row
        label={t("usageSync.settings.autoSync")}
        hint={t("usageSync.settings.autoSyncHint")}
      >
        <Switch
          checked={settings.autoSync}
          disabled={save.isPending}
          aria-label={t("usageSync.settings.autoSync")}
          onCheckedChange={(autoSync) => void persist({ autoSync })}
        />
        <Select
          value={String(settings.intervalMinutes)}
          onValueChange={(value) =>
            void persist({ intervalMinutes: Number(value) })
          }
          disabled={!settings.autoSync || save.isPending}
        >
          <SelectTrigger
            className="w-40 text-xs"
            aria-label={t("usageSync.settings.interval")}
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {INTERVAL_OPTIONS.map((minutes) => (
              <SelectItem key={minutes} value={String(minutes)}>
                {t("usageSync.settings.everyMinutes", { count: minutes })}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </Row>

      <Row
        label={t("usageSync.settings.deviceName")}
        hint={t("usageSync.settings.deviceNameHint")}
      >
        <Input
          value={nameDraft}
          onChange={(event) => setNameDraft(event.target.value)}
          placeholder={overview.thisDevice.deviceName}
          className="flex-1 text-xs"
          aria-label={t("usageSync.settings.deviceName")}
          maxLength={64}
        />
        <Button
          type="button"
          variant="neutral"
          size="compact"
          disabled={
            setName.isPending ||
            nameDraft.trim() === (overview.thisDevice.customName ?? "")
          }
          onClick={() =>
            setName.mutate(nameDraft.trim() ? nameDraft.trim() : null)
          }
        >
          {t("usageSync.settings.saveName")}
        </Button>
      </Row>

      {settings.enabled && overview.remoteLocation && (
        <p className="m-0 text-xs text-fg-2">
          {t("usageSync.settings.location", {
            location: overview.remoteLocation,
          })}
        </p>
      )}
      {settings.enabled && overview.transportSecure === false && (
        <Notice tone="warning" title={t("usageSync.insecure.title")}>
          {t("usageSync.insecure.body")}
        </Notice>
      )}

      {settings.status.identityConflict && (
        <Notice
          tone="danger"
          title={t("usageSync.conflict.title")}
          actions={
            <div className="flex gap-2">
              <Button
                variant="neutral"
                size="compact"
                disabled={resolveConflict.isPending}
                onClick={() => void handleResolve("newIdentity")}
              >
                {t("usageSync.conflict.newIdentity")}
              </Button>
              <Button
                variant="quiet"
                size="compact"
                disabled={resolveConflict.isPending}
                onClick={() => setConfirmTakeOver(true)}
              >
                {t("usageSync.conflict.takeOver")}
              </Button>
            </div>
          }
        >
          <p className="m-0">{t("usageSync.conflict.body")}</p>
          <ul className="m-0 mt-1.5 flex list-disc flex-col gap-1 ps-5">
            <li>{t("usageSync.conflict.newIdentityEffect")}</li>
            <li>{t("usageSync.conflict.takeOverEffect")}</li>
          </ul>
        </Notice>
      )}

      {settings.enabled && (
        <div className="flex items-center gap-3">
          <div className="min-w-0 flex-1 text-xs text-fg-2">
            {lastSync
              ? t("usageSync.settings.lastSync", { time: lastSync })
              : t("usageSync.settings.neverSynced")}
            {settings.status.lastError && (
              <p className="m-0 mt-1 break-words text-danger-text">
                {settings.status.lastError}
              </p>
            )}
          </div>
          <Button
            type="button"
            variant="neutral"
            size="compact"
            disabled={runSync.isPending || overview.syncing}
            onClick={() => void handleSyncNow()}
          >
            {runSync.isPending || overview.syncing ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <RefreshCw className="h-3.5 w-3.5" />
            )}
            {t("usageSync.syncNow")}
          </Button>
        </div>
      )}

      <ConfirmDialog
        isOpen={confirmTakeOver}
        title={t("usageSync.conflict.takeOverTitle")}
        message={t("usageSync.conflict.takeOverMessage")}
        confirmText={t("usageSync.conflict.takeOver")}
        variant="destructive"
        zIndex="top"
        onConfirm={() => {
          setConfirmTakeOver(false);
          void handleResolve("takeOver");
        }}
        onCancel={() => setConfirmTakeOver(false)}
      />
    </div>
  );
}
