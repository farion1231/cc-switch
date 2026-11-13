import { Loader2, Play, RotateCw, type LucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { Button } from "@/components/ui/button";
import { HoverTip } from "@/components/ui/hover-tip";
import { useDesktopAppStatus } from "@/hooks/useDesktopAppStatus";
import type { AppId } from "@/lib/api";

interface DesktopAppControlsProps {
  app: AppId;
}

interface ControlButtonProps {
  tip: string;
  label: string;
  icon: LucideIcon;
  busy: boolean;
  disabled: boolean;
  onClick: () => void;
}

function ControlButton({
  tip,
  label,
  icon: Icon,
  busy,
  disabled,
  onClick,
}: ControlButtonProps) {
  const button = (
    <Button
      variant="neutral"
      size="compact"
      className="px-2 xl:px-3"
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
    >
      {busy ? (
        <Loader2 className="h-3.5 w-3.5 animate-spin" />
      ) : (
        <Icon className="h-3.5 w-3.5" />
      )}
      <span className="hidden xl:inline">{label}</span>
    </Button>
  );

  return (
    <HoverTip content={tip}>
      {disabled ? <span className="inline-flex">{button}</span> : button}
    </HoverTip>
  );
}

export function DesktopAppControls({ app }: DesktopAppControlsProps) {
  const { t } = useTranslation();
  const {
    supported,
    checking,
    installed,
    running,
    launch,
    restart,
    launching,
    restarting,
  } = useDesktopAppStatus(app);

  if (!supported) return null;

  const name = APP_DISPLAY_NAME[app];
  const busy = launching || restarting;

  const startTip = t(
    `appPage.desktopApp.${
      checking
        ? "checking"
        : !installed
          ? "notInstalled"
          : running
            ? "alreadyRunning"
            : "startApp"
    }`,
    { name },
  );
  const restartTip = t(
    `appPage.desktopApp.${
      checking ? "checking" : !installed ? "notInstalled" : "restartApp"
    }`,
    { name },
  );

  return (
    <>
      <ControlButton
        tip={startTip}
        label={t("appPage.desktopApp.startApp")}
        icon={Play}
        busy={launching}
        disabled={busy || !installed || running}
        onClick={() => launch()}
      />
      <ControlButton
        tip={restartTip}
        label={t("appPage.desktopApp.restartApp")}
        icon={RotateCw}
        busy={restarting}
        disabled={busy || !installed}
        onClick={() => restart()}
      />
    </>
  );
}
