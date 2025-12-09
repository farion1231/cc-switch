import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { useTranslation } from "react-i18next";

type LanguageOption = "zh" | "en" | "ja";

interface LanguageSettingsProps {
  value: LanguageOption;
  onChange: (value: LanguageOption) => void;
}

export function LanguageSettings({ value, onChange }: LanguageSettingsProps) {
  const { t } = useTranslation();

  return (
    <section className="space-y-2">
      <header className="space-y-1">
        <h3 className="text-sm font-medium">{t("settings.language")}</h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.languageHint")}
        </p>
      </header>
      <div className="inline-flex gap-1 rounded-md border border-border-default bg-background p-1">
        <LanguageButton active={value === "zh"} onClick={() => onChange("zh")}>
          {t("settings.languageOptionChinese")}
        </LanguageButton>
        <LanguageButton active={value === "en"} onClick={() => onChange("en")}>
          {t("settings.languageOptionEnglish")}
        </LanguageButton>
        <LanguageButton active={value === "ja"} onClick={() => onChange("ja")}>
          {t("settings.languageOptionJapanese")}
        </LanguageButton>
      </div>
    </section>
  );
}

interface LanguageButtonProps {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}

function LanguageButton({ active, onClick, children }: LanguageButtonProps) {
  return (
    <Button
      type="button"
      onClick={onClick}
      size="sm"
      variant={active ? "default" : "ghost"}
      className={cn(
        "min-w-[96px]",
        active
          ? "shadow-sm"
          : "text-muted-foreground hover:text-foreground hover:bg-muted",
      )}
    >
      {children}
    </Button>
  );
}
