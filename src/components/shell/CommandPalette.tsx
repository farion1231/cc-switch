import type { ComponentType } from "react";
import { useTranslation } from "react-i18next";
import {
  BookOpen,
  ChartColumn,
  Database,
  Folder,
  Globe,
  History,
  Info,
  KeyRound,
  LayoutGrid,
  Route,
  Server,
  SlidersHorizontal,
} from "lucide-react";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { SkillsIcon } from "@/components/BrandIcons";
import type { AppId } from "@/lib/api";
import type { VisibleApps } from "@/types";
import { APP_IDS } from "@/config/appConfig";
import type { GlobalPage, SettingsSection } from "@/lib/navigation";
import { APP_DISPLAY_NAME, AppGlyph } from "./AppGlyph";

type IconComponent = ComponentType<{ className?: string }>;

interface CommandPaletteProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  visibleApps: VisibleApps;
  onSelectApp: (app: AppId) => void;
  onSelectPage: (page: GlobalPage) => void;
  onOpenSettings: (section: SettingsSection) => void;
}

const PAGES: {
  page: GlobalPage;
  labelKey?: string;
  label?: string;
  icon: IconComponent;
}[] = [
  { page: "usage", labelKey: "nav.usage", icon: ChartColumn },
  { page: "auth", labelKey: "nav.auth", icon: KeyRound },
  { page: "mcp", label: "MCP", icon: Server },
  { page: "skills", label: "Skills", icon: SkillsIcon },
  { page: "prompts", labelKey: "nav.prompts", icon: BookOpen },
  { page: "sessions", labelKey: "nav.sessions", icon: History },
  { page: "apps", labelKey: "nav.apps", icon: LayoutGrid },
];

const SETTINGS: { section: SettingsSection; icon: IconComponent }[] = [
  { section: "general", icon: SlidersHorizontal },
  { section: "appConfig", icon: Folder },
  { section: "routing", icon: Route },
  { section: "network", icon: Globe },
  { section: "data", icon: Database },
  { section: "about", icon: Info },
];

/**
 * ⌘K：输入应用名、页面名、设置分组名直接跳过去（应用多、侧栏放不下时用）。
 */
export function CommandPalette({
  open,
  onOpenChange,
  visibleApps,
  onSelectApp,
  onSelectPage,
  onOpenSettings,
}: CommandPaletteProps) {
  const { t } = useTranslation();
  const run = (action: () => void) => {
    onOpenChange(false);
    action();
  };
  const apps = APP_IDS.filter((app) => visibleApps[app] !== false);

  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="fixed inset-0 z-[80] bg-overlay" />
        <DialogPrimitive.Content
          aria-describedby={undefined}
          className="fixed start-1/2 top-[18%] z-[80] w-[min(92vw,520px)] -translate-x-1/2 overflow-hidden rounded-dialog border border-border bg-surface shadow-v7-lg rtl:translate-x-1/2"
        >
          <DialogPrimitive.Title className="sr-only">
            {t("commandPalette.title")}
          </DialogPrimitive.Title>
          <Command className="bg-transparent" loop>
            <CommandInput placeholder={t("commandPalette.placeholder")} />
            <CommandList className="max-h-[360px]">
              <CommandEmpty>{t("commandPalette.empty")}</CommandEmpty>
              <CommandGroup heading={t("commandPalette.apps")}>
                {apps.map((app) => (
                  <CommandItem
                    key={app}
                    value={`app ${APP_DISPLAY_NAME[app]} ${app}`}
                    onSelect={() => run(() => onSelectApp(app))}
                  >
                    <AppGlyph app={app} size={16} />
                    {APP_DISPLAY_NAME[app]}
                  </CommandItem>
                ))}
              </CommandGroup>
              <CommandGroup heading={t("commandPalette.pages")}>
                {PAGES.map(({ page, labelKey, label, icon: Icon }) => {
                  const text = labelKey ? t(labelKey) : (label ?? page);
                  return (
                    <CommandItem
                      key={page}
                      value={`page ${text} ${page}`}
                      onSelect={() => run(() => onSelectPage(page))}
                    >
                      <Icon className="h-4 w-4 text-fg-2" />
                      {text}
                    </CommandItem>
                  );
                })}
              </CommandGroup>
              <CommandGroup heading={t("commandPalette.settings")}>
                {SETTINGS.map(({ section, icon: Icon }) => {
                  const text = t(`settings.sections.${section}`);
                  return (
                    <CommandItem
                      key={section}
                      value={`settings ${text} ${section}`}
                      onSelect={() => run(() => onOpenSettings(section))}
                    >
                      <Icon className="h-4 w-4 text-fg-2" />
                      {text}
                    </CommandItem>
                  );
                })}
              </CommandGroup>
            </CommandList>
          </Command>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}
