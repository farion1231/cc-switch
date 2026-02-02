import React, { useState, useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { Keyboard, RotateCcw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

// Detect platform
const isMac =
  typeof navigator !== "undefined" &&
  /Mac|iPod|iPhone|iPad/.test(navigator.platform);

interface KeyboardShortcutsSettingsProps {
  searchShortcut?: string;
  onChange: (shortcut: string) => void;
}

// Format shortcut for display (e.g., "mod+k" -> "⌘K" or "Ctrl+K")
function formatShortcutDisplay(shortcut: string): string {
  const parts = shortcut.toLowerCase().split("+");
  const formatted = parts.map((part) => {
    if (part === "mod") return isMac ? "⌘" : "Ctrl";
    if (part === "shift") return isMac ? "⇧" : "Shift";
    if (part === "alt") return isMac ? "⌥" : "Alt";
    if (part === "ctrl") return "Ctrl";
    return part.toUpperCase();
  });
  return formatted.join(isMac ? "" : "+");
}

// Parse keyboard event to shortcut string
function eventToShortcut(event: KeyboardEvent): string | null {
  const key = event.key.toLowerCase();
  
  // Ignore modifier-only keys
  if (["control", "shift", "alt", "meta"].includes(key)) {
    return null;
  }

  const parts: string[] = [];
  
  // Use "mod" for platform-appropriate modifier
  if (isMac ? event.metaKey : event.ctrlKey) {
    parts.push("mod");
  }
  if (event.shiftKey) {
    parts.push("shift");
  }
  if (event.altKey) {
    parts.push("alt");
  }
  
  // Only allow shortcuts with at least one modifier
  if (parts.length === 0) {
    return null;
  }
  
  parts.push(key);
  return parts.join("+");
}

export const KeyboardShortcutsSettings: React.FC<KeyboardShortcutsSettingsProps> = ({
  searchShortcut = "mod+k",
  onChange,
}) => {
  const { t } = useTranslation();
  const [isRecording, setIsRecording] = useState(false);
  const [tempShortcut, setTempShortcut] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const displayValue = tempShortcut
    ? formatShortcutDisplay(tempShortcut)
    : formatShortcutDisplay(searchShortcut);

  const handleStartRecording = useCallback(() => {
    setIsRecording(true);
    setTempShortcut(null);
    // Focus the input to capture keyboard events
    setTimeout(() => inputRef.current?.focus(), 0);
  }, []);

  const handleStopRecording = useCallback(() => {
    setIsRecording(false);
    if (tempShortcut) {
      onChange(tempShortcut);
    }
    setTempShortcut(null);
  }, [tempShortcut, onChange]);

  const handleReset = useCallback(() => {
    onChange("mod+k");
    setTempShortcut(null);
    setIsRecording(false);
  }, [onChange]);

  // Handle keyboard events when recording
  useEffect(() => {
    if (!isRecording) return;

    const handleKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();

      const shortcut = eventToShortcut(event);
      if (shortcut) {
        setTempShortcut(shortcut);
      }
    };

    const handleKeyUp = () => {
      // Stop recording after key is released if we have a valid shortcut
      if (tempShortcut) {
        handleStopRecording();
      }
    };

    window.addEventListener("keydown", handleKeyDown, true);
    window.addEventListener("keyup", handleKeyUp, true);

    return () => {
      window.removeEventListener("keydown", handleKeyDown, true);
      window.removeEventListener("keyup", handleKeyUp, true);
    };
  }, [isRecording, tempShortcut, handleStopRecording]);

  // Handle blur to stop recording
  useEffect(() => {
    if (!isRecording) return;

    const handleBlur = () => {
      setIsRecording(false);
      setTempShortcut(null);
    };

    const input = inputRef.current;
    input?.addEventListener("blur", handleBlur);

    return () => {
      input?.removeEventListener("blur", handleBlur);
    };
  }, [isRecording]);

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-3">
        <Keyboard className="h-5 w-5 text-primary" />
        <div>
          <h3 className="text-base font-semibold">
            {t("settings.keyboard.title", { defaultValue: "键盘快捷键" })}
          </h3>
          <p className="text-sm text-muted-foreground">
            {t("settings.keyboard.description", {
              defaultValue: "自定义应用内的键盘快捷键",
            })}
          </p>
        </div>
      </div>

      <div className="space-y-3 pl-8">
        <div className="flex items-center justify-between gap-4">
          <Label className="text-sm font-medium">
            {t("settings.keyboard.search", { defaultValue: "打开搜索" })}
          </Label>
          <div className="flex items-center gap-2">
            <Input
              ref={inputRef}
              value={isRecording ? t("settings.keyboard.recording", { defaultValue: "按下快捷键..." }) : displayValue}
              readOnly
              onClick={handleStartRecording}
              className={`w-32 text-center cursor-pointer font-mono ${
                isRecording
                  ? "ring-2 ring-primary bg-primary/10"
                  : "hover:bg-muted/50"
              }`}
              placeholder={formatShortcutDisplay("mod+k")}
            />
            <Button
              variant="ghost"
              size="icon"
              onClick={handleReset}
              title={t("settings.keyboard.reset", { defaultValue: "重置为默认" })}
              className="h-9 w-9"
            >
              <RotateCcw className="h-4 w-4" />
            </Button>
          </div>
        </div>
        <p className="text-xs text-muted-foreground">
          {t("settings.keyboard.hint", {
            defaultValue: "点击输入框后按下新的快捷键组合",
          })}
        </p>
      </div>
    </div>
  );
};

export default KeyboardShortcutsSettings;
