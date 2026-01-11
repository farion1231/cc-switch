import React, { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Sparkles, Trash2, ExternalLink } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import {
  useInstalledSkills,
  useToggleSkillApp,
  useUninstallSkill,
  useScanUnmanagedSkills,
  useImportSkillsFromApps,
  type InstalledSkill,
  type AppType,
} from "@/hooks/useSkills";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { settingsApi } from "@/lib/api";
import { toast } from "sonner";

interface UnifiedSkillsPanelProps {
  onOpenDiscovery: () => void;
}

/**
 * 统一 Skills 管理面板
 * v3.10.0 新架构：所有 Skills 统一管理，每个 Skill 通过开关控制应用到哪些客户端
 */
export interface UnifiedSkillsPanelHandle {
  openDiscovery: () => void;
  openImport: () => void;
}

const UnifiedSkillsPanel = React.forwardRef<
  UnifiedSkillsPanelHandle,
  UnifiedSkillsPanelProps
>(({ onOpenDiscovery }, ref) => {
  const { t } = useTranslation();
  const [confirmDialog, setConfirmDialog] = useState<{
    isOpen: boolean;
    title: string;
    message: string;
    onConfirm: () => void;
  } | null>(null);
  const [importDialogOpen, setImportDialogOpen] = useState(false);

  // Queries and Mutations
  const { data: skills, isLoading } = useInstalledSkills();
  const toggleAppMutation = useToggleSkillApp();
  const uninstallMutation = useUninstallSkill();
  const { data: unmanagedSkills, refetch: scanUnmanaged } =
    useScanUnmanagedSkills();
  const importMutation = useImportSkillsFromApps();

  // Count enabled skills per app
  const enabledCounts = useMemo(() => {
    const counts = { claude: 0, codex: 0, gemini: 0 };
    if (!skills) return counts;
    skills.forEach((skill) => {
      if (skill.apps.claude) counts.claude++;
      if (skill.apps.codex) counts.codex++;
      if (skill.apps.gemini) counts.gemini++;
    });
    return counts;
  }, [skills]);

  const handleToggleApp = async (
    id: string,
    app: AppType,
    enabled: boolean,
  ) => {
    try {
      await toggleAppMutation.mutateAsync({ id, app, enabled });
    } catch (error) {
      toast.error(t("common.error"), {
        description: String(error),
      });
    }
  };

  const handleUninstall = (skill: InstalledSkill) => {
    setConfirmDialog({
      isOpen: true,
      title: t("skills.uninstall"),
      message: t("skills.uninstallConfirm", { name: skill.name }),
      onConfirm: async () => {
        try {
          await uninstallMutation.mutateAsync(skill.id);
          setConfirmDialog(null);
          toast.success(t("skills.uninstallSuccess", { name: skill.name }), {
            closeButton: true,
          });
        } catch (error) {
          toast.error(t("common.error"), {
            description: String(error),
          });
        }
      },
    });
  };

  const handleOpenImport = async () => {
    try {
      const result = await scanUnmanaged();
      if (!result.data || result.data.length === 0) {
        toast.success(t("skills.noUnmanagedFound"), { closeButton: true });
        return;
      }
      setImportDialogOpen(true);
    } catch (error) {
      toast.error(t("common.error"), {
        description: String(error),
      });
    }
  };

  const handleImport = async (directories: string[]) => {
    try {
      const imported = await importMutation.mutateAsync(directories);
      setImportDialogOpen(false);
      toast.success(t("skills.importSuccess", { count: imported.length }), {
        closeButton: true,
      });
    } catch (error) {
      toast.error(t("common.error"), {
        description: String(error),
      });
    }
  };

  React.useImperativeHandle(ref, () => ({
    openDiscovery: onOpenDiscovery,
    openImport: handleOpenImport,
  }));

  return (
    <div className="mx-auto max-w-[56rem] px-6 flex flex-col h-[calc(100vh-8rem)] overflow-hidden">
      {/* Info Section */}
      <div className="flex-shrink-0 py-4 glass rounded-xl border border-white/10 mb-4 px-6">
        <div className="text-sm text-muted-foreground">
          {t("skills.installed", { count: skills?.length || 0 })} ·{" "}
          {t("skills.apps.claude")}: {enabledCounts.claude} ·{" "}
          {t("skills.apps.codex")}: {enabledCounts.codex} ·{" "}
          {t("skills.apps.gemini")}: {enabledCounts.gemini}
        </div>
      </div>

      {/* Content - Scrollable */}
      <div className="flex-1 overflow-y-auto overflow-x-hidden pb-24">
        {isLoading ? (
          <div className="text-center py-12 text-muted-foreground">
            {t("skills.loading")}
          </div>
        ) : !skills || skills.length === 0 ? (
          <div className="text-center py-12">
            <div className="w-16 h-16 mx-auto mb-4 bg-muted rounded-full flex items-center justify-center">
              <Sparkles size={24} className="text-muted-foreground" />
            </div>
            <h3 className="text-lg font-medium text-foreground mb-2">
              {t("skills.noInstalled")}
            </h3>
            <p className="text-muted-foreground text-sm">
              {t("skills.noInstalledDescription")}
            </p>
          </div>
        ) : (
          <div className="space-y-3">
            {skills.map((skill) => (
              <InstalledSkillListItem
                key={skill.id}
                skill={skill}
                onToggleApp={handleToggleApp}
                onUninstall={() => handleUninstall(skill)}
              />
            ))}
          </div>
        )}
      </div>

      {/* Confirm Dialog */}
      {confirmDialog && (
        <ConfirmDialog
          isOpen={confirmDialog.isOpen}
          title={confirmDialog.title}
          message={confirmDialog.message}
          onConfirm={confirmDialog.onConfirm}
          onCancel={() => setConfirmDialog(null)}
        />
      )}

      {/* Import Dialog */}
      {importDialogOpen && unmanagedSkills && (
        <ImportSkillsDialog
          skills={unmanagedSkills}
          onImport={handleImport}
          onClose={() => setImportDialogOpen(false)}
        />
      )}
    </div>
  );
});

