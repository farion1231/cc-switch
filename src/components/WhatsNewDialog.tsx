import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { ChevronRight, ExternalLink, Sparkles } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { useNewLayoutNoticePending } from "@/components/shell/NewLayoutDialog";
import { settingsApi } from "@/lib/api";
import { useSettingsQuery } from "@/lib/query";
import {
  WHATS_NEW_ENTRIES,
  changelogUrl,
  entriesSince,
  isNewerThanSeen,
  markSeen,
  resolveWhatsNewLanguage,
  type WhatsNewEntry,
} from "@/lib/whatsNew";

/** 默认展开的版本数，更早的折叠起来，跳了很多版本也不会撑满弹窗 */
const EXPANDED_VERSIONS = 3;

interface WhatsNewDialogProps {
  open: boolean;
  onClose: () => void;
  /** 新到旧 */
  entries: WhatsNewEntry[];
  /** 启动弹出时传当前版本，标题写「已更新到 vX」；不传是关于页的「近期更新」 */
  updatedTo?: string;
  /** 上次看过的版本，副标题写「自 vX 以来的更新」 */
  since?: string;
}

export function WhatsNewDialog({
  open,
  onClose,
  entries,
  updatedTo,
  since,
}: WhatsNewDialogProps) {
  const { t, i18n } = useTranslation();
  const language = resolveWhatsNewLanguage(i18n.language);
  const [showAll, setShowAll] = useState(false);

  const visible = showAll ? entries : entries.slice(0, EXPANDED_VERSIONS);
  const hiddenCount = entries.length - visible.length;

  const openExternal = (url: string) => {
    settingsApi.openExternal(url).catch((error) => {
      console.error("Failed to open changelog:", error);
    });
  };

  const handleViewFull = () => {
    // 只有一个版本就直接打开这一版，多个版本打开更新日志列表
    openExternal(
      changelogUrl(
        language,
        entries.length === 1 ? entries[0].version : undefined,
      ),
    );
    onClose();
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
    >
      <DialogContent
        className="max-w-md"
        zIndex="top"
        {...(since ? {} : { "aria-describedby": undefined })}
      >
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Sparkles className="h-5 w-5 text-fg-1" />
            {updatedTo
              ? t("whatsNew.updatedTo", { version: updatedTo })
              : t("whatsNew.recentTitle")}
          </DialogTitle>
          {since && (
            <DialogDescription>
              {t("whatsNew.since", { version: since })}
            </DialogDescription>
          )}
        </DialogHeader>
        <div className="min-h-0 flex-1 space-y-5 overflow-y-auto px-6 py-4">
          {visible.map((entry) => (
            <section key={entry.version}>
              <div className="flex items-center justify-between gap-3">
                <h3 className="text-strong tabular-nums text-fg-1">
                  v{entry.version}
                </h3>
                <button
                  type="button"
                  className="inline-flex items-center gap-1 text-caption text-fg-2 hover:text-fg-1"
                  onClick={() =>
                    openExternal(changelogUrl(language, entry.version))
                  }
                >
                  {t("whatsNew.versionDetails")}
                  <ExternalLink className="h-3 w-3" />
                </button>
              </div>
              <ul className="mt-2 space-y-1.5">
                {entry.items.map((item, index) => (
                  <li
                    key={index}
                    className="flex items-start gap-2 text-body text-fg-1"
                  >
                    <span className="mt-0.5 shrink-0 rounded-control bg-subtle px-1.5 text-badge text-fg-2">
                      {t(`whatsNew.types.${item.type}`)}
                    </span>
                    <span className="min-w-0">{item[language]}</span>
                  </li>
                ))}
              </ul>
            </section>
          ))}
          {hiddenCount > 0 && (
            <button
              type="button"
              className="inline-flex items-center gap-1 text-caption text-fg-2 hover:text-fg-1"
              onClick={() => setShowAll(true)}
            >
              <ChevronRight className="h-3.5 w-3.5" />
              {t("whatsNew.showEarlier", { count: hiddenCount })}
            </button>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={handleViewFull}>
            {t("whatsNew.viewFull")}
          </Button>
          <Button onClick={onClose}>{t("whatsNew.confirm")}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/**
 * 启动时的更新摘要：当前版本比 settings.json 里记的 whatsNewSeenVersion 新，
 * 就把这之间有内容的版本弹给用户看，关掉后记下当前版本。
 * 排在欢迎弹窗和「界面改版了」后面，那两个确认时会顺手记下当前版本，不会接着弹。
 */
export function WhatsNewNotice() {
  const queryClient = useQueryClient();
  const { data: settings } = useSettingsQuery();
  const newLayoutPending = useNewLayoutNoticePending(settings);
  const { data: currentVersion } = useQuery({
    queryKey: ["app-version"],
    queryFn: () => getVersion(),
    staleTime: Infinity,
  });
  const [closed, setClosed] = useState(false);
  const stampedRef = useRef(false);

  const seen = settings?.whatsNewSeenVersion;
  const due =
    settings != null &&
    settings.firstRunNoticeConfirmed === true &&
    newLayoutPending === false &&
    !!currentVersion &&
    isNewerThanSeen(currentVersion, seen);

  const entries = useMemo(
    () =>
      due && currentVersion
        ? entriesSince(WHATS_NEW_ENTRIES, seen, currentVersion)
        : [],
    [due, seen, currentVersion],
  );

  // 打开时把内容定格：确认后设置刷新、区间变空，关闭动画里内容不会被清掉
  const [shown, setShown] = useState<{
    entries: WhatsNewEntry[];
    since?: string;
  } | null>(null);
  if (shown === null && entries.length > 0) {
    setShown({ entries, since: seen });
  }

  const acknowledge = useCallback(async () => {
    setClosed(true);
    if (!settings || !currentVersion) return;
    try {
      const { webdavSync: _, ...rest } = settings;
      await settingsApi.save({
        ...rest,
        whatsNewSeenVersion: markSeen(
          settings.whatsNewSeenVersion,
          currentVersion,
        ),
      });
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
    } catch (error) {
      console.error("Failed to save whatsNewSeenVersion:", error);
    }
  }, [settings, currentVersion, queryClient]);

  // 区间里没有可显示的内容：静默记下当前版本，下次升级才有准确的起点
  useEffect(() => {
    if (!due || entries.length > 0 || stampedRef.current) return;
    stampedRef.current = true;
    void acknowledge();
  }, [due, entries.length, acknowledge]);

  if (!shown) return null;

  return (
    <WhatsNewDialog
      open={!closed}
      onClose={() => void acknowledge()}
      entries={shown.entries}
      updatedTo={currentVersion}
      since={shown.since}
    />
  );
}
