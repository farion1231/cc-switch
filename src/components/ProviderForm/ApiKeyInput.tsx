import React from "react";

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
  placeholder = "请输入API Key",
  disabled = false,
  required = false,
  label = "API Key",
  id = "apiKey",
}) => {
  const inputClass = `w-full px-3 py-2 border rounded-lg text-sm transition-colors ${
    disabled
      ? "bg-[var(--color-bg-tertiary)] border-[var(--color-border)] text-[var(--color-text-tertiary)] cursor-not-allowed"
      : "border-[var(--color-border)] focus:outline-none focus:ring-2 focus:ring-[var(--color-primary)]/20 focus:border-[var(--color-primary)]"
  }`;

  return (
    <div className="space-y-2">
      <label
        htmlFor={id}
        className="block text-sm font-medium text-[var(--color-text-primary)]"
      >
        {label} {required && "*"}
      </label>
      <input
        type="text"
        id={id}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        disabled={disabled}
        required={required}
        autoComplete="off"
        className={inputClass}
      />
    </div>
  );
};

export default ApiKeyInput;