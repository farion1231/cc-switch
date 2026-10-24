import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Notice } from "@/components/ui/notice";
import { clineApi } from "@/lib/api/cline";
import { copyText } from "@/lib/clipboard";
import { toast } from "@/lib/toast";

export const clineHubStatusKey = ["cline", "hubStatus"] as const;
const RESTART_COMMAND = "cline doctor fix";

/**
 * The Cline CLI and desktop app share one `cline-hub` that reads custom
 * providers only when it starts, and quitting the desktop app leaves it
 * running. Warn while that hub predates the last custom-provider change.
 */
export function ClineHubNotice() {
  const { t } = useTranslation();
  const { data } = useQuery({
    queryKey: clineHubStatusKey,
    queryFn: () => clineApi.getHubStatus(),
    refetchInterval: 10_000,
    refetchOnWindowFocus: true,
  });
  if (!data?.stale) return null;

  return (
    <Notice
      tone="warning"
      title={t("cline.hub.staleTitle")}
      actions={
        <Button
          size="compact"
          variant="neutral"
          onClick={() =>
            void copyText(RESTART_COMMAND)
              .then(() =>
                toast.success(
                  t("cline.hub.copied", { command: RESTART_COMMAND }),
                ),
              )
              .catch(() => toast.error(t("cline.hub.copyFailed")))
          }
        >
          {t("cline.hub.copyCommand")}
        </Button>
      }
    >
      {t("cline.hub.staleBody", {
        pid: data.pid ?? "?",
        command: RESTART_COMMAND,
      })}
    </Notice>
  );
}
