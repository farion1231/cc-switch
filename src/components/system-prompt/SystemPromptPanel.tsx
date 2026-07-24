import React, { useEffect, useState, useCallback } from "react";
import { FileText, BookOpen, FolderOpen, RotateCcw, Save } from "lucide-react";
import type { AppId } from "@/lib/api";
import { systemPromptApi } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { toast } from "sonner";
import MarkdownEditor from "@/components/MarkdownEditor";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import {
  useSystemPromptFile,
  useSaveSystemPromptFile,
  useInjectionToggle,
  useSetInjectionToggle,
  useSharedPrompt,
  useSaveSharedPrompt,
} from "@/lib/query/systemPrompt";
import { useQueryClient } from "@tanstack/react-query";

interface SystemPromptPanelProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  appId: AppId;
}

const SUPPORTED_APPS: { id: AppId; label: string; file: string; dir: string }[] = [
  { id: "claude", label: "Claude", file: "CLAUDE.md", dir: "~/.claude/" },
  { id: "codex", label: "Codex", file: "AGENTS.md", dir: "~/.codex/" },
  { id: "gemini", label: "Gemini", file: "GEMINI.md", dir: "~/.gemini/" },
  { id: "grokbuild", label: "Grok", file: "AGENTS.md", dir: "~/.grok/" },
  { id: "opencode", label: "OpenCode", file: "AGENTS.md", dir: "~/.config/opencode/" },
  { id: "openclaw", label: "OpenClaw", file: "AGENTS.md", dir: "~/.openclaw/" },
  { id: "hermes", label: "Hermes", file: "AGENTS.md", dir: "%LOCALAPPDATA%/hermes/" },
];

type PanelTab = "per-app" | "shared";

