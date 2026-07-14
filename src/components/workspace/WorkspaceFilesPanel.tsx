import React, { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import {
  FileCode,
  Heart,
  User,
  IdCard,
  Wrench,
  Brain,
  Activity,
  Rocket,
  Power,
  CheckCircle2,
  Circle,
  Calendar,
  ChevronRight,
  FolderOpen,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { workspaceApi } from "@/lib/api/workspace";
import WorkspaceFileEditor from "./WorkspaceFileEditor";
import DailyMemoryPanel from "./DailyMemoryPanel";

interface WorkspaceFile {
  filename: string;
  icon: LucideIcon;
  descKey: string;
}

const WORKSPACE_FILES: WorkspaceFile[] = [
  { filename: "AGENTS.md", icon: FileCode, descKey: "workspace.files.agents" },
  { filename: "SOUL.md", icon: Heart, descKey: "workspace.files.soul" },
  { filename: "USER.md", icon: User, descKey: "workspace.files.user" },
  {
    filename: "IDENTITY.md",
    icon: IdCard,
    descKey: "workspace.files.identity",
  },
  { filename: "TOOLS.md", icon: Wrench, descKey: "workspace.files.tools" },
  { filename: "MEMORY.md", icon: Brain, descKey: "workspace.files.memory" },
  {
    filename: "HEARTBEAT.md",
    icon: Activity,
    descKey: "workspace.files.heartbeat",
  },
  {
    filename: "BOOTSTRAP.md",
    icon: Rocket,
    descKey: "workspace.files.bootstrap",
  },
  { filename: "BOOT.md", icon: Power, descKey: "workspace.files.boot" },
];

const WorkspaceFilesPanel: React.FC = () => {
  const { t } = useTranslation();
  const [editingFile, setEditingFile] = useState<string | null>(null);
  const [fileExists, setFileExists] = useState<Record<string, boolean>>({});
  const [showDailyMemory, setShowDailyMemory] = useState(false);

  const checkFileExistence = async () => {
    const results: Record<string, boolean> = {};
    await Promise.all(
      WORKSPACE_FILES.map(async (f) => {
        try {
          const content = await workspaceApi.readFile(f.filename);
          results[f.filename] = content !== null;
        } catch {
          results[f.filename] = false;
        }
      }),
    );
    setFileExists(results);
  };

  useEffect(() => {
    void checkFileExistence();
  }, []);

  const handleEditorClose = () => {
    setEditingFile(null);
    // Re-check file existence after closing editor (file may have been created)
    void checkFileExistence();
  };

  return (
    <div className="px-6 pt-4 pb-8">
      <p
        className="text-sm text-muted-foreground mb-6 cursor-pointer hover:text-foreground transition-colors inline-flex items-center gap-1"
        onClick={() => workspaceApi.openDirectory("workspace")}
        title={t("workspace.openDirectory")}
      >
        ~/.openclaw/workspace/
        <FolderOpen className="w-3.5 h-3.5" />
      </p>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
        {WORKSPACE_FILES.map((file) => {
          const Icon = file.icon;
          const exists = fileExists[file.filename];

          return (
            <button
              key={file.filename}
              onClick={() => setEditingFile(file.filename)}
              className="flex items-start gap-3 p-4 rounded-xl border border-border bg-card hover:bg-accent/50 transition-colors text-left group"
            >
              <div className="mt-0.5 text-muted-foreground group-hover:text-foreground transition-colors">
                <Icon className="w-5 h-5" />
              </div>
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2">
                  <span className="font-medium text-sm text-foreground">
                    {file.filename}
                  </span>
                  {exists ? (
                    <CheckCircle2 className="w-3.5 h-3.5 text-emerald-500 flex-shrink-0" />
                  ) : (
                    <Circle className="w-3.5 h-3.5 text-muted-foreground/40 flex-shrink-0" />
                  )}
                </div>
                <p className="text-xs text-muted-foreground mt-0.5">
                  {t(file.descKey)}
                </p>
              </div>
            </button>
          );
        })}

        {/* Daily Memory — inline with workspace files */}
        <button
          onClick={() => setShowDailyMemory(true)}
          className="flex items-start gap-3 p-4 rounded-xl border border-border bg-card hover:bg-accent/50 transition-colors text-left group"
        >
          <div className="mt-0.5 text-muted-foreground group-hover:text-foreground transition-colors">
            <Calendar className="w-5 h-5" />
          </div>
          <div className="flex-1 min-w-0">
            <span className="font-medium text-sm text-foreground">
              {t("workspace.dailyMemory.cardTitle")}
            </span>
            <p className="text-xs text-muted-foreground mt-0.5">
              {t("workspace.dailyMemory.cardDescription")}
            </p>
          </div>
          <div className="mt-0.5 text-muted-foreground group-hover:text-foreground transition-colors">
            <ChevronRight className="w-4 h-4" />
          </div>
        </button>
      </div>

      <WorkspaceFileEditor
        filename={editingFile ?? ""}
        isOpen={!!editingFile}
        onClose={handleEditorClose}
      />

      <DailyMemoryPanel
        isOpen={showDailyMemory}
        onClose={() => setShowDailyMemory(false)}
      />
    </div>
  );
};

export default WorkspaceFilesPanel;
