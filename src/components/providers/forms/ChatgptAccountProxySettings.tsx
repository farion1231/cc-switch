import { useId, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  getChatgptProxyConfig,
  setChatgptProxyConfig,
  type ChatgptProxyConfig,
} from "@/lib/api/auth";
import { toast } from "@/lib/toast";

const proxyConfigKey = ["codex_oauth", "proxy_config"] as const;
const defaultProxyConfig: ChatgptProxyConfig = { enabled: false, url: "" };

export function ChatgptAccountProxySettings({
  loginPending,
}: {
  loginPending: boolean;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const switchId = useId();
  const [draft, setDraft] = useState<ChatgptProxyConfig | null>(null);
  const config = useQuery({
    queryKey: proxyConfigKey,
    queryFn: getChatgptProxyConfig,
    networkMode: "always",
    retry: false,
  });
  const save = useMutation({
    mutationFn: setChatgptProxyConfig,
    networkMode: "always",
    onSuccess: (_, savedConfig) => {
      queryClient.setQueryData(proxyConfigKey, savedConfig);
      setDraft(null);
      void queryClient.invalidateQueries({
        queryKey: ["codex_oauth", "quota"],
      });
      toast.success(t("authCenter.loginProxy.saved"));
    },
    onError: (error) => toast.error(String(error)),
  });

  const saved = config.data ?? defaultProxyConfig;
  const current = draft ?? saved;
  const url = current.url.trim();
  const changed = current.enabled !== saved.enabled || url !== saved.url;
  const disabled = loginPending || config.isFetching || save.isPending;

  const saveConfig = () => {
    if (current.enabled && !url) {
      toast.error(t("authCenter.loginProxy.required"));
      return;
    }
    save.mutate({ enabled: current.enabled, url });
  };

  return (
    <div className="space-y-2 border-t border-border pt-3">
      <div className="flex items-center justify-between gap-3">
        <Label htmlFor={switchId} className="text-sm text-fg-1">
          {t("authCenter.loginProxy.label")}
        </Label>
        <Switch
          id={switchId}
          checked={current.enabled}
          onCheckedChange={(enabled) => setDraft({ ...current, enabled })}
          disabled={disabled}
        />
      </div>
      {current.enabled && (
        <Input
          aria-label={t("authCenter.loginProxy.url")}
          placeholder="http://127.0.0.1:7890"
          value={current.url}
          onChange={(event) =>
            setDraft({ ...current, url: event.target.value })
          }
          disabled={disabled}
          className="font-mono text-sm"
        />
      )}
      <p className="text-xs text-fg-2">{t("authCenter.loginProxy.hint")}</p>
      {config.isError && (
        <div className="flex items-center justify-between gap-3">
          <p role="alert" className="text-xs text-fg-2">
            {t("authCenter.loginProxy.loadFailed")}
          </p>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            onClick={() => void config.refetch()}
            disabled={disabled}
          >
            {t("common.retry")}
          </Button>
        </div>
      )}
      <div className="flex justify-end">
        <Button
          type="button"
          size="sm"
          variant="outline"
          onClick={saveConfig}
          disabled={disabled || (!changed && !config.isError)}
        >
          {save.isPending && (
            <Loader2 className="mr-1 h-3.5 w-3.5 animate-spin" />
          )}
          {t("authCenter.loginProxy.save")}
        </Button>
      </div>
    </div>
  );
}
