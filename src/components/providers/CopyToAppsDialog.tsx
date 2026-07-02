import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Copy } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";

interface CopyToAppsDialogProps {
  isOpen: boolean;
  onClose: () => void;
  provider: Provider;
  sourceApp: AppId;
  onCopy: (targetApps: AppId[]) => Promise<void>;
}

const APP_OPTIONS: { id: AppId; label: string; labelKey: string }[] = [
  { id: "claude", label: "Claude Code", labelKey: "apps.claude" },
  {
    id: "claude-desktop",
    label: "Claude Desktop",
    labelKey: "apps.claudeDesktop",
  },
  { id: "codex", label: "Codex", labelKey: "apps.codex" },
  { id: "gemini", label: "Gemini", labelKey: "apps.gemini" },
  { id: "opencode", label: "OpenCode", labelKey: "apps.opencode" },
  { id: "openclaw", label: "OpenClaw", labelKey: "apps.openclaw" },
  { id: "hermes", label: "Hermes", labelKey: "apps.hermes" },
];

export function CopyToAppsDialog({
  isOpen,
  onClose,
  provider,
  sourceApp,
  onCopy,
}: CopyToAppsDialogProps) {
  const { t } = useTranslation();
  const [selectedApps, setSelectedApps] = useState<Set<AppId>>(new Set());
  const [copying, setCopying] = useState(false);

  const availableApps = APP_OPTIONS.filter((app) => app.id !== sourceApp);

  const handleToggleApp = (appId: AppId) => {
    const newSelected = new Set(selectedApps);
    if (newSelected.has(appId)) {
      newSelected.delete(appId);
    } else {
      newSelected.add(appId);
    }
    setSelectedApps(newSelected);
  };

  const handleSelectAll = () => {
    if (selectedApps.size === availableApps.length) {
      setSelectedApps(new Set());
    } else {
      setSelectedApps(new Set(availableApps.map((app) => app.id)));
    }
  };

  const handleCopy = async () => {
    if (selectedApps.size === 0) return;

    setCopying(true);
    try {
      await onCopy(Array.from(selectedApps));
      onClose();
      setSelectedApps(new Set()); // 重置选择
    } catch (error) {
      console.error("Failed to copy provider:", error);
    } finally {
      setCopying(false);
    }
  };

  const handleClose = () => {
    if (!copying) {
      onClose();
      setSelectedApps(new Set());
    }
  };

  return (
    <Dialog open={isOpen} onOpenChange={handleClose}>
      <DialogContent className="sm:max-w-[480px]" zIndex="top">
        <DialogHeader>
          <DialogTitle>
            {t("provider.copyToApps.title", {
              defaultValue: "复制到其他应用",
            })}
          </DialogTitle>
          <DialogDescription>
            {t("provider.copyToApps.description", {
              defaultValue: `将 "${provider.name}" 的配置复制到选定的应用中`,
              name: provider.name,
            })}
          </DialogDescription>
        </DialogHeader>

        {/* 全选选项 */}
        <div className="flex items-center gap-2 p-2 border-b">
          <Checkbox
            id="select-all"
            checked={selectedApps.size === availableApps.length}
            onCheckedChange={handleSelectAll}
          />
          <Label
            htmlFor="select-all"
            className="text-sm font-medium cursor-pointer"
          >
            {t("provider.copyToApps.selectAll", {
              defaultValue: "全选",
            })}
          </Label>
        </div>

        {/* 应用列表 */}
        <div className="grid grid-cols-2 gap-2 py-3">
          {availableApps.map((app) => (
            <Label
              key={app.id}
              htmlFor={`app-${app.id}`}
              className="flex items-center gap-2 p-2 rounded-md hover:bg-accent/50 cursor-pointer"
            >
              <Checkbox
                id={`app-${app.id}`}
                checked={selectedApps.has(app.id)}
                onCheckedChange={() => handleToggleApp(app.id)}
              />
              <span className="text-sm flex-1">
                {t(app.labelKey, { defaultValue: app.label })}
              </span>
            </Label>
          ))}
        </div>

        {selectedApps.size > 0 && (
          <p className="text-xs text-muted-foreground pb-2">
            {t("provider.copyToApps.selectedCount", {
              defaultValue: `已选择 ${selectedApps.size} 个应用`,
              count: selectedApps.size,
            })}
          </p>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={handleClose} disabled={copying}>
            {t("common.cancel", { defaultValue: "取消" })}
          </Button>
          <Button
            onClick={handleCopy}
            disabled={selectedApps.size === 0 || copying}
          >
            {copying ? (
              <>
                <div className="h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent" />
                {t("provider.copyToApps.copying", {
                  defaultValue: "复制中...",
                })}
              </>
            ) : (
              <>
                <Copy className="h-4 w-4" />
                {t("provider.copyToApps.copy", {
                  defaultValue: `复制到 ${selectedApps.size} 个应用`,
                  count: selectedApps.size,
                })}
              </>
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
