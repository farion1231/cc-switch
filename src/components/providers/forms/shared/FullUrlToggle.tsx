import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { Switch } from "@/components/ui/switch";
import { HelpTip } from "@/components/ui/help-tip";
import { cn } from "@/lib/utils";

interface FullUrlToggleProps {
  checked: boolean;
  onCheckedChange: (value: boolean) => void;
  /** 上游格式为 Gemini Native 时，路由对完整 URL 另有一套规范化规则。 */
  geminiNative?: boolean;
  className?: string;
}

/**
 * 「完整 URL」开关（Claude Code、Codex、Grok Build 的高级选项共用）。
 * 只对端点路径不标准的网关有用，只在路由、聚合下生效；说明全在「?」里。
 */
export function FullUrlToggle({
  checked,
  onCheckedChange,
  geminiNative = false,
  className,
}: FullUrlToggleProps) {
  const { t } = useTranslation();
  const label = t("providerForm.fullUrlLabel", { defaultValue: "完整 URL" });

  return (
    <div className={cn("flex items-center justify-between gap-4", className)}>
      <div className="flex items-center gap-0.5">
        <FormLabel>{label}</FormLabel>
        <HelpTip title={label}>
          {geminiNative
            ? t("providerForm.fullUrlHelpGeminiNative", {
                defaultValue:
                  "Gemini Native 下：标准 Gemini 地址按模型自动规范化，自定义中转的完整地址原样使用、只补查询参数。直连下用不了。",
              })
            : t("providerForm.fullUrlHelp", {
                defaultValue:
                  "端点路径不标准的网关才需要：地址填完整请求地址，路由原样使用、不再拼接路径。直连下用不了。",
              })}
        </HelpTip>
      </div>
      <Switch
        checked={checked}
        onCheckedChange={onCheckedChange}
        aria-label={label}
      />
    </div>
  );
}
