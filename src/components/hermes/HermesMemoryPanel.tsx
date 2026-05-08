import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { ExternalLink } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
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
          "flex items-center justify-between px-3 py-2 rounded-md border",
          enabled ? "bg-muted/30" : "bg-amber-500/10 border-amber-500/30",
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
          <span className="text-sm">
            {enabled
              ? t("hermes.memory.enableOn")
              : t("hermes.memory.enableOff")}
          </span>
        </div>
        {!enabled && (
          <span className="text-xs text-amber-700 dark:text-amber-400">
            {t("hermes.memory.disabledHint")}
          </span>
        )}
      </div>

      {isLoading && !loaded ? (
        <div className="flex items-center justify-center h-64 text-muted-foreground">
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

      <div className="flex items-center justify-between gap-3 text-sm">
        <span
          className={cn(
            "text-muted-foreground",
            isOver && "text-red-600 dark:text-red-400 font-medium",
          )}
        >
          {t("hermes.memory.usage", { current: charCount, limit })}
          {isOver ? ` — ${t("hermes.memory.overLimit")}` : ""}
        </span>
        <div className="flex items-center gap-3">
          <span className="hidden md:inline text-xs text-muted-foreground">
            {t("hermes.memory.runtimeNote")}
          </span>
          <Button
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

  return (
    <div className="flex flex-col h-full">
      <Tabs
        value={activeTab}
        onValueChange={(v) => setActiveTab(v as HermesMemoryKind)}
        className="flex-1 flex flex-col"
      >
        <div className="px-6 pt-4 flex items-center justify-between gap-3 flex-wrap">
          <TabsList>
            <TabsTrigger value="memory">
              {t("hermes.memory.agentTab")}
            </TabsTrigger>
            <TabsTrigger value="user">{t("hermes.memory.userTab")}</TabsTrigger>
          </TabsList>
          <Button
            variant="outline"
            size="sm"
            onClick={() => void openHermesWebUI("/config")}
          >
            <ExternalLink className="w-3.5 h-3.5 mr-1" />
            {t("hermes.memory.openConfig")}
          </Button>
        </div>

        <TabsContent value="memory" className="flex-1 px-6 pb-4 mt-4">
          <MemoryTabPane
            kind="memory"
            limit={memoryLimit}
            enabled={memoryEnabled}
          />
        </TabsContent>
        <TabsContent value="user" className="flex-1 px-6 pb-4 mt-4">
          <MemoryTabPane kind="user" limit={userLimit} enabled={userEnabled} />
        </TabsContent>
      </Tabs>
    </div>
  );
};

export default HermesMemoryPanel;
