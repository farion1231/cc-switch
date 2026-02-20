import { useTranslation } from "react-i18next";
import { Switch } from "@/components/ui/switch";
import { usePluginList, useSetPluginEnabled } from "@/hooks/usePlugins";
import type { PluginState } from "@/lib/api";

function PluginRow({ plugin }: { plugin: PluginState }) {
  const { mutate: setEnabled, isPending } = useSetPluginEnabled();
  const [name, registry] = plugin.plugin_id.split("@");

  return (
    <div className="flex items-center justify-between py-3 px-4 border-b last:border-b-0">
      <div className="flex flex-col gap-0.5">
        <span className="font-medium text-sm">{name}</span>
        <span className="text-xs text-muted-foreground">
          {registry} · {plugin.version ?? "unknown"}
        </span>
      </div>
      <Switch
        checked={plugin.enabled}
        disabled={isPending}
        onCheckedChange={(enabled) =>
          setEnabled({ pluginId: plugin.plugin_id, enabled })
        }
      />
    </div>
  );
}

export function PluginList() {
  const { t } = useTranslation();
  const { data: plugins = [], isLoading } = usePluginList();

  if (isLoading) return null;

  if (plugins.length === 0) {
    return (
      <div className="text-sm text-muted-foreground text-center py-4">
        {t("plugins.noPluginsInstalled", {
          defaultValue: "未检测到已安装插件",
        })}
      </div>
    );
  }

  return (
    <div className="rounded-md border">
      <div className="px-4 py-2 border-b bg-muted/50">
        <span className="text-xs font-medium text-muted-foreground">
          {t("plugins.title", { defaultValue: "Claude 插件" })} ({plugins.length})
        </span>
      </div>
      {plugins.map((plugin) => (
        <PluginRow key={plugin.plugin_id} plugin={plugin} />
      ))}
    </div>
  );
}
