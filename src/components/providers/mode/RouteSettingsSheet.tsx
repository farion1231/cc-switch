import { useTranslation } from "react-i18next";
import { ChevronRight } from "lucide-react";
import type { ProxyAppId } from "@/config/appConfig";
import {
  Sheet,
  SheetBody,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Button } from "@/components/ui/button";
import { AutoFailoverConfigPanel } from "@/components/proxy/AutoFailoverConfigPanel";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";

interface RouteSettingsSheetProps {
  app: ProxyAppId;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onOpenSettings: () => void;
}

/** 路由页签上的「路由设置 ›」：当前应用的超时、重试、熔断；更多在设置 → 本地路由。 */
export function RouteSettingsSheet({
  app,
  open,
  onOpenChange,
  onOpenSettings,
}: RouteSettingsSheetProps) {
  const { t } = useTranslation();
  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent width={420} closeLabel={t("common.close")}>
        <SheetHeader>
          <SheetTitle>{t("mode.routeSettingsTitle")}</SheetTitle>
          <SheetDescription>{APP_DISPLAY_NAME[app]}</SheetDescription>
        </SheetHeader>
        <SheetBody>
          <AutoFailoverConfigPanel appType={app} />
        </SheetBody>
        <SheetFooter className="justify-start">
          <Button
            variant="quiet"
            size="compact"
            onClick={() => {
              onOpenChange(false);
              onOpenSettings();
            }}
          >
            {t("mode.moreRouteSettings")}
            <ChevronRight className="h-3.5 w-3.5" />
          </Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}
