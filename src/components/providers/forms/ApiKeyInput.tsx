import React, { useMemo, useState } from "react";
import { Eye, EyeOff } from "lucide-react";
import { useTranslation } from "react-i18next";
import { fieldClass } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { REQUIRED_LABEL } from "./BasicFormFields";
import { useApiKeyVars } from "@/lib/query/apiKeyVars";
import { apiKeyRefStatus, parseApiKeyRef } from "@/lib/api/apiKeyVars";

interface ApiKeyInputProps {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  disabled?: boolean;
  required?: boolean;
  label?: string;
  id?: string;
  /** 标签行右侧（「获取 API Key ↗」） */
  labelAside?: React.ReactNode;
  /** 输入框下方的说明文字 */
  hint?: React.ReactNode;
  /** 允许填写 `{{NAME}}` 引用 API Key 变量 */
  allowVarRef?: boolean;
}

const ApiKeyInput: React.FC<ApiKeyInputProps> = ({
  value,
  onChange,
  placeholder,
  disabled = false,
  required = false,
  label = "API Key",
  id = "apiKey",
  labelAside,
  hint,
  allowVarRef = false,
}) => {
  const { t } = useTranslation();
  const [showKey, setShowKey] = useState(false);
  const [focused, setFocused] = useState(false);
  const { data: vars } = useApiKeyVars(allowVarRef);
  const refName = allowVarRef ? parseApiKeyRef(value) : null;
  const refInfo = allowVarRef ? apiKeyRefStatus(value, vars) : null;

  // 输入以 `{{` 开头时提示已定义的变量
  const suggestions = useMemo(() => {
    if (!allowVarRef || !vars?.length || refName) return [];
    const trimmed = value.trimStart();
    if (!trimmed.startsWith("{{")) return [];
    const prefix = trimmed.slice(2).replace(/\}+$/, "").toUpperCase();
    return vars.filter((v) => v.name.startsWith(prefix)).slice(0, 8);
  }, [allowVarRef, vars, value, refName]);

  const statusLine = (() => {
    if (!refInfo) return null;
    const { name, status, variable } = refInfo;
    const expires = variable?.state.expiresAt
      ? new Date(variable.state.expiresAt).toLocaleString()
      : t("apiKeyVars.noExpiry");
    const ok = status === "ok";
    const text = ok
      ? t("apiKeyVars.refOk", {
          name,
          value: variable?.maskedValue ?? "",
          time: expires,
        })
      : t(`apiKeyVars.ref.${status}`, { name });
    return (
      <p
        className={cn("text-caption", ok ? "text-fg-2" : "text-danger-text")}
        data-testid="api-key-ref-status"
      >
        {text}
      </p>
    );
  })();

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between gap-2">
        <label
          htmlFor={id}
          className={cn(
            "block text-caption font-medium text-fg-1",
            required && REQUIRED_LABEL,
          )}
        >
          {label}
        </label>
        {labelAside}
      </div>
      <div className="relative">
        <input
          type={showKey || refName ? "text" : "password"}
          id={id}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onFocus={() => setFocused(true)}
          onBlur={() => setTimeout(() => setFocused(false), 150)}
          placeholder={placeholder ?? t("apiKeyInput.placeholder")}
          disabled={disabled}
          // 只标给读屏，不用原生 required：空 Key 由表单校验弹「仍要保存」确认，
          // 原生校验会抢先拦下提交
          aria-required={required || undefined}
          autoComplete="off"
          className={cn(
            fieldClass,
            "h-8 pe-10",
            disabled && "border-border bg-subtle text-fg-3 opacity-100",
          )}
        />
        {!disabled && value && !refName && (
          <button
            type="button"
            onClick={() => setShowKey(!showKey)}
            className="absolute inset-y-0 end-0 flex items-center pe-3 text-fg-3 transition-colors hover:text-fg-1"
            aria-label={showKey ? t("apiKeyInput.hide") : t("apiKeyInput.show")}
          >
            {showKey ? <EyeOff size={16} /> : <Eye size={16} />}
          </button>
        )}
        {focused && suggestions.length > 0 && (
          <ul
            role="listbox"
            className="absolute inset-x-0 top-full z-50 mt-1 max-h-48 overflow-auto rounded-control border border-border bg-surface py-1 shadow-v7-sm"
          >
            {suggestions.map((v) => (
              <li key={v.name}>
                <button
                  type="button"
                  role="option"
                  aria-selected={false}
                  className="flex w-full items-center justify-between gap-2 px-3 py-1.5 text-start text-caption hover:bg-subtle"
                  onMouseDown={(e) => {
                    e.preventDefault();
                    onChange(`{{${v.name}}}`);
                  }}
                >
                  <code className="font-mono text-fg-1">{`{{${v.name}}}`}</code>
                  <span className="text-fg-3">
                    {v.hasValue ? v.maskedValue : t("apiKeyVars.noValue")}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
      {statusLine}
      {allowVarRef && !value && !disabled && vars && vars.length > 0 && (
        <p className="text-caption text-fg-3">{t("apiKeyVars.inputHint")}</p>
      )}
      {hint && <p className="text-caption text-fg-2">{hint}</p>}
    </div>
  );
};

export default ApiKeyInput;
