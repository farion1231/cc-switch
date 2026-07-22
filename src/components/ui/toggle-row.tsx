import { Switch } from "@/components/ui/switch";

export interface ToggleRowProps {
  icon: React.ReactNode;
  title: string;
  description?: string;
  checked: boolean;
  onCheckedChange: (value: boolean) => void;
  disabled?: boolean;
}

export function ToggleRow({
  icon,
  title,
  description,
  checked,
  onCheckedChange,
  disabled,
}: ToggleRowProps) {
  return (
    <div className="flex items-center justify-between gap-4 rounded-2xl glass-card p-4 transition-all hover:bg-white/40 dark:hover:bg-white/[0.07]">
      <div className="flex items-center gap-3">
        <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-xl bg-white/50 dark:bg-white/10 ring-1 ring-white/40 dark:ring-white/10 shadow-[inset_0_1px_0_rgba(255,255,255,0.5)]">
          {icon}
        </div>
        <div className="space-y-1">
          <p className="text-sm font-medium leading-none">{title}</p>
          {description ? (
            <p className="text-xs text-muted-foreground">{description}</p>
          ) : null}
        </div>
      </div>
      <Switch
        checked={checked}
        onCheckedChange={onCheckedChange}
        disabled={disabled}
        aria-label={title}
      />
    </div>
  );
}
