import React, { useState } from "react";
import { Copy, Eye, EyeOff } from "lucide-react";
import { useTranslation } from "react-i18next";
import { fieldClass } from "@/components/ui/input";
import { copyText } from "@/lib/clipboard";
import { toast } from "@/lib/toast";
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
  /** 输入框下方的说明文字 */
  hint?: React.ReactNode;
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
}) => {
  const { t } = useTranslation();
  const [showKey, setShowKey] = useState(false);
  // 复制和显示条件都按 trim 后判断，与表单「是否已填」的校验保持一致
  const trimmedValue = value.trim();

  const handleCopy = async () => {
    try {
      await copyText(trimmedValue);
      toast.success(t("apiKeyInput.copied"));
    } catch {
      toast.error(t("apiKeyInput.copyFailed"));
    }
  };

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
        {/* 复制在前、获取链接在后：Key 为空时复制按钮消失，链接不跳位 */}
        <div className="flex items-center gap-3">
          {!disabled && trimmedValue && (
            <button
              type="button"
              onClick={handleCopy}
              className="flex items-center gap-1 text-caption text-fg-2 transition-colors hover:text-fg-1"
            >
              <Copy className="h-3.5 w-3.5" />
              {t("common.copy")}
            </button>
          )}
          {labelAside}
        </div>
      </div>
      <div className="relative">
        <input
          type={showKey ? "text" : "password"}
          id={id}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder ?? t("apiKeyInput.placeholder")}
          disabled={disabled}
          // 只标给读屏，不用原生 required：空 Key 由表单校验弹「仍要保存」确认，
          // 原生校验会抢先拦下提交
          aria-required={required || undefined}
          autoComplete="off"
          // 切成明文后手打 Key，macOS 会把首字母改成大写
          autoCorrect="off"
          autoCapitalize="off"
          spellCheck={false}
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
      {hint && <p className="text-caption text-fg-2">{hint}</p>}
    </div>
  );
};

export default ApiKeyInput;
