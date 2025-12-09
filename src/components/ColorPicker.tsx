import React from "react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";

interface ColorPickerProps {
  value?: string;
  onValueChange: (color: string) => void;
  label?: string;
  presets?: string[];
}

const DEFAULT_PRESETS = [
  "#00A67E",
  "#D4915D",
  "#4285F4",
  "#FF6A00",
  "#00A4FF",
  "#FF9900",
  "#0078D4",
  "#FF0000",
  "#1E88E5",
  "#6366F1",
  "#0F62FE",
  "#2932E1",
];

export const ColorPicker: React.FC<ColorPickerProps> = ({
  value = "#4285F4",
  onValueChange,
  label = "图标颜色",
  presets = DEFAULT_PRESETS,
}) => {
  return (
    <div className="space-y-3">
      <Label>{label}</Label>

      {/* 颜色预设 */}
      <div className="grid grid-cols-6 gap-2">
        {presets.map((color) => (
          <button
            key={color}
            type="button"
            onClick={() => onValueChange(color)}
            className={cn(
              "w-full aspect-square rounded-lg border-2 transition-all",
              "hover:scale-110 hover:shadow-lg",
              value === color
                ? "border-primary ring-2 ring-primary/20"
                : "border-border",
            )}
            style={{ backgroundColor: color }}
            title={color}
          />
        ))}
      </div>

      {/* 自定义颜色输入 */}
      <div className="flex items-center gap-2">
        <Input
          type="color"
          value={value}
          onChange={(e) => onValueChange(e.target.value)}
          className="w-16 h-10 p-1 cursor-pointer"
        />
        <Input
          type="text"
          value={value}
          onChange={(e) => onValueChange(e.target.value)}
          placeholder="#4285F4"
          className="flex-1 font-mono"
        />
      </div>
    </div>
  );
};
