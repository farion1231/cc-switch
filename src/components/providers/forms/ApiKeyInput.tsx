import React, { useState } from "react";
import { Eye, EyeOff } from "lucide-react";
import { useTranslation } from "react-i18next";
import { fieldClass } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { REQUIRED_LABEL } from "./BasicFormFields";

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
}) => {
  const { t } = useTranslation();
  const [showKey, setShowKey] = useState(false);

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
          type={showKey ? "text" : "password"}
          id={id}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder ?? t("apiKeyInput.placeholder")}
          disabled={disabled}
          required={required}
          aria-required={required || undefined}
          autoComplete="off"
          className={cn(
            fieldClass,
            "h-8 pe-10",
            disabled && "border-border bg-subtle text-fg-3 opacity-100",
          )}
        />
        {!disabled && value && (
          <button
            type="button"
            onClick={() => setShowKey(!showKey)}
            className="absolute inset-y-0 end-0 flex items-center pe-3 text-fg-3 transition-colors hover:text-fg-1"
            aria-label={showKey ? t("apiKeyInput.hide") : t("apiKeyInput.show")}
          >
            {showKey ? <EyeOff size={16} /> : <Eye size={16} />}
          </button>
        )}
      </div>
    </div>
  );
};

export default ApiKeyInput;
