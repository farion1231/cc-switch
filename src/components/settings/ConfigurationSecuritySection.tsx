import { useState } from "react";
import { AlertTriangle, Loader2, Shield } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { AppId } from "@/lib/api";
import { providerSecurityApi } from "@/lib/api/providerSecurity";
import type { RecoveryMode } from "@/types/providerSecurity";

const APP_OPTIONS: { id: AppId; label: string }[] = [
  { id: "claude", label: "Claude" },
  { id: "codex", label: "Codex" },
  { id: "gemini", label: "Gemini" },
  { id: "opencode", label: "OpenCode" },
  { id: "openclaw", label: "OpenClaw" },
];

export function ConfigurationSecuritySection() {
  const [appId, setAppId] = useState<AppId>("claude");
  const [busyMode, setBusyMode] = useState<RecoveryMode | null>(null);

  const runRecover = async (mode: RecoveryMode) => {
    const confirmText =
      mode === "project_db_to_live"
        ? `确认将 ${appId} 的项目数据库凭据写回 Live 配置？这会覆盖磁盘上的当前 Live 凭据。`
        : `确认将 ${appId} 的 Live 凭据导入项目数据库？这会覆盖数据库中的当前凭据。`;
    if (!window.confirm(confirmText)) return;

    setBusyMode(mode);
    try {
      const result = await providerSecurityApi.recover(appId, mode);
      if (result.state === "consistent") {
        toast.success(
          mode === "project_db_to_live"
            ? "已用数据库修复 Live 配置"
            : "已用 Live 导入修复数据库",
        );
      } else {
        toast.warning("恢复完成，但配置仍可能不一致，请检查供应商列表");
      }
    } catch (error) {
      const message =
        error instanceof Error ? error.message : String(error ?? "");
      if (message.includes("configuration_inconsistent")) {
        toast.error("配置处于不一致锁定状态，请按提示完成恢复");
      } else if (message.includes("live_projection_failed")) {
        toast.error("Live 投影失败，请检查应用配置路径后重试");
      } else {
        toast.error(message || "恢复失败");
      }
    } finally {
      setBusyMode(null);
    }
  };

  return (
    <div className="space-y-4" data-testid="configuration-security-section">
      <div className="flex items-start gap-3">
        <Shield className="mt-0.5 h-5 w-5 text-primary" />
        <div className="space-y-1">
          <h3 className="text-sm font-medium">配置安全与恢复</h3>
          <p className="text-xs text-muted-foreground">
            当 Live
            投影失败或凭据来源不一致时，可按应用维度执行恢复。恢复不会自动合并字段；请选择单向来源。
          </p>
        </div>
      </div>

      <div className="rounded-lg border border-amber-500/30 bg-amber-500/5 p-3 text-xs text-muted-foreground">
        <div className="mb-1 flex items-center gap-1.5 font-medium text-amber-800 dark:text-amber-100">
          <AlertTriangle className="h-3.5 w-3.5" />
          解锁条件
        </div>
        <ul className="list-disc space-y-1 pl-5">
          <li>一次只恢复一个应用，避免跨应用串写。</li>
          <li>恢复完成后 revision 会前进，旧的编辑面板需重新加载。</li>
          <li>审计只记录指纹与来源，不展示原始密钥。</li>
        </ul>
      </div>

      <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
        <div className="w-full sm:w-48">
          <Select value={appId} onValueChange={(v) => setAppId(v as AppId)}>
            <SelectTrigger>
              <SelectValue placeholder="选择应用" />
            </SelectTrigger>
            <SelectContent>
              {APP_OPTIONS.map((opt) => (
                <SelectItem key={opt.id} value={opt.id}>
                  {opt.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={busyMode !== null}
            onClick={() => void runRecover("project_db_to_live")}
          >
            {busyMode === "project_db_to_live" ? (
              <Loader2 className="mr-1 h-3.5 w-3.5 animate-spin" />
            ) : null}
            数据库 → Live
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={busyMode !== null}
            onClick={() => void runRecover("import_live_to_db")}
          >
            {busyMode === "import_live_to_db" ? (
              <Loader2 className="mr-1 h-3.5 w-3.5 animate-spin" />
            ) : null}
            Live → 数据库
          </Button>
        </div>
      </div>
    </div>
  );
}
