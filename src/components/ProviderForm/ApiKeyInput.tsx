import React, { useState } from "react";
import { Eye, EyeOff } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";

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
  const [showKey, setShowKey] = useState(false);

  const toggleShowKey = () => {
    setShowKey(!showKey);
  };

  return (
    <div className="space-y-2">
      <Label htmlFor={id}>
        {label} {required && "*"}
      </Label>
      <div className="relative">
        <Input
          type={showKey ? "text" : "password"}
          id={id}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
          disabled={disabled}
          required={required}
          autoComplete="off"
          className="pr-10"
        />
        {!disabled && value && (
          <Button
            type="button"
            variant="ghost"
            size="icon"
            onClick={toggleShowKey}
            className="absolute inset-y-0 right-0 h-full w-10 hover:bg-transparent"
            aria-label={showKey ? "隐藏API Key" : "显示API Key"}
          >
            {showKey ? <EyeOff size={16} /> : <Eye size={16} />}
          </Button>
        )}
      </div>
    </div>
  );
};

export default ApiKeyInput;
