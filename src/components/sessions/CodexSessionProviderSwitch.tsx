import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Switch } from "@/components/ui/switch";
import { sessionsApi } from "@/lib/api";
import { cn } from "@/lib/utils";
import { extractErrorMessage } from "@/utils/errorUtils";
import type { SessionMeta } from "@/types";

type CodexProviderGroup = "custom" | "openai";

interface CodexSessionProviderSwitchProps {
  session: SessionMeta;
}

function isCodexProviderGroup(
  value: string | null | undefined,
): value is CodexProviderGroup {
  return value === "custom" || value === "openai";
}

export function CodexSessionProviderSwitch({
  session,
}: CodexSessionProviderSwitchProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const enabled = session.providerId === "codex" && Boolean(session.sourcePath);
  const queryKey = [
    "codexSessionProvider",
    session.sessionId,
    session.sourcePath,
  ];

  const { data: provider } = useQuery({
    queryKey,
    enabled,
    queryFn: () =>
      sessionsApi.getCodexProvider(session.sessionId, session.sourcePath!),
  });

  const mutation = useMutation({
    mutationFn: async (targetProvider: CodexProviderGroup) => {
      return await sessionsApi.switchCodexProvider({
        sessionId: session.sessionId,
        sourcePath: session.sourcePath!,
        targetProvider,
      });
    },
    onSuccess: async (updatedProvider) => {
      queryClient.setQueryData(queryKey, updatedProvider);
      await queryClient.invalidateQueries({ queryKey: ["sessions"] });
      toast.success(
        t("sessionManager.codexProviderSwitchSuccess", {
          defaultValue: "Codex 会话分组已更新",
        }),
      );
    },
    onError: (error: Error) => {
      const detail = extractErrorMessage(error) || t("common.unknown");
      toast.error(
        t("sessionManager.codexProviderSwitchFailed", {
          defaultValue: "Codex 会话分组更新失败: {{error}}",
          error: detail,
        }),
      );
    },
  });

  if (!enabled || !isCodexProviderGroup(provider)) {
    return null;
  }

  const handleSwitch = (checked: boolean) => {
    const targetProvider: CodexProviderGroup = checked ? "openai" : "custom";
    if (provider === targetProvider) return;
    mutation.mutate(targetProvider);
  };

  return (
    <div className="mt-3 flex flex-wrap items-center justify-between gap-3 rounded-md border bg-muted/30 px-3 py-2">
      <div>
        <div className="text-xs font-medium">
          {t("sessionManager.codexProviderGroup", {
            defaultValue: "Codex 会话分组",
          })}
        </div>
        <div className="text-[11px] text-muted-foreground">
          {t("sessionManager.codexProviderGroupHint", {
            defaultValue: "切换后，此会话会出现在 Codex 对应分组的历史列表中。",
          })}
        </div>
      </div>
      <div className="flex items-center gap-2">
        <span
          className={cn(
            "text-xs transition-colors",
            provider === "custom"
              ? "font-semibold text-primary"
              : "text-muted-foreground",
          )}
        >
          Custom
        </span>
        <Switch
          checked={provider === "openai"}
          disabled={mutation.isPending}
          aria-label={t("sessionManager.codexProviderGroup", {
            defaultValue: "Codex 会话分组",
          })}
          onCheckedChange={handleSwitch}
        />
        <span
          className={cn(
            "text-xs transition-colors",
            provider === "openai"
              ? "font-semibold text-primary"
              : "text-muted-foreground",
          )}
        >
          OpenAI
        </span>
      </div>
    </div>
  );
}
