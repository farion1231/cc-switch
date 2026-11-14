import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Save } from "lucide-react";
import { toast } from "@/lib/toast";
import { useOpenClawEnv, useSaveOpenClawEnv } from "@/hooks/useOpenClaw";
import { extractErrorMessage } from "@/utils/errorUtils";
import { Button } from "@/components/ui/button";
import JsonEditor from "@/components/JsonEditor";
import { parseOpenClawEnvEditorValue } from "./utils";

const EnvPanel: React.FC = () => {
  const { t } = useTranslation();
  const { data: envData, isLoading } = useOpenClawEnv();
  const saveEnvMutation = useSaveOpenClawEnv();
  const [editorValue, setEditorValue] = useState("{}");
  const [isDarkMode, setIsDarkMode] = useState(false);
  // 用户改过、还没保存时，任何一次 refetch（窗口重新聚焦、别的面板触发
  // invalidateQueries）都不能用服务端旧值盖掉编辑器里的内容。
  const dirtyRef = useRef(false);

  useEffect(() => {
    if (dirtyRef.current) return;
    const nextValue =
      envData && Object.keys(envData).length > 0
        ? JSON.stringify(envData, null, 2)
        : "{}";
    setEditorValue(nextValue);
  }, [envData]);

  useEffect(() => {
    setIsDarkMode(document.documentElement.classList.contains("dark"));

    const observer = new MutationObserver(() => {
      setIsDarkMode(document.documentElement.classList.contains("dark"));
    });

    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });

    return () => observer.disconnect();
  }, []);

  const handleEditorChange = (value: string) => {
    dirtyRef.current = true;
    setEditorValue(value);
  };

  const handleSave = async () => {
    try {
      const env = parseOpenClawEnvEditorValue(editorValue);
      await saveEnvMutation.mutateAsync(env);
      dirtyRef.current = false;
      toast.success(t("openclaw.env.saveSuccess"));
    } catch (error) {
      const detail = extractErrorMessage(error);
      let description = detail || undefined;
      if (detail === "OPENCLAW_ENV_EMPTY") {
        description = t("openclaw.env.empty", {
          defaultValue:
            "OpenClaw env cannot be empty. Use {} for an empty object.",
        });
      } else if (detail === "OPENCLAW_ENV_INVALID_JSON") {
        description = t("openclaw.env.invalidJson", {
          defaultValue: "OpenClaw env must be valid JSON.",
        });
      } else if (detail === "OPENCLAW_ENV_OBJECT_REQUIRED") {
        description = t("openclaw.env.objectRequired", {
          defaultValue: "OpenClaw env must be a JSON object.",
        });
      }
      toast.error(t("openclaw.env.saveFailed"), {
        description,
      });
    }
  };

  if (isLoading) {
    return (
      <div className="px-6 pt-4 pb-8 flex items-center justify-center min-h-[200px]">
        <div className="text-sm text-fg-2">{t("common.loading")}</div>
      </div>
    );
  }

  return (
    <div className="px-6 pt-4 pb-8">
      <p className="text-xs text-fg-2 mb-4">
        {t("openclaw.env.editorHint", {
          defaultValue:
            "Edit the full env section as JSON. Nested objects such as env.vars and env.shellEnv are supported.",
        })}
      </p>

      <JsonEditor
        value={editorValue}
        onChange={handleEditorChange}
        darkMode={isDarkMode}
        rows={18}
        showValidation={true}
        language="json"
      />

      <div className="flex justify-end mt-4">
        <Button
          size="sm"
          onClick={handleSave}
          disabled={saveEnvMutation.isPending}
        >
          <Save className="w-4 h-4 mr-1" />
          {saveEnvMutation.isPending ? t("common.saving") : t("common.save")}
        </Button>
      </div>
    </div>
  );
};

export default EnvPanel;
