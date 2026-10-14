import { useState } from "react";
import { useTranslation } from "react-i18next";
import { RotateCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { restartCodexDesktop } from "@/lib/api/codexDesktop";
import { toast } from "@/lib/toast";

export function CodexDesktopRestartButton() {
  const { t } = useTranslation();
  const [pending, setPending] = useState(false);
  const restart = async () => {
    if (pending) return;
    setPending(true);
    try {
      await restartCodexDesktop();
      toast.success(t("codexDesktop.restarted"));
    } catch (error) {
      toast.error(
        t("codexDesktop.failed", {
          error: error instanceof Error ? error.message : String(error),
        }),
      );
    } finally {
      setPending(false);
    }
  };
  return (
    <Button
      variant="quiet"
      size="regular"
      disabled={pending}
      onClick={() => void restart()}
      title={t("codexDesktop.restartHint")}
    >
      <RotateCw
        className={pending ? "h-3.5 w-3.5 animate-spin" : "h-3.5 w-3.5"}
      />
      {t(pending ? "codexDesktop.restarting" : "codexDesktop.restart")}
    </Button>
  );
}
