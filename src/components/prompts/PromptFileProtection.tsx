import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { promptsApi, type AppId } from "@/lib/api";
import { Switch } from "@/components/ui/switch";
import { Button } from "@/components/ui/button";
import { toast } from "@/lib/toast";

export function usePromptFileProtection(
  app: AppId,
  onChanged: () => Promise<unknown>,
) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const supported = app === "claude" || app === "codex";
  const queryKey = ["prompt-file-protection", app];
  const query = useQuery({
    queryKey,
    queryFn: () => promptsApi.getFileProtection(app),
    enabled: supported,
  });
  const mutation = useMutation({
    mutationFn: ({ targetApp, value }: { targetApp: AppId; value: boolean }) =>
      promptsApi.setFileProtection(targetApp, value),
    onSuccess: async (_, { targetApp, value }) => {
      client.setQueryData(["prompt-file-protection", targetApp], value);
      if (targetApp === app) await onChanged();
    },
    onError: (error) =>
      toast.error(
        t("prompts.fileProtection.saveFailed", { error: String(error) }),
      ),
  });
  return {
    app,
    supported,
    query,
    mutation,
    readOnly:
      supported &&
      (query.isPending ||
        query.data === true ||
        query.isError ||
        mutation.isPending),
  };
}

export function PromptFileProtection({
  state,
  disabled,
  fileName,
}: {
  state: ReturnType<typeof usePromptFileProtection>;
  disabled: boolean;
  fileName: string;
}) {
  const { t } = useTranslation();
  if (!state.supported) return null;
  return (
    <div className="rounded-panel border border-border bg-subtle px-4 py-3">
      <div className="flex items-center justify-between gap-4">
        <label
          htmlFor="prompt-file-protection"
          className="text-body font-medium"
        >
          {t("prompts.fileProtection.label", { file: fileName })}
        </label>
        <Switch
          id="prompt-file-protection"
          checked={state.query.data === true}
          disabled={
            disabled ||
            state.query.isPending ||
            state.query.isError ||
            state.mutation.isPending
          }
          onCheckedChange={(value) =>
            state.mutation.mutate({ targetApp: state.app, value })
          }
        />
      </div>
      <p className="mt-1 text-caption text-fg-2">
        {t(
          state.query.data === true
            ? "prompts.fileProtection.protected"
            : "prompts.fileProtection.help",
        )}
      </p>
      {state.query.isError && (
        <div role="alert" className="mt-2 text-caption text-danger-text">
          {t("prompts.fileProtection.loadFailed")}
          <Button
            type="button"
            variant="quiet"
            size="compact"
            onClick={() => void state.query.refetch()}
          >
            {t("common.retry")}
          </Button>
        </div>
      )}
    </div>
  );
}
