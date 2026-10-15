import React, { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { AlertTriangle, ExternalLink } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { PageTabs } from "@/components/ui/page-tabs";
import { DisabledReason, HelpTip } from "@/components/ui/help-tip";
import {
  useHermesConfigDir,
  useHermesMemory,
  useHermesMemoryLimits,
  useOpenHermesWebUI,
  useSaveHermesMemory,
  useToggleHermesMemoryEnabled,
} from "@/hooks/useHermes";
import { shortenHomePath } from "@/components/sessions/utils";
import type { HermesMemoryKind } from "@/types";
import { fieldClass } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import {
  hermesMemoryDrafts,
  isHermesMemoryDirty,
  useHermesMemoryDrafts,
} from "./hermesMemoryDrafts";

const KINDS: HermesMemoryKind[] = ["memory", "user"];

/** 每份记忆的文件名和文案键；上限缺省值与 Hermes 自带的默认值一致。 */
const KIND_META: Record<
  HermesMemoryKind,
  {
    file: string;
    nameKey: string;
    descKey: string;
    placeholderKey: string;
    defaultLimit: number;
  }
> = {
  memory: {
    file: "MEMORY.md",
    nameKey: "hermes.memory.agentName",
    descKey: "hermes.memory.agentDesc",
    placeholderKey: "hermes.memory.agentPlaceholder",
    defaultLimit: 2200,
  },
  user: {
    file: "USER.md",
    nameKey: "hermes.memory.userName",
    descKey: "hermes.memory.userDesc",
    placeholderKey: "hermes.memory.userPlaceholder",
    defaultLimit: 1375,
  },
};

function memoryFilePath(configDir: string | undefined, file: string) {
  const dir = configDir ? shortenHomePath(configDir) : "~/.hermes";
  const sep = dir.includes("\\") && !dir.includes("/") ? "\\" : "/";
  return `${dir.replace(/[\\/]+$/, "")}${sep}memories${sep}${file}`;
}

/**
 * 页头的「保存」（记忆页唯一的 solid 按钮）：保存当前页签那份记忆。
 * 「Hermes 下次启动或新建会话时生效」是后果，写在保存成功的 toast 里。
 */
export const HermesMemorySaveButton: React.FC = () => {
  const { t } = useTranslation();
  const drafts = useHermesMemoryDrafts();
  const saveMutation = useSaveHermesMemory();
  const kind = drafts.active;
  const dirty = isHermesMemoryDirty(drafts, kind);
  const saving = saveMutation.isPending;

  const handleSave = async () => {
    const content = hermesMemoryDrafts.getState().draft[kind];
    if (content === undefined) return;
    try {
      await saveMutation.mutateAsync({ kind, content });
      hermesMemoryDrafts.markSaved(kind, content);
      toast.success(t("hermes.memory.saveSuccess"));
    } catch {
      // useSaveHermesMemory 已经弹出带原因的错误 toast
    }
  };

  return (
    <DisabledReason
      reason={!dirty && !saving ? t("hermes.memory.noChanges") : undefined}
      align="end"
    >
      <Button
        variant="solid"
        size="regular"
        className="px-4"
        disabled={saving}
        onClick={() => void handleSave()}
      >
        {saving ? t("common.saving") : t("common.save")}
      </Button>
    </DisabledReason>
  );
};

interface MemoryTabPaneProps {
  kind: HermesMemoryKind;
  limit: number;
  enabled: boolean;
  configDir?: string;
}

const MemoryTabPane: React.FC<MemoryTabPaneProps> = ({
  kind,
  limit,
  enabled,
  configDir,
}) => {
  const { t } = useTranslation();
  const meta = KIND_META[kind];
  const name = t(meta.nameKey);
  const { data, isLoading, isError } = useHermesMemory(kind, true);
  const drafts = useHermesMemoryDrafts();
  const toggleMutation = useToggleHermesMemoryEnabled();

  // 只在第一次读到文件时填草稿；保存后的重新读取不覆盖正在编辑的内容
  useEffect(() => {
    if (data !== undefined) hermesMemoryDrafts.hydrate(kind, data);
  }, [data, kind]);

  const content = drafts.draft[kind];
  const loaded = content !== undefined;
  const count = content?.length ?? 0;
  const remaining = limit - count;
  const ratio = limit > 0 ? count / limit : 0;
  const isOver = remaining < 0;
  const isLow = !isOver && ratio > 0.9;
  // 和授权中心的额度条同一套：轨道 --chart-grid，填充画「剩余」
  const remainingPct =
    limit > 0 ? Math.max(0, Math.min(100, (remaining / limit) * 100)) : 0;

  const handleToggle = (next: boolean) => {
    toggleMutation.mutate(
      { kind, enabled: next },
      {
        onSuccess: () =>
          toast.success(
            t(next ? "hermes.memory.toggledOn" : "hermes.memory.toggledOff", {
              name,
            }),
          ),
      },
    );
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3">
      <div
        className={cn(
          "flex h-11 shrink-0 items-center gap-2 rounded-panel pe-3.5 ps-3",
          enabled ? "bg-subtle" : "bg-warning-soft",
        )}
      >
        <Switch
          checked={enabled}
          disabled={toggleMutation.isPending}
          aria-label={t("hermes.memory.enableLabel", { name })}
          onCheckedChange={handleToggle}
          className="me-1"
        />
        <div className="flex min-w-0 flex-1 items-center gap-0.5">
          <span className="whitespace-nowrap text-body font-medium text-fg-1">
            {t("hermes.memory.enableLabel", { name })}
          </span>
          <HelpTip title={t("hermes.memory.helpTitle", { name })}>
            {t("hermes.memory.helpBody", {
              desc: t(meta.descKey),
              rule: t("hermes.memory.separatorRule"),
            })}
          </HelpTip>
        </div>
        {enabled ? (
          <code className="shrink-0 truncate font-mono text-caption text-fg-2">
            {memoryFilePath(configDir, meta.file)}
          </code>
        ) : (
          <span className="flex shrink-0 items-center gap-1.5 whitespace-nowrap text-caption text-warning-text">
            <AlertTriangle className="h-3.5 w-3.5" strokeWidth={1.5} />
            {t("hermes.memory.disabledHint")}
          </span>
        )}
      </div>

      {isError && !loaded ? (
        <div className="flex min-h-[200px] flex-1 items-center justify-center rounded-panel border border-border text-body text-danger-text">
          {t("hermes.memory.loadFailed")}
        </div>
      ) : (
        <textarea
          aria-label={t("hermes.memory.editorLabel", {
            name,
            file: meta.file,
          })}
          aria-busy={isLoading && !loaded}
          value={content ?? ""}
          disabled={!loaded}
          onChange={(event) =>
            hermesMemoryDrafts.edit(kind, event.target.value)
          }
          placeholder={loaded ? t(meta.placeholderKey) : t("prompts.loading")}
          spellCheck={false}
          className={cn(
            fieldClass,
            "min-h-[200px] flex-1 resize-none px-3.5 py-3 font-mono leading-[21px]",
          )}
        />
      )}

      <div className="flex h-11 shrink-0 items-center gap-3">
        <span
          role="meter"
          aria-label={t("hermes.memory.meterLabel")}
          aria-valuemin={0}
          aria-valuemax={limit}
          aria-valuenow={Math.max(0, remaining)}
          className="relative h-1.5 w-24 shrink-0 overflow-hidden rounded-full bg-chart-grid"
        >
          <span
            className={cn(
              "absolute inset-y-0 start-0 rounded-full",
              isLow ? "bg-warning" : "bg-chart-1",
            )}
            style={{ width: `${remainingPct}%` }}
          />
        </span>
        <span
          className={cn(
            "whitespace-nowrap text-caption tabular-nums",
            isOver
              ? "font-medium text-danger-text"
              : isLow
                ? "font-medium text-warning-text"
                : "text-fg-2",
          )}
        >
          {isOver
            ? t("hermes.memory.overLimitBy", {
                over: (-remaining).toLocaleString(),
                limit: limit.toLocaleString(),
              })
            : t("hermes.memory.remaining", {
                remaining: remaining.toLocaleString(),
                limit: limit.toLocaleString(),
              })}
        </span>
      </div>
    </div>
  );
};

interface HermesMemoryPanelProps {
  /** 打开 Hermes Web UI；App 传入带「未启动 → 确认后启动」的版本 */
  onOpenWebUI?: (path?: string) => void | Promise<void>;
}

const HermesMemoryPanel: React.FC<HermesMemoryPanelProps> = ({
  onOpenWebUI,
}) => {
  const { t } = useTranslation();
  const drafts = useHermesMemoryDrafts();
  const activeTab = drafts.active;
  const fallbackOpenWebUI = useOpenHermesWebUI();
  const openWebUI = onOpenWebUI ?? fallbackOpenWebUI;
  const { data: limits } = useHermesMemoryLimits(true);
  const { data: configDir } = useHermesConfigDir(true);

  // 离开记忆页就丢掉草稿，回来时重新读文件
  useEffect(() => () => hermesMemoryDrafts.reset(), []);

  const limitOf = (kind: HermesMemoryKind) =>
    (kind === "memory" ? limits?.memory : limits?.user) ??
    KIND_META[kind].defaultLimit;
  const enabledOf = (kind: HermesMemoryKind) =>
    (kind === "memory" ? limits?.memoryEnabled : limits?.userEnabled) ?? true;

  // 两份记忆是两份文件、换的是整块内容：用二级下划线页签。两个面板都保持挂载，
  // 草稿也各存一份，切过去再切回来不会丢掉还没保存的修改；有未保存修改的那份
  // 页签上带一个小圆点。
  return (
    <div className="flex min-h-full flex-col gap-3 px-6 pb-2 pt-3">
      <PageTabs
        size="sm"
        aria-label={t("hermes.memory.fileTabs")}
        idPrefix="hermes-memory"
        value={activeTab}
        onValueChange={(kind) => hermesMemoryDrafts.setActive(kind)}
        className="shrink-0"
        items={KINDS.map((kind) => ({
          value: kind,
          label: (
            <>
              <span>{t(KIND_META[kind].nameKey)}</span>
              <span className="font-mono text-badge font-normal text-fg-3">
                {KIND_META[kind].file}
              </span>
              {isHermesMemoryDirty(drafts, kind) && (
                <span
                  role="img"
                  aria-label={t("hermes.memory.unsaved")}
                  title={t("hermes.memory.unsaved")}
                  className="h-1.5 w-1.5 shrink-0 rounded-full bg-warning"
                />
              )}
            </>
          ),
        }))}
        trailing={
          <Button
            variant="quiet"
            size="compact"
            className="text-fg-2"
            onClick={() => void openWebUI("/config")}
          >
            {t("hermes.memory.openConfig")}
            <ExternalLink className="h-3.5 w-3.5" />
          </Button>
        }
      />

      {KINDS.map((kind) => (
        <div
          key={kind}
          role="tabpanel"
          id={`hermes-memory-panel-${kind}`}
          aria-labelledby={`hermes-memory-${kind}`}
          hidden={activeTab !== kind}
          className={cn(
            "min-h-0 flex-1 flex-col",
            activeTab === kind ? "flex" : "hidden",
          )}
        >
          <MemoryTabPane
            kind={kind}
            limit={limitOf(kind)}
            enabled={enabledOf(kind)}
            configDir={configDir}
          />
        </div>
      ))}
    </div>
  );
};

export default HermesMemoryPanel;