UnifiedSkillsPanel.displayName = "UnifiedSkillsPanel";

/**
 * 已安装 Skill 列表项组件
 */
interface InstalledSkillListItemProps {
  skill: InstalledSkill;
  onToggleApp: (id: string, app: AppType, enabled: boolean) => void;
  onUninstall: () => void;
}

const InstalledSkillListItem: React.FC<InstalledSkillListItemProps> = ({
  skill,
  onToggleApp,
  onUninstall,
}) => {
  const { t } = useTranslation();

  const openDocs = async () => {
    if (!skill.readmeUrl) return;
    try {
      await settingsApi.openExternal(skill.readmeUrl);
    } catch {
      // ignore
    }
  };

  // 生成来源标签
  const sourceLabel = useMemo(() => {
    if (skill.repoOwner && skill.repoName) {
      return `${skill.repoOwner}/${skill.repoName}`;
    }
    return t("skills.local");
  }, [skill.repoOwner, skill.repoName, t]);

  return (
    <div className="group relative flex items-center gap-4 p-4 rounded-xl border border-border-default bg-muted/50 hover:bg-muted hover:border-border-default/80 hover:shadow-sm transition-all duration-300">
      {/* 左侧：Skill 信息 */}
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2 mb-1">
          <h3 className="font-medium text-foreground">{skill.name}</h3>
          {skill.readmeUrl && (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={openDocs}
              className="h-6 px-2"
            >
              <ExternalLink size={14} />
            </Button>
          )}
        </div>
        {skill.description && (
          <p className="text-sm text-muted-foreground line-clamp-2">
            {skill.description}
          </p>
        )}
        <p className="text-xs text-muted-foreground/70 mt-1">{sourceLabel}</p>
      </div>

      {/* 中间：应用开关 */}
      <div className="flex flex-col gap-2 flex-shrink-0 min-w-[120px]">
        <div className="flex items-center justify-between gap-3">
          <label
            htmlFor={`${skill.id}-claude`}
            className="text-sm text-foreground/80 cursor-pointer"
          >
            {t("skills.apps.claude")}
          </label>
          <Switch
            id={`${skill.id}-claude`}
            checked={skill.apps.claude}
            onCheckedChange={(checked: boolean) =>
              onToggleApp(skill.id, "claude", checked)
            }
          />
        </div>

        <div className="flex items-center justify-between gap-3">
          <label
            htmlFor={`${skill.id}-codex`}
            className="text-sm text-foreground/80 cursor-pointer"
          >
            {t("skills.apps.codex")}
          </label>
          <Switch
            id={`${skill.id}-codex`}
            checked={skill.apps.codex}
            onCheckedChange={(checked: boolean) =>
              onToggleApp(skill.id, "codex", checked)
            }
          />
        </div>

        <div className="flex items-center justify-between gap-3">
          <label
            htmlFor={`${skill.id}-gemini`}
            className="text-sm text-foreground/80 cursor-pointer"
          >
            {t("skills.apps.gemini")}
          </label>
          <Switch
            id={`${skill.id}-gemini`}
            checked={skill.apps.gemini}
            onCheckedChange={(checked: boolean) =>
              onToggleApp(skill.id, "gemini", checked)
            }
          />
        </div>
      </div>

      {/* 右侧：删除按钮 */}
      <div className="flex items-center gap-2 flex-shrink-0">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          onClick={onUninstall}
          className="hover:text-red-500 hover:bg-red-100 dark:hover:text-red-400 dark:hover:bg-red-500/10"
          title={t("skills.uninstall")}
        >
          <Trash2 size={16} />
        </Button>
      </div>
    </div>
  );
};

