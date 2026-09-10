import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { LayoutDashboard, BarChart2 } from "lucide-react";

export type HomePageMode = "default" | "usage";

interface HomePageSettingsProps {
  value: HomePageMode;
  onChange: (value: HomePageMode) => void;
}

export function HomePageSettings({ value, onChange }: HomePageSettingsProps) {
  const { t } = useTranslation();

  return (
    <section className="space-y-2">
      <header className="space-y-1">
        <h3 className="text-sm font-medium">{t("settings.homePage.title")}</h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.homePage.description")}
        </p>
      </header>
      <div className="inline-flex gap-1 rounded-md border border-border-default bg-background p-1">
        <HomePageButton
          active={value === "default"}
          onClick={() => onChange("default")}
        >
          <LayoutDashboard className="mr-2 h-4 w-4" />
          {t("settings.homePage.default")}
        </HomePageButton>
        <HomePageButton
          active={value === "usage"}
          onClick={() => onChange("usage")}
        >
          <BarChart2 className="mr-2 h-4 w-4" />
          {t("settings.homePage.usage")}
        </HomePageButton>
      </div>
      <p className="text-xs text-muted-foreground">
        {value === "usage"
          ? t("settings.homePage.usageHint")
          : t("settings.homePage.defaultHint")}
      </p>
    </section>
  );
}

interface HomePageButtonProps {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}

function HomePageButton({ active, onClick, children }: HomePageButtonProps) {
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
