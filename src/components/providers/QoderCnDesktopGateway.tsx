import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, Copy } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { toast } from "@/lib/toast";
import {
  qoderCnDesktopApi,
  qoderCnGatewayQueryKey,
} from "@/lib/api/qoderCnDesktop";
import {
  extractErrorMessage,
  translateQoderProviderMutationError,
} from "@/utils/errorUtils";

export function QoderCnDesktopGateway({
  hasProvider,
}: {
  hasProvider: boolean;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data, error, isPending } = useQuery({
    queryKey: qoderCnGatewayQueryKey,
    queryFn: qoderCnDesktopApi.status,
    refetchInterval: 5000,
  });
  const formatError = (error: unknown) => {
    const message = extractErrorMessage(error);
    return (
      translateQoderProviderMutationError(message, t) ||
      message ||
      t("common.error")
    );
  };
  const toggleGateway = useMutation({
    mutationFn: qoderCnDesktopApi.setGateway,
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: qoderCnGatewayQueryKey }),
    onError: (error) => toast.error(formatError(error)),
  });

  return (
    <details className="group rounded-panel border border-border bg-surface">
      <summary className="flex cursor-pointer list-none items-center gap-2 px-4 py-3 text-body [&::-webkit-details-marker]:hidden">
        <ChevronDown
          className="h-4 w-4 -rotate-90 group-open:rotate-0"
          aria-hidden
        />
        {t("qoderCnDesktop.gateway")}
        {data && (
          <span className="ms-auto text-caption text-fg-2">
            {t(
              data.running
                ? "qoderCnDesktop.running"
                : "qoderCnDesktop.stopped",
            )}
          </span>
        )}
      </summary>
      <div className="space-y-3 border-t border-border p-4">
        {isPending && <p>{t("common.loading")}</p>}
        {error && (
          <p role="alert" className="text-destructive">
            {formatError(error)}
          </p>
        )}
        {data && (
          <>
            <div className="flex flex-wrap items-center justify-between gap-2">
              <p className="text-sm text-muted-foreground">
                {t("qoderCnDesktop.setup")}
              </p>
              <div className="flex gap-2">
                {data.enabled && (
                  <Button
                    variant="neutral"
                    disabled={toggleGateway.isPending}
                    onClick={() => toggleGateway.mutate(false)}
                  >
                    {t("qoderCnDesktop.stop")}
                  </Button>
                )}
                {!data.running && (
                  <Button
                    disabled={toggleGateway.isPending || !hasProvider}
                    onClick={() => toggleGateway.mutate(true)}
                  >
                    {t("qoderCnDesktop.start")}
                  </Button>
                )}
              </div>
            </div>
            <dl className="grid gap-2 sm:grid-cols-3">
              {[
                { label: "Base URL", value: data.baseUrl },
                { label: "Model ID", value: data.model },
                { label: "API Key", value: data.gatewayToken, secret: true },
              ].map(({ label, value, secret }) => (
                <div
                  key={label}
                  className="min-w-0 rounded-md bg-muted/40 px-2.5 py-1.5"
                >
                  <dt className="text-xs text-muted-foreground">{label}</dt>
                  <dd className="flex items-center justify-between gap-1">
                    <code className="truncate text-xs">
                      {value ? (secret ? "••••••••" : value) : "—"}
                    </code>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7 shrink-0"
                      disabled={!value}
                      aria-label={t("qoderCnDesktop.copy", { label })}
                      onClick={() => {
                        void navigator.clipboard.writeText(value).then(
                          () => toast.success(t("qoderCnDesktop.copied")),
                          (error) => toast.error(formatError(error)),
                        );
                      }}
                    >
                      <Copy className="h-3.5 w-3.5" />
                    </Button>
                  </dd>
                </div>
              ))}
            </dl>
          </>
        )}
      </div>
    </details>
  );
}
