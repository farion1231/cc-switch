import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Play, RotateCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { restartCodexDesktop } from "@/lib/api/codexDesktop";
import { toast } from "@/lib/toast";
import { useCodexDesktopStatus } from "@/hooks/useCodexDesktopStatus";

export function CodexDesktopRestartButton() {
  const { t } = useTranslation();
  const [pending, setPending] = useState(false);
  const { data: status, isError, refetch } = useCodexDesktopStatus();
  const restart = async () => {
    if (pending) return;
    setPending(true);
    try {
      const result = await restartCodexDesktop();
      toast.success(
        t(
          result.action === "started"
            ? "codexDesktop.opened"
            : "codexDesktop.restarted",
        ),
      );
    } catch (error) {
      toast.error(
        t("codexDesktop.failed", {
          error: error instanceof Error ? error.message : String(error),
        }),
      );
    } finally {
      setPending(false);
      void refetch();
    }
  };
  if (isError || !status?.supported || !status.installed) return null;
  const Icon = status.running ? RotateCw : Play;
  return (
    <Button
      variant="quiet"
      size="regular"
      disabled={pending}
      onClick={() => void restart()}
      title={t(
        status.running ? "codexDesktop.restartHint" : "codexDesktop.openHint",
      )}
    >
      <Icon className={pending ? "h-3.5 w-3.5 animate-spin" : "h-3.5 w-3.5"} />
      {t(
        pending
          ? status.running
            ? "codexDesktop.restarting"
            : "codexDesktop.opening"
          : status.running
            ? "codexDesktop.restart"
            : "codexDesktop.open",
      )}
    </Button>
  );
}
