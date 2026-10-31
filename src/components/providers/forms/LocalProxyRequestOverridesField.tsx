import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { Textarea } from "@/components/ui/textarea";
import { HelpTip } from "@/components/ui/help-tip";
import {
  parseBodyOverrideJson,
  parseHeaderOverrideJson,
} from "@/lib/requestOverrides";

interface LocalProxyRequestOverridesFieldProps {
  headersJson: string;
  onHeadersJsonChange: (value: string) => void;
  /** 不传 bodyJson/onBodyJsonChange 时只渲染 Header 覆盖（如 Claude Desktop 表单）。 */
  bodyJson?: string;
  onBodyJsonChange?: (value: string) => void;
  /** 字段名后「?」里的说明；「只在路由、聚合模式下生效」由调用方在分组开头说一次。 */
  help: string;
}

export function LocalProxyRequestOverridesField({
  headersJson,
  onHeadersJsonChange,
  bodyJson,
  onBodyJsonChange,
  help,
}: LocalProxyRequestOverridesFieldProps) {
  const { t } = useTranslation();
  const label = t("providerForm.localProxyRequestOverrides", {
    defaultValue: "本地代理请求覆盖",
  });
  const headerError = parseHeaderOverrideJson(headersJson).error;
  const showBody = bodyJson !== undefined && onBodyJsonChange !== undefined;
  const bodyError = showBody
    ? parseBodyOverrideJson(bodyJson).error
    : undefined;

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-0.5">
        <FormLabel>{label}</FormLabel>
        <HelpTip title={label}>{help}</HelpTip>
      </div>

      <div className={showBody ? "grid gap-3 md:grid-cols-2" : undefined}>
        <div className="space-y-2">
          <FormLabel className="text-xs text-fg-2">
            {t("providerForm.localProxyHeaderOverrides", {
              defaultValue: "Header 覆盖",
            })}
          </FormLabel>
          <Textarea
            value={headersJson}
            onChange={(event) => onHeadersJsonChange(event.target.value)}
            placeholder={'{\n  "X-Provider": "cc-switch"\n}'}
            className="min-h-[132px] resize-y font-mono text-xs"
            aria-invalid={Boolean(headerError)}
          />
          {headerError && (
            <p className="text-xs text-destructive">
              {t("providerForm.localProxyHeaderOverridesInvalidDetail", {
                error: headerError,
                defaultValue: "Header 覆盖格式错误：{{error}}",
              })}
            </p>
          )}
        </div>

        {showBody && (
          <div className="space-y-2">
            <FormLabel className="text-xs text-fg-2">
              {t("providerForm.localProxyBodyOverrides", {
                defaultValue: "Body 覆盖",
              })}
            </FormLabel>
            <Textarea
              value={bodyJson}
              onChange={(event) => onBodyJsonChange(event.target.value)}
              placeholder={'{\n  "temperature": 0.2\n}'}
              className="min-h-[132px] resize-y font-mono text-xs"
              aria-invalid={Boolean(bodyError)}
            />
            {bodyError && (
              <p className="text-xs text-destructive">
                {t("providerForm.localProxyBodyOverridesInvalidDetail", {
                  error: bodyError,
                  defaultValue: "Body 覆盖格式错误：{{error}}",
                })}
              </p>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
