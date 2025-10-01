import { AppType } from "../lib/query";
import { ClaudeIcon, CodexIcon } from "./BrandIcons";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";

interface AppSwitcherProps {
  activeApp: AppType;
  onSwitch: (app: AppType) => void;
}

export function AppSwitcher({ activeApp, onSwitch }: AppSwitcherProps) {
  return (
    <Tabs value={activeApp} onValueChange={(value) => onSwitch(value as AppType)}>
      <TabsList className="inline-flex bg-muted rounded-lg p-1 gap-1 border border-transparent">
        <TabsTrigger
          value="claude"
          className="group data-[state=active]:bg-background"
        >
          <ClaudeIcon
            size={16}
            className="text-[#D97757] dark:text-[#D97757] transition-colors duration-200 group-data-[state=inactive]:text-muted-foreground group-hover:text-[#D97757] dark:group-hover:text-[#D97757]"
          />
          <span>Claude</span>
        </TabsTrigger>

        <TabsTrigger
          value="codex"
          className="group data-[state=active]:bg-background"
        >
          <CodexIcon size={16} />
          <span>Codex</span>
        </TabsTrigger>
      </TabsList>
    </Tabs>
  );
}
