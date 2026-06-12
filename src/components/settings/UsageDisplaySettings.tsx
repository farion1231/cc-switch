import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { useTranslation } from "react-i18next";

type UsageDisplayOrder = "remaining-first" | "used-first";

interface UsageDisplaySettingsProps {
  value: UsageDisplayOrder;
  onChange: (value: UsageDisplayOrder) => void;
}

export function UsageDisplaySettings({
  value,
  onChange,
}: UsageDisplaySettingsProps) {
  const { t } = useTranslation();

  return (
    <section className="space-y-2">
      <header className="space-y-1">
        <h3 className="text-sm font-medium">
          {t("settings.usageDisplayOrder")}
        </h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.usageDisplayOrderHint")}
        </p>
      </header>
      <div className="inline-flex gap-1 rounded-md border border-border-default bg-background p-1">
        <OrderButton
          active={value === "remaining-first"}
          onClick={() => onChange("remaining-first")}
        >
          {t("settings.usageDisplayRemainingFirst")}
        </OrderButton>
        <OrderButton
          active={value === "used-first"}
          onClick={() => onChange("used-first")}
        >
          {t("settings.usageDisplayUsedFirst")}
        </OrderButton>
      </div>
    </section>
  );
}

interface OrderButtonProps {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}

function OrderButton({ active, onClick, children }: OrderButtonProps) {
  return (
    <Button
      type="button"
      onClick={onClick}
      size="sm"
      variant={active ? "default" : "ghost"}
      className={cn(
        "min-w-[120px]",
        active
          ? "shadow-sm"
          : "text-muted-foreground hover:text-foreground hover:bg-muted",
      )}
    >
      {children}
    </Button>
  );
}
