import React, { useState } from "react";
import { Eye, EyeOff } from "lucide-react";
import { useTranslation } from "react-i18next";

interface ApiKeyInputProps {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  disabled?: boolean;
  required?: boolean;
  label?: string;
  id?: string;
}

const ApiKeyInput: React.FC<ApiKeyInputProps> = ({
  value,
  onChange,
  placeholder,
  disabled = false,
  required = false,
  label = "API Key",
  id = "apiKey",
}) => {
  const { t } = useTranslation();
  const [showKey, setShowKey] = useState(false);

  const toggleShowKey = () => {
    setShowKey(!showKey);
  };

  const inputClass = `w-full px-3 py-2 pr-10 border rounded-lg text-sm transition-colors ${
    disabled
      ? "bg-muted border-border-default text-muted-foreground cursor-not-allowed"
      : "border-border-default dark:bg-gray-800 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-blue-500/20 dark:focus:ring-blue-400/20"
  }`;

  return (
    <div className="space-y-2">
      <label
        htmlFor={id}
        className="block text-sm font-medium text-gray-900 dark:text-gray-100"
      >
        {label} {required && "*"}
      </label>
      <div className="relative">
        <input
          type={showKey ? "text" : "password"}
          id={id}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder ?? t("apiKeyInput.placeholder")}
          disabled={disabled}
          required={required}
          autoComplete="off"
          className={inputClass}
        />
        {!disabled && value && (
          <button
            type="button"
            onClick={toggleShowKey}
            className="absolute inset-y-0 right-0 flex items-center pr-3 text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-100 transition-colors"
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
