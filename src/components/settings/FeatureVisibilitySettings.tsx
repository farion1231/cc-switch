import { Book, History, SlidersHorizontal, Wrench } from "lucide-react";
import { useTranslation } from "react-i18next";
import { McpIcon } from "@/components/BrandIcons";
import { ToggleRow } from "@/components/ui/toggle-row";
import { normalizeFeatureVisibility } from "@/config/featureVisibility";
import type { SettingsFormState } from "@/hooks/useSettings";
import type { FeatureVisibility } from "@/types";

interface FeatureVisibilitySettingsProps {
  settings: SettingsFormState;
  onChange: (updates: Partial<SettingsFormState>) => void;
}

const FEATURE_ROWS: Array<{
  key: keyof FeatureVisibility;
  titleKey: string;
  descriptionKey: string;
  icon: React.ReactNode;
}> = [
  {
    key: "mcp",
    titleKey: "settings.featureVisibility.mcp",
    descriptionKey: "settings.featureVisibility.mcpDescription",
    icon: <McpIcon size={16} className="text-violet-500" />,
  },
  {
    key: "prompts",
    titleKey: "settings.featureVisibility.prompts",
    descriptionKey: "settings.featureVisibility.promptsDescription",
    icon: <Book className="h-4 w-4 text-sky-500" />,
  },
  {
    key: "sessions",
    titleKey: "settings.featureVisibility.sessions",
    descriptionKey: "settings.featureVisibility.sessionsDescription",
    icon: <History className="h-4 w-4 text-amber-500" />,
  },
  {
    key: "skills",
    titleKey: "settings.featureVisibility.skills",
    descriptionKey: "settings.featureVisibility.skillsDescription",
    icon: <Wrench className="h-4 w-4 text-emerald-500" />,
  },
];

export function FeatureVisibilitySettings({
  settings,
  onChange,
}: FeatureVisibilitySettingsProps) {
  const { t } = useTranslation();
  const featureVisibility = normalizeFeatureVisibility(
    settings.featureVisibility,
  );

  const handleToggle = (key: keyof FeatureVisibility, value: boolean) => {
    onChange({
      featureVisibility: {
        ...featureVisibility,
        [key]: value,
      },
    });
  };

  return (
    <section className="space-y-4">
      <div className="flex items-center gap-2 pb-2 border-b border-border/40">
        <SlidersHorizontal className="h-4 w-4 text-primary" />
        <div className="space-y-1">
          <h3 className="text-sm font-medium">
            {t("settings.featureVisibility.title")}
          </h3>
          <p className="text-xs text-muted-foreground">
            {t("settings.featureVisibility.description")}
          </p>
        </div>
      </div>

      <div className="grid gap-3 md:grid-cols-2">
        {FEATURE_ROWS.map((row) => (
          <ToggleRow
            key={row.key}
            icon={row.icon}
            title={t(row.titleKey)}
            description={t(row.descriptionKey)}
            checked={featureVisibility[row.key]}
            onCheckedChange={(value) => handleToggle(row.key, value)}
          />
        ))}
      </div>
    </section>
  );
}
