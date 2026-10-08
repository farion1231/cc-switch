import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ChevronRight, Loader2 } from "lucide-react";
import {
  Sheet,
  SheetBody,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import { Switch } from "@/components/ui/switch";
import { useGlobalProxyConfig } from "@/lib/query/proxy";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { KNOWN_APP_TYPES } from "@/types/usage";
import { cn } from "@/lib/utils";
import { getResolvedLang, joinNames } from "./format";

interface UsageDataSourcesSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  sessionAutoSyncEnabled: boolean;
  onSessionAutoSyncEnabledChange?: (next: boolean) => void;
  /** 「刚刚同步」「N 分钟前同步」；还没手动同步过时为空 */
  syncedLabel?: string;
  syncing: boolean;
  onSyncNow: () => void;
  /** 打开设置 → 本地路由（「记录请求用量」开关在那里） */
  onOpenRoutingSettings?: () => void;
  rebuildingCodex: boolean;
  /** 只负责打开确认框；确认框里写清后果 */
  onRebuildCodex: () => void;
}

function SourceCard({
  title,
  help,
  trailing,
  children,
}: {
  title: string;
  help: { title: string; body: string };
  trailing?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <section className="flex flex-col gap-3 rounded-panel border border-border px-4 py-3.5">
      <div className="flex items-center gap-2">
        <div className="flex min-w-0 flex-1 items-center gap-0.5">
          <h3 className="m-0 truncate text-strong font-semibold text-fg-1">
            {title}
          </h3>
          <HelpTip title={help.title}>{help.body}</HelpTip>
        </div>
        {trailing}
      </div>
      {children}
    </section>
  );
}

function LoggingStatus() {
  const { t } = useTranslation();
  const { data: config, isLoading } = useGlobalProxyConfig();
  if (isLoading || !config) {
    return <Loader2 className="h-4 w-4 animate-spin text-fg-3" />;
  }
  const on = config.enableLogging;
  return (
    <span
      className={cn(
        "rounded-[5px] px-1.5 text-badge leading-5",
        on ? "bg-success-soft text-success-text" : "bg-subtle text-fg-2",
      )}
    >
      {on ? t("usage.sources.on") : t("usage.sources.off")}
    </span>
  );
}

/** 「数据来源」抽屉（v7 S6）：会话日志扫描、路由请求日志（只读）、Codex 用量维护。 */
export function UsageDataSourcesSheet({
  open,
  onOpenChange,
  sessionAutoSyncEnabled,
  onSessionAutoSyncEnabledChange,
  syncedLabel,
  syncing,
  onSyncNow,
  onOpenRoutingSettings,
  rebuildingCodex,
  onRebuildCodex,
}: UsageDataSourcesSheetProps) {
  const { t, i18n } = useTranslation();
  const coveredApps = joinNames(
    KNOWN_APP_TYPES.map((app) => APP_DISPLAY_NAME[app]),
    getResolvedLang(i18n),
  );
  const cadence = sessionAutoSyncEnabled
    ? t("usage.sources.cadenceOn")
    : t("usage.sources.cadenceOff");

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent
        width={400}
        closeLabel={t("common.close")}
        dismissOnOutsideClick
      >
        <SheetHeader className="pb-3">
          <SheetTitle>{t("usage.dataSources")}</SheetTitle>
          <SheetDescription className="sr-only">
            {t("usage.dataSources")}
          </SheetDescription>
        </SheetHeader>
        <SheetBody className="flex flex-col gap-3 border-t border-border">
          <SourceCard
            title={t("usage.sources.scanTitle")}
            help={{
              title: t("usage.sources.scanHelpTitle"),
              body: t("usage.sources.scanHelp"),
            }}
            trailing={
              <Switch
                checked={sessionAutoSyncEnabled}
                onCheckedChange={(value) =>
                  onSessionAutoSyncEnabledChange?.(value)
                }
                aria-label={t("usage.sources.scanTitle")}
              />
            }
          >
            <ul className="m-0 flex list-none flex-col gap-1 rounded-control bg-subtle px-3 py-2.5 text-caption text-fg-2">
              <li>{t("usage.sources.coverage", { apps: coveredApps })}</li>
              <li>{syncedLabel ? `${cadence} · ${syncedLabel}` : cadence}</li>
              <li>{t("usage.sources.unsupported")}</li>
            </ul>
            <div className="flex justify-end">
              <Button
                type="button"
                variant="neutral"
                size="compact"
                disabled={syncing}
                onClick={onSyncNow}
              >
                {syncing && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
                {t("usage.sessionSync.syncNow")}
              </Button>
            </div>
          </SourceCard>

          <SourceCard
            title={t("routingSettings.logging")}
            help={{
              title: t("usage.sources.loggingHelpTitle"),
              body: t("usage.sources.loggingHelp"),
            }}
            trailing={open ? <LoggingStatus /> : null}
          >
            {onOpenRoutingSettings && (
              <button
                type="button"
                className="inline-flex w-fit items-center gap-0.5 rounded-[4px] text-body font-medium text-fg-1 underline decoration-border-strong underline-offset-4 hover:decoration-fg-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                onClick={() => {
                  onOpenChange(false);
                  onOpenRoutingSettings();
                }}
              >
                {t("usage.sources.editLogging")}
                <ChevronRight className="h-3.5 w-3.5" />
              </button>
            )}
          </SourceCard>

          <SourceCard
            title={t("usage.rebuildCodex.title")}
            help={{
              title: t("usage.sources.codexHelpTitle"),
              body: t("usage.rebuildCodex.description"),
            }}
          >
            <p className="m-0 text-caption text-fg-2">
              {t("usage.rebuildCodex.warning")}
            </p>
            <div className="flex justify-end">
              <Button
                type="button"
                variant="neutral"
                size="compact"
                disabled={rebuildingCodex}
                onClick={onRebuildCodex}
              >
                {rebuildingCodex && (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                )}
                {t("usage.rebuildCodex.actionEllipsis")}
              </Button>
            </div>
          </SourceCard>
        </SheetBody>
      </SheetContent>
    </Sheet>
  );
}
