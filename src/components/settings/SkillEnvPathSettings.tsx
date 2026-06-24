import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { FolderSearch, RotateCcw } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { settingsApi } from "@/lib/api";

export interface SkillEnvPathSettingsProps {
  value?: string;
  onChange: (path?: string) => Promise<boolean>;
}

export function SkillEnvPathSettings({
  value,
  onChange,
}: SkillEnvPathSettingsProps) {
  const { t } = useTranslation();
  const [defaultPath, setDefaultPath] = useState("");
  const [draft, setDraft] = useState(value ?? "");
  const [isSaving, setIsSaving] = useState(false);

  useEffect(() => {
    setDraft(value ?? "");
  }, [value]);

  useEffect(() => {
    let active = true;
    settingsApi
      .getDefaultSkillEnvOutputPath()
      .then((path) => {
        if (active) setDefaultPath(path);
      })
      .catch((error) => {
        console.warn(
          "[SkillEnvPathSettings] Failed to load default path",
          error,
        );
      });
    return () => {
      active = false;
    };
  }, []);

  const currentPath = draft || defaultPath;

  const savePath = async (nextPath?: string) => {
    const normalized = nextPath?.trim() ? nextPath.trim() : undefined;
    const previous = value?.trim() ? value.trim() : undefined;
    if (normalized === previous) return;
    setIsSaving(true);
    try {
      const ok = await onChange(normalized);
      if (ok) {
        toast.success(
          t("settings.skillEnv.pathSaved", {
            defaultValue: "Skill 环境变量存储位置已保存",
          }),
        );
      }
    } finally {
      setIsSaving(false);
    }
  };

  const handleBrowse = async () => {
    const selected = await settingsApi.pickSkillEnvOutputFile(currentPath);
    if (!selected) return;
    setDraft(selected);
    await savePath(selected);
  };

  return (
    <section className="space-y-2">
      <header className="space-y-1">
        <h3 className="text-sm font-medium">
          {t("settings.skillEnv.pathTitle", {
            defaultValue: "Skill 环境变量存储位置",
          })}
        </h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.skillEnv.pathDescription", {
            defaultValue:
              "配置 CC Switch 生成的系统环境变量文件路径。变量内容通过主页面环境变量按钮编辑。",
          })}
        </p>
      </header>
      <div className="flex max-w-2xl items-center gap-2">
        <Input
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onBlur={() => void savePath(draft)}
          placeholder={defaultPath}
          className="font-mono text-xs"
        />
        <Button
          type="button"
          variant="outline"
          size="icon"
          disabled={isSaving}
          onClick={() => void handleBrowse()}
          title={t("common.browse", { defaultValue: "浏览" })}
        >
          <FolderSearch className="h-4 w-4" />
        </Button>
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={isSaving || !draft}
          onClick={() => {
            setDraft("");
            void savePath(undefined);
          }}
          title={t("settings.skillEnv.resetDefault", {
            defaultValue: "恢复默认路径",
          })}
        >
          <RotateCcw className="mr-2 h-4 w-4" />
          {t("settings.skillEnv.resetDefault", {
            defaultValue: "恢复默认路径",
          })}
        </Button>
      </div>
      <p className="text-xs text-muted-foreground">
        {t("settings.skillEnv.effectivePath", {
          defaultValue: "当前生效路径：{{path}}",
          path: currentPath,
        })}
      </p>
    </section>
  );
}
