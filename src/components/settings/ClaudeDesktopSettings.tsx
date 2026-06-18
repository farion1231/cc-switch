import { MonitorUp, Shield } from "lucide-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { ToggleRow } from "@/components/ui/toggle-row";
import { providersApi, type ClaudeDesktopStatus } from "@/lib/api/providers";
import { extractErrorMessage } from "@/utils/errorUtils";

export function ClaudeDesktopSettings() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const { data: status, isLoading } = useQuery({
    queryKey: ["claudeDesktopStatus"],
    queryFn: () => providersApi.getClaudeDesktopStatus(),
    refetchInterval: 5000,
  });

  const mutation = useMutation({
    mutationFn: (enabled: boolean) =>
      providersApi.setClaudeDesktopDisableAutoUpdates(enabled),
    onMutate: async (enabled) => {
      await queryClient.cancelQueries({ queryKey: ["claudeDesktopStatus"] });
      const previous = queryClient.getQueryData<ClaudeDesktopStatus>([
        "claudeDesktopStatus",
      ]);

      queryClient.setQueryData<ClaudeDesktopStatus>(
        ["claudeDesktopStatus"],
        (current) =>
          current ? { ...current, disableAutoUpdates: enabled } : current,
      );

      return { previous };
    },
    onError: (error, _enabled, context) => {
      if (context?.previous) {
        queryClient.setQueryData(["claudeDesktopStatus"], context.previous);
      }
      toast.error(
        t("claudeDesktop.autoUpdates.failed", {
          error: extractErrorMessage(error),
          defaultValue: "更新 Claude Desktop 自动更新设置失败: {{error}}",
        }),
      );
    },
    onSuccess: (_result, enabled) => {
      toast.success(
        enabled
          ? t("claudeDesktop.autoUpdates.blocked", {
              defaultValue: "已关闭 Claude Desktop 自动更新",
            })
          : t("claudeDesktop.autoUpdates.allowed", {
              defaultValue: "已允许 Claude Desktop 自动更新",
            }),
      );
    },
    onSettled: async () => {
      await queryClient.invalidateQueries({
        queryKey: ["claudeDesktopStatus"],
      });
    },
  });

  const supported = status?.supported !== false;
  const configured = status?.configured === true;
  const disabled =
    isLoading || mutation.isPending || !status || !supported || !configured;
  const description = supported
    ? t("settings.disableClaudeDesktopAutoUpdatesDescription", {
        defaultValue:
          "开启后，Claude Desktop 不会自动下载和安装新版本；关闭后恢复默认自动更新。",
      })
    : t("settings.claudeDesktopUnsupportedDescription", {
        defaultValue: "当前平台暂不支持 Claude Desktop 3P 配置写入。",
      });

  return (
    <section className="space-y-4">
      <div className="flex items-center gap-2 border-b border-border/40 pb-2">
        <MonitorUp className="h-4 w-4 text-primary" />
        <h3 className="text-sm font-medium">
          {t("settings.claudeDesktopSettings", {
            defaultValue: "Claude Desktop 设置",
          })}
        </h3>
      </div>

      <div className="space-y-3">
        <ToggleRow
          icon={<Shield className="h-4 w-4 text-blue-500" />}
          title={t("settings.disableClaudeDesktopAutoUpdates", {
            defaultValue: "关闭 Claude Desktop 自动更新",
          })}
          description={description}
          checked={status?.disableAutoUpdates ?? false}
          onCheckedChange={(value) => mutation.mutate(value)}
          disabled={disabled}
        />
      </div>
    </section>
  );
}
