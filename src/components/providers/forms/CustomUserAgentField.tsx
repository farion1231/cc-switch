import { useTranslation } from "react-i18next";
import { ChevronDown } from "lucide-react";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { isValidUserAgentHeader } from "@/lib/userAgent";
import { USER_AGENT_PRESETS } from "@/config/userAgentPresets";

interface CustomUserAgentFieldProps {
  /** 输入框的 id（用于 label htmlFor）；两个表单需传入各自唯一值。 */
  id: string;
  value: string;
  onChange: (value: string) => void;
  /** 字段名后「?」里的说明；「只在路由、聚合模式下生效」由调用方在分组开头说一次。 */
  help: string;
}

/**
 * 供应商级自定义 User-Agent 字段（Claude / Codex 表单共用）。
 *
 * 含标签 + 输入框 + 右侧预设下拉菜单 + 实时合法性提示。校验口径与后端
 * `parse_custom_user_agent` 一致（见 `@/lib/userAgent`），非法时给非阻断红字提示
 * （运行时仍会静默忽略）。
 */
export function CustomUserAgentField({
  id,
  value,
  onChange,
  help,
}: CustomUserAgentFieldProps) {
  const { t } = useTranslation();
  const valid = isValidUserAgentHeader(value);
  const label = t("providerForm.customUserAgent", {
    defaultValue: "自定义 User-Agent",
  });

  return (
    <div className="space-y-2">
      <div className="flex items-center gap-0.5">
        <FormLabel htmlFor={id}>{label}</FormLabel>
        <HelpTip title={label}>{help}</HelpTip>
      </div>
      <div className="flex items-center gap-2">
        <Input
          id={id}
          type="text"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder="Mozilla/5.0 ..."
          autoComplete="off"
          className="flex-1"
        />
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button type="button" variant="outline" className="shrink-0 gap-1">
              {t("providerForm.customUserAgentPresets", {
                defaultValue: "预设",
              })}
              <ChevronDown className="h-3.5 w-3.5 opacity-60" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent
            align="end"
            className="max-h-64 overflow-y-auto z-[200]"
          >
            {USER_AGENT_PRESETS.map((preset) => (
              <DropdownMenuItem
                key={preset}
                onSelect={() => onChange(preset)}
                className="font-mono text-xs"
              >
                {preset}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      {!valid && (
        <p className="text-xs text-destructive">
          {t("providerForm.customUserAgentInvalid", {
            defaultValue:
              "User-Agent 不能包含控制字符（如换行符），否则将被忽略。",
          })}
        </p>
      )}
    </div>
  );
}
