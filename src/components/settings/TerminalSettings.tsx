import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useTranslation } from "react-i18next";

interface TerminalSettingsProps {
  value?: string;
  onChange: (value: string) => void;
}

export function TerminalSettings({ value, onChange }: TerminalSettingsProps) {
  const { t } = useTranslation();

  return (
    <div className="space-y-2">
      <Label>{t("settings.advanced.terminalPath.label")}</Label>
      <Input
        value={value ?? ""}
        placeholder={t("settings.advanced.terminalPath.placeholder")}
        onChange={(event) => onChange(event.target.value)}
      />
      <p className="text-sm text-muted-foreground">
        {t("settings.advanced.terminalPath.description")}
      </p>
    </div>
  );
}