/**
 * 导入 Skills 对话框
 */
interface ImportSkillsDialogProps {
  skills: Array<{
    directory: string;
    name: string;
    description?: string;
    foundIn: string[];
  }>;
  onImport: (directories: string[]) => void;
  onClose: () => void;
}

const ImportSkillsDialog: React.FC<ImportSkillsDialogProps> = ({
  skills,
  onImport,
  onClose,
}) => {
  const { t } = useTranslation();
  const [selected, setSelected] = useState<Set<string>>(
    new Set(skills.map((s) => s.directory)),
  );

  const toggleSelect = (directory: string) => {
    const newSelected = new Set(selected);
    if (newSelected.has(directory)) {
      newSelected.delete(directory);
    } else {
      newSelected.add(directory);
    }
    setSelected(newSelected);
  };

  const handleImport = () => {
    onImport(Array.from(selected));
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-background rounded-xl p-6 max-w-lg w-full mx-4 shadow-xl max-h-[80vh] flex flex-col">
        <h2 className="text-lg font-semibold mb-2">{t("skills.import")}</h2>
        <p className="text-sm text-muted-foreground mb-4">
          {t("skills.importDescription")}
        </p>

        <div className="flex-1 overflow-y-auto space-y-2 mb-4">
          {skills.map((skill) => (
            <label
              key={skill.directory}
              className="flex items-start gap-3 p-3 rounded-lg border hover:bg-muted cursor-pointer"
            >
              <input
                type="checkbox"
                checked={selected.has(skill.directory)}
                onChange={() => toggleSelect(skill.directory)}
                className="mt-1"
              />
              <div className="flex-1 min-w-0">
                <div className="font-medium">{skill.name}</div>
                {skill.description && (
                  <div className="text-sm text-muted-foreground line-clamp-1">
                    {skill.description}
                  </div>
                )}
                <div className="text-xs text-muted-foreground/70 mt-1">
                  {t("skills.foundIn")}: {skill.foundIn.join(", ")}
                </div>
              </div>
            </label>
          ))}
        </div>

        <div className="flex justify-end gap-3">
          <Button variant="outline" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button onClick={handleImport} disabled={selected.size === 0}>
            {t("skills.importSelected", { count: selected.size })}
          </Button>
        </div>
      </div>
    </div>
  );
};

export default UnifiedSkillsPanel;
