import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { HelpTip } from "@/components/ui/help-tip";
import { Zap } from "lucide-react";
import { REQUIRED_LABEL } from "../BasicFormFields";

interface EndpointFieldProps {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  hint?: string;
  required?: boolean;
  /** 填写说明放进字段名后的「?」，不占页面；和 `hint` 二选一。 */
  help?: string;
  showManageButton?: boolean;
  onManageClick?: () => void;
  manageButtonLabel?: string;
}

export function EndpointField({
  id,
  label,
  value,
  onChange,
  placeholder,
  hint,
  required = true,
  help,
  showManageButton = true,
  onManageClick,
  manageButtonLabel,
}: EndpointFieldProps) {
  const { t } = useTranslation();

  const defaultManageLabel = t("providerForm.manageAndTest", {
    defaultValue: "管理和测速",
  });

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-0.5">
          <FormLabel
            htmlFor={id}
            className={required ? REQUIRED_LABEL : undefined}
          >
            {label}
          </FormLabel>
          {help ? (
            <HelpTip title={label}>{help.replace(/^💡\s*/u, "")}</HelpTip>
          ) : null}
        </div>
        {showManageButton && onManageClick ? (
          <button
            type="button"
            onClick={onManageClick}
            className="flex items-center gap-1 text-caption text-fg-2 transition-colors hover:text-fg-1"
          >
            <Zap className="h-3.5 w-3.5" />
            {manageButtonLabel || defaultManageLabel}
          </button>
        ) : null}
      </div>
      <Input
        id={id}
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        autoComplete="off"
        aria-required={required}
      />
      {hint ? (
        <p className="text-caption text-fg-2">{hint.replace(/^💡\s*/u, "")}</p>
      ) : null}
    </div>
  );
}
