import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { HelpTip } from "@/components/ui/help-tip";

export function isValidBillingMultiplier(raw: string): boolean {
  const value = raw.trim();
  return (
    value === "" ||
    (/^(?:\d+(?:\.\d*)?|\.\d+)$/.test(value) &&
      Number(value) >= 0 &&
      Number(value) <= 1000)
  );
}

interface BillingMultiplierFieldProps {
  /** 输入框的 id（用于 label htmlFor）；各表单传入各自唯一值。 */
  id: string;
  value: string;
  onChange: (value: string) => void;
}

/**
 * 供应商费用倍率（Claude / Codex / Grok Build / Gemini 表单共用）。
 *
 * 只作用于经本地路由的请求，和 User-Agent、请求覆盖放在同一组；
 * 「只在路由下生效」写在「?」里，Gemini 表单没有分组提示也能看到。
 */
export function BillingMultiplierField({
  id,
  value,
  onChange,
}: BillingMultiplierFieldProps) {
  const { t } = useTranslation();
  const invalid = !isValidBillingMultiplier(value);
  const label = t("providerForm.billingMultiplier", {
    defaultValue: "供应商费用倍率",
  });

  return (
    <div className="space-y-2">
      <div className="flex items-center gap-0.5">
        <FormLabel htmlFor={id}>{label}</FormLabel>
        <HelpTip title={label}>
          {t("providerForm.billingMultiplierHelp", {
            defaultValue:
              "该供应商所有模型的参考价都乘以这个值，用来估算经本地路由的请求花了多少钱。0.5 即半价，留空为 1 倍。只影响之后的请求；不开路由（直连）时从会话记录导入的用量仍按 1 倍统计。",
          })}
        </HelpTip>
      </div>
      <Input
        id={id}
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder="1"
        inputMode="decimal"
        autoComplete="off"
        aria-invalid={invalid}
      />
      {invalid && (
        <p role="alert" className="text-xs text-destructive">
          {t("providerForm.billingMultiplierInvalid", {
            defaultValue: "请输入 0 到 1000 的数字，或留空使用 1 倍。",
          })}
        </p>
      )}
    </div>
  );
}
