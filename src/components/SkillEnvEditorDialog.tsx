import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, RefreshCw, Save, X } from "lucide-react";
import { toast } from "sonner";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { settingsApi } from "@/lib/api";
import type { SkillEnvState } from "@/lib/api/settings";
import { extractErrorMessage } from "@/utils/errorUtils";

export interface SkillEnvEditorDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function SkillEnvEditorDialog({
  open,
  onOpenChange,
}: SkillEnvEditorDialogProps) {
  const { t } = useTranslation();
  const [state, setState] = useState<SkillEnvState | null>(null);
  const [content, setContent] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [isRefreshing, setIsRefreshing] = useState(false);

  const load = async () => {
    setIsLoading(true);
    try {
      const next = await settingsApi.getSkillEnvState();
      setState(next);
      setContent(next.content);
    } catch (error) {
      toast.error(
        t("skillEnv.loadFailed", {
          defaultValue: "读取环境变量失败",
        }),
        { description: extractErrorMessage(error) || undefined },
      );
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    if (open) {
      void load();
    }
  }, [open]);

  const handleSave = async () => {
    setIsSaving(true);
    try {
      const result = await settingsApi.saveSkillEnv(content);
      toast.success(
        t("skillEnv.saveSuccess", {
          defaultValue: "环境变量已保存并应用",
        }),
      );
      setState((prev: SkillEnvState | null) =>
        prev
          ? {
              ...prev,
              sourcePath: result.sourcePath,
              outputPath: result.outputPath,
              parsedCount: result.parsedCount,
              content,
            }
          : {
              sourcePath: result.sourcePath,
              outputPath: result.outputPath,
              parsedCount: result.parsedCount,
              content,
            },
      );
    } catch (error) {
      toast.error(
        t("skillEnv.saveFailed", {
          defaultValue: "保存环境变量失败",
        }),
        { description: extractErrorMessage(error) || undefined },
      );
    } finally {
      setIsSaving(false);
    }
  };

  const handleRefresh = async () => {
    setIsRefreshing(true);
    try {
      const result = await settingsApi.refreshSkillEnv();
      const next = await settingsApi.getSkillEnvState();
      setState(next);
      setContent(next.content);
      toast.success(
        t("skillEnv.refreshSuccess", {
          defaultValue: "环境变量已重新加载并应用",
        }),
        {
          description: t("skillEnv.refreshCount", {
            defaultValue: "已重新加载 {{count}} 个变量",
            count: result.parsedCount,
          }),
        },
      );
    } catch (error) {
      toast.error(
        t("skillEnv.refreshFailed", {
          defaultValue: "重新加载环境变量失败",
        }),
        { description: extractErrorMessage(error) || undefined },
      );
    } finally {
      setIsRefreshing(false);
    }
  };

  const busy = isLoading || isSaving || isRefreshing;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-3xl" zIndex="top">
        <DialogHeader>
          <DialogTitle>
            {t("skillEnv.title", { defaultValue: "Skill 环境变量" })}
          </DialogTitle>
          <DialogDescription>
            {t("skillEnv.description", {
              defaultValue:
                "使用 .env 格式配置全局 Skill 环境变量。保存后会写入源文件并生成当前系统的环境变量文件。",
            })}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-3">
          <Textarea
            value={content}
            onChange={(event) => setContent(event.target.value)}
            disabled={isLoading}
            className="min-h-[360px] resize-none font-mono text-xs leading-5"
            placeholder={
              "# zhangtian-platform\nZHANGTIAN_USER_ID=your-user-id\nZHANGTIAN_VPN=true"
            }
          />
          <div className="space-y-1 rounded-md border border-border-default bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
            <div>
              {t("skillEnv.sourcePath", {
                defaultValue: "源文件：{{path}}",
                path: state?.sourcePath || "-",
              })}
            </div>
            <div>
              {t("skillEnv.outputPath", {
                defaultValue: "生成文件：{{path}}",
                path: state?.outputPath || "-",
              })}
            </div>
            <div>
              {t("skillEnv.parsedCount", {
                defaultValue: "变量数量：{{count}}",
                count: state?.parsedCount ?? 0,
              })}
            </div>
          </div>
          <p className="text-xs text-muted-foreground">
            {t("skillEnv.restartHint", {
              defaultValue:
                "保存/刷新后，CC Switch 后续启动的进程会使用新变量；已运行的客户端需要重启。",
            })}
          </p>
        </div>

        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            disabled={busy}
            onClick={() => onOpenChange(false)}
          >
            <X className="mr-2 h-4 w-4" />
            {t("common.close", { defaultValue: "关闭" })}
          </Button>
          <Button
            type="button"
            variant="outline"
            disabled={busy}
            onClick={() => void handleRefresh()}
          >
            {isRefreshing ? (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            ) : (
              <RefreshCw className="mr-2 h-4 w-4" />
            )}
            {t("skillEnv.reloadApply", { defaultValue: "重新加载并应用" })}
          </Button>
          <Button
            type="button"
            disabled={busy}
            onClick={() => void handleSave()}
          >
            {isSaving ? (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            ) : (
              <Save className="mr-2 h-4 w-4" />
            )}
            {t("common.save", { defaultValue: "保存" })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
