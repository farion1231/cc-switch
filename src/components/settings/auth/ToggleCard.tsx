import { useId } from "react";
import { Switch } from "@/components/ui/switch";

/** 授权中心顶部的一行开关卡片：标题 + 说明 + 开关 */
export function ToggleCard({
  title,
  description,
  checked,
  disabled,
  onCheckedChange,
}: {
  title: string;
  description: string;
  checked: boolean;
  disabled?: boolean;
  onCheckedChange: (checked: boolean) => void;
}) {
  const titleId = useId();
  const descriptionId = useId();
  return (
    <div className="flex shrink-0 items-center gap-4 rounded-panel border border-border bg-surface py-3 pe-4 ps-4">
      <div className="min-w-0 flex-1 space-y-0.5">
        <p id={titleId} className="m-0 text-body font-medium text-fg-1">
          {title}
        </p>
        <p id={descriptionId} className="m-0 text-caption text-fg-2">
          {description}
        </p>
      </div>
      <Switch
        checked={checked}
        disabled={disabled}
        onCheckedChange={onCheckedChange}
        aria-labelledby={titleId}
        aria-describedby={descriptionId}
      />
    </div>
  );
}
