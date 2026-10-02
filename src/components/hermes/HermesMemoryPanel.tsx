import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { ExternalLink } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { PageTabs } from "@/components/ui/page-tabs";
import MarkdownEditor from "@/components/MarkdownEditor";
import {
  useHermesMemory,
  useHermesMemoryLimits,
  useOpenHermesWebUI,
  useSaveHermesMemory,
  useToggleHermesMemoryEnabled,
} from "@/hooks/useHermes";
import { useDarkMode } from "@/hooks/useDarkMode";
import type { HermesMemoryKind } from "@/types";
import { cn } from "@/lib/utils";

interface MemoryTabPaneProps {
  kind: HermesMemoryKind;
  limit: number;
  enabled: boolean;
}

const MemoryTabPane: React.FC<MemoryTabPaneProps> = ({
  kind,
  limit,
  enabled,
}) => {
  const { t } = useTranslation();
  const darkMode = useDarkMode();
  const { data, isLoading } = useHermesMemory(kind, true);
  const saveMutation = useSaveHermesMemory();
  const toggleMutation = useToggleHermesMemoryEnabled();
  const [content, setContent] = useState("");
  const [loaded, setLoaded] = useState(false);

  // Hydrate local dirty buffer from query data only on first load. Later
  // refetches (e.g. after a successful save) must not clobber in-flight user
  // edits — the caller owns `content` until they click Save again.
  useEffect(() => {
    if (!loaded && data !== undefined) {
      setContent(data);
      setLoaded(true);
    }
  }, [data, loaded]);

  const handleSave = async () => {
    try {
      await saveMutation.mutateAsync({ kind, content });
      toast.success(t("hermes.memory.saveSuccess"));
    } catch {
      // useSaveHermesMemory already surfaces a localized error toast.
    }
  };

  const charCount = content.length;
  const isOver = charCount > limit;

  return (
    <div className="flex flex-col gap-3">
      <div
        className={cn(
          "flex items-center justify-between rounded-panel px-3 py-2",
          enabled ? "bg-subtle" : "bg-warning-soft",
        )}
      >
        <div className="flex items-center gap-2">
          <Switch
            checked={enabled}
            disabled={toggleMutation.isPending}
            onCheckedChange={(next) =>
              toggleMutation.mutate({ kind, enabled: next })
            }
          />
          <span className="text-body font-medium text-fg-1">
            {enabled
              ? t("hermes.memory.enableOn")
              : t("hermes.memory.enableOff")}
          </span>
        </div>
        {!enabled && (
          <span className="text-caption text-warning-text">
            {t("hermes.memory.disabledHint")}
          </span>
        )}
      </div>

      {isLoading && !loaded ? (
        <div className="flex items-center justify-center h-64 text-fg-2">
          {t("prompts.loading")}
        </div>
      ) : (
        <MarkdownEditor
          value={content}
          onChange={setContent}
          darkMode={darkMode}
          minHeight="calc(100vh - 320px)"
        />
      )}

      <div className="flex items-center justify-between gap-3 text-body">
        <span
          className={cn("text-fg-2", isOver && "text-danger-text font-medium")}
        >
          {t("hermes.memory.usage", { current: charCount, limit })}
          {isOver ? ` — ${t("hermes.memory.overLimit")}` : ""}
        </span>
        <div className="flex items-center gap-3">
          <span className="hidden text-caption text-fg-2 md:inline">
            {t("hermes.memory.runtimeNote")}
          </span>
          <Button
            variant="solid"
            size="regular"
            onClick={handleSave}
            disabled={saveMutation.isPending || !loaded}
          >
            {saveMutation.isPending ? t("common.saving") : t("common.save")}
          </Button>
        </div>
      </div>
    </div>
  );
};

const HermesMemoryPanel: React.FC = () => {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<HermesMemoryKind>("memory");
  const openHermesWebUI = useOpenHermesWebUI();
  const { data: limits } = useHermesMemoryLimits(true);

  const memoryLimit = limits?.memory ?? 2200;
  const userLimit = limits?.user ?? 1375;
  const memoryEnabled = limits?.memoryEnabled ?? true;
  const userEnabled = limits?.userEnabled ?? true;

  // 两份记忆是两份文件、换的是整块内容：用二级下划线页签（挂在「供应商 / 记忆」
  // 一级页签下面，小一号、不画整行底线）。两个面板都保持挂载，切过去再切回来
  // 不会丢掉还没保存的修改。
  return (
    <div className="flex h-full flex-col">
      <div className="px-6 pt-1">
        <PageTabs
          size="sm"
          aria-label={t("hermes.memory.title")}
          idPrefix="hermes-memory"
          value={activeTab}
          onValueChange={setActiveTab}
          items={[
            { value: "memory", label: t("hermes.memory.agentTab") },
            { value: "user", label: t("hermes.memory.userTab") },
          ]}
          trailing={
            <Button
              variant="quiet"
              size="compact"
              className="text-fg-2"
              onClick={() => void openHermesWebUI("/config")}
            >
              {t("hermes.memory.openConfig")}
              <ExternalLink className="h-3.5 w-3.5" />
            </Button>
          }
        />
      </div>

      {(["memory", "user"] as const).map((kind) => (
        <div
          key={kind}
          role="tabpanel"
          id={`hermes-memory-panel-${kind}`}
          aria-labelledby={`hermes-memory-${kind}`}
          hidden={activeTab !== kind}
          className="mt-3 flex-1 px-6 pb-4"
        >
          <MemoryTabPane
            kind={kind}
            limit={kind === "memory" ? memoryLimit : userLimit}
            enabled={kind === "memory" ? memoryEnabled : userEnabled}
          />
        </div>
      ))}
    </div>
  );
};

export default HermesMemoryPanel;