const SystemPromptPanel = React.forwardRef<unknown, SystemPromptPanelProps>(
  ({ open, onOpenChange }, _ref) => {
    const [activeTab, setActiveTab] = useState<PanelTab>("per-app");
    const [selectedApp, setSelectedApp] = useState<AppId>("claude");
    const queryClient = useQueryClient();

    // 专属配置
    const { data: fileContent = "", isLoading: fileLoading } =
      useSystemPromptFile(selectedApp);
    const saveFile = useSaveSystemPromptFile(selectedApp);
    const { data: toggle, isLoading: toggleLoading } =
      useInjectionToggle(selectedApp);
    const setToggle = useSetInjectionToggle(selectedApp);
    const [editorContent, setEditorContent] = useState("");
    const [hasUnsaved, setHasUnsaved] = useState(false);

    useEffect(() => {
      if (!fileLoading) { setEditorContent(fileContent); setHasUnsaved(false); }
    }, [fileContent, fileLoading]);

    const handleSave = useCallback(async () => {
      await saveFile.mutateAsync(editorContent);
      setHasUnsaved(false);
    }, [editorContent, saveFile]);

    // 共享规则
    const { data: sharedContent = "", isLoading: sharedLoading } = useSharedPrompt();
    const saveShared = useSaveSharedPrompt();
    const [sharedEditor, setSharedEditor] = useState("");
    const [sharedUnsaved, setSharedUnsaved] = useState(false);

    useEffect(() => {
      if (!sharedLoading) { setSharedEditor(sharedContent); setSharedUnsaved(false); }
    }, [sharedContent, sharedLoading]);

    const handleSaveShared = useCallback(async () => {
      await saveShared.mutateAsync(sharedEditor);
      setSharedUnsaved(false);
    }, [sharedEditor, saveShared]);

    const app = SUPPORTED_APPS.find((a) => a.id === selectedApp);

    const footer = activeTab === "per-app" ? (
      <div className="flex justify-end gap-2">
        <Button
          variant="outline"
          onClick={() => {
            setEditorContent(fileContent);
            setHasUnsaved(false);
            toast.success("已重新加载");
          }}
        >
          <RotateCcw className="w-4 h-4 mr-2" />
          重新加载
        </Button>
        <Button
          onClick={async () => {
            await handleSave();
            toast.success("已保存");
            onOpenChange(false);
          }}
          disabled={!hasUnsaved || saveFile.isPending}
          className="bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <Save className="w-4 h-4 mr-2" />
          {saveFile.isPending ? "保存中..." : "保存"}
        </Button>
      </div>
    ) : (
      <div className="flex justify-end gap-2">
        <Button
          variant="outline"
          onClick={() => {
            setSharedEditor(sharedContent);
            setSharedUnsaved(false);
            toast.success("已重新加载");
          }}
        >
          <RotateCcw className="w-4 h-4 mr-2" />
          重新加载
        </Button>
        <Button
          onClick={async () => {
            await handleSaveShared();
            toast.success("已保存");
            onOpenChange(false);
          }}
          disabled={!sharedUnsaved || saveShared.isPending}
          className="bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <Save className="w-4 h-4 mr-2" />
          {saveShared.isPending ? "保存中..." : "保存"}
        </Button>
      </div>
    );

    return (
      <FullScreenPanel
        isOpen={open}
        onClose={() => onOpenChange(false)}
        title={"System 注入"}
        footer={footer}
      >
        <Tabs value={activeTab} onValueChange={(v) => setActiveTab(v as PanelTab)} className="w-full">
          <div className="flex items-center justify-between mb-4">
            <TabsList>
              <TabsTrigger value="per-app"><FileText className="w-4 h-4 mr-1" />专属配置</TabsTrigger>
              <TabsTrigger value="shared"><BookOpen className="w-4 h-4 mr-1" />统一规则</TabsTrigger>
            </TabsList>
            <div className="text-right">
              <span className="text-xs text-muted-foreground">
                每轮对话强制注入 system prompt，避免规则在多轮后被淡化
              </span>
              <br />
              <span className="text-[10px] text-muted-foreground/60">
                设置 → 路由 → 本地路由 → 路由总开关 → 选择要路由的应用
              </span>
            </div>
          </div>

          {activeTab === "per-app" && (
            <div className="flex flex-col gap-4">
              {/* 应用选择 */}
              <Tabs value={selectedApp} onValueChange={(v) => {
                if (hasUnsaved) {
                  toast.warning("有未保存的修改，已丢弃");
                }
                setSelectedApp(v as AppId);
                setHasUnsaved(false);
              }}>
                <TabsList className="w-full justify-start overflow-x-auto">
                  {SUPPORTED_APPS.map((a) => (
                    <TabsTrigger key={a.id} value={a.id} className="text-xs px-3">
                      {a.label}
                    </TabsTrigger>
                  ))}
                </TabsList>
              </Tabs>

              {/* 开关 */}
              {!toggleLoading && toggle && (
                <div className="flex items-center gap-6 text-sm py-2 px-1">
                  <label className="flex items-center gap-2 cursor-pointer select-none">
                    <Switch checked={toggle.enabled} onCheckedChange={(v) => setToggle.mutate({ ...toggle, enabled: v })} />
                    <span>注入开关</span>
                  </label>
                  <label className="flex items-center gap-2 cursor-pointer select-none">
                    <Switch checked={toggle.receiveShared} onCheckedChange={(v) => setToggle.mutate({ ...toggle, receiveShared: v })} />
                    <span>接受统一规则</span>
                  </label>
                </div>
              )}

              {/* 文件路径提示 */}
              <div className="flex items-center justify-between text-xs text-muted-foreground">
                <span className="truncate mr-2">
                  {toggle?.customFilePath
                    ? `📄 ${toggle.customFilePath}`
                    : `📄 ${app?.dir ?? "~/.claude/"}${app?.file ?? "CLAUDE.md"}`}
                </span>
                <div className="flex items-center gap-1 flex-shrink-0">
                  {toggle?.customFilePath && (
                    <Button
                      variant="ghost"
                      size="sm"
                      className="h-6 px-1 text-xs"
                      onClick={async () => {
                        await setToggle.mutateAsync({ ...toggle!, customFilePath: null });
                        queryClient.invalidateQueries({ queryKey: ["systemPromptFile", selectedApp] });
                        toast.success("已恢复默认文件");
                      }}
                    >
                      <RotateCcw className="w-3 h-3 mr-1" />
                      恢复默认
                    </Button>
                  )}
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-6 px-1 text-xs"
                    onClick={async () => {
                      const picked = await systemPromptApi.pickFile();
                      if (picked && toggle) {
                        await setToggle.mutateAsync({ ...toggle, customFilePath: picked });
                        queryClient.invalidateQueries({ queryKey: ["systemPromptFile", selectedApp] });
                        toast.success(`已切换至 ${picked.split(/[/\\]/).pop()}`);
                      }
                    }}
                  >
                    <FolderOpen className="w-3 h-3 mr-1" />
                    选择文件
                  </Button>
                </div>
              </div>

              {/* 编辑器 */}
              <MarkdownEditor
                value={editorContent}
                onChange={(v) => { setEditorContent(v); setHasUnsaved(true); }}
                placeholder="# 在此编辑全局系统提示..."
                minHeight="350px"
              />
            </div>
          )}

          {activeTab === "shared" && (
            <div className="flex flex-col gap-4">
              <p className="text-sm text-muted-foreground">
                统一规则将追加到所有启用了"接受统一规则"的 AI 工具的系统提示词中。
              </p>
              <p className="text-xs text-muted-foreground">
                📄 ~/.cc-switch/system_prompt_shared.md
              </p>
              <MarkdownEditor
                value={sharedEditor}
                onChange={(v) => { setSharedEditor(v); setSharedUnsaved(true); }}
                placeholder="# 在此编辑统一规则..."
                minHeight="350px"
              />
            </div>
          )}
        </Tabs>
      </FullScreenPanel>
    );
  },
);

SystemPromptPanel.displayName = "SystemPromptPanel";
export default SystemPromptPanel;
