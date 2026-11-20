import { useTranslation } from "react-i18next";
import JsonEditor from "@/components/JsonEditor";
import { Label } from "@/components/ui/label";
import { useDarkMode } from "@/hooks/useDarkMode";
import { useNativeConfigPath } from "@/hooks/useNativeConfigPath";
import type { AppId } from "@/lib/api/types";

interface NativeConfigEditorProps {
  appId: AppId;
  /** 配置目录下的文件名，如 `config.toml`。 */
  file: string;
  id: string;
  value: string;
  onChange: (value: string) => void;
  /** 已翻译的错误；有值时显示在编辑框下面。 */
  error?: string | null;
}

/**
 * 累加式应用的供应商编辑框：按应用自己的文件格式（TOML / YAML）显示这一家
 * 在文件里的那一段，标题带上真实路径。
 */
export function NativeConfigEditor({
  appId,
  file,
  id,
  value,
  onChange,
  error,
}: NativeConfigEditorProps) {
  const { t } = useTranslation();
  const darkMode = useDarkMode();
  const path = useNativeConfigPath(appId, file);
  const title = t("providerForm.nativeConfig.title", { path });
  return (
    <div className="space-y-2">
      <Label htmlFor={id} className="break-all">
        {title}
      </Label>
      <p className="text-xs text-fg-2">{t("providerForm.nativeConfig.hint")}</p>
      <JsonEditor
        id={id}
        ariaLabel={title}
        value={value}
        darkMode={darkMode}
        language="javascript"
        showValidation={false}
        height={Math.max(1, value.split("\n").length) * 20 + 20}
        onChange={onChange}
      />
      {error && (
        <p role="alert" className="text-xs text-danger-text">
          {error}
        </p>
      )}
    </div>
  );
}
